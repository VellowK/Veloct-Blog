//! Serving an already-built directory of static files: the works preview.
//!
//! **Why this module exists, and why it is a static server rather than an Astro command.** `handoff.md`
//! §10.4 requires the work bodies on a genuinely different origin, and ADR-0004 says what that is:
//! "a second sidecar that serves the work bodies as plain static files". The work bodies ARE build
//! output — the `splitPublishRoots` integration emits `dist/works/<slug>/` during `astro build` — so
//! neither Astro serve command is the right tool for them:
//!
//! * **`dev` is wrong.** It compiles the site's SOURCES and serves the project's routes. The works root
//!   has no Astro project, no pages and no sources; `astro dev` there would serve the main site (or a
//!   failure), never the bodies that were built.
//! * **`preview` is wrong for the same reason the main site's `dev` is site-shaped.** It serves the
//!   SITE project's `outDir`, which `site/astro.config.mjs` sets to `dist/site`; the work bodies are the
//!   sibling `dist/works`, and Astro has exactly one `outDir` per project. It would also make serving
//!   the works depend on the site's `node_modules`.
//!
//! So the launcher answers `serve <directory>` itself. That also means the works preview needs **no Node
//! runtime at all**: the directory is read and the bytes on disk are the bytes served.
//!
//! The rest of this file is about not letting a work body reach anything else. A work body is untrusted
//! input (§10.4: 作品不能获得主站控制权), and this server is the isolation boundary, so a request that
//! tries to leave the served root is REFUSED rather than normalised into one that stays inside it, and a
//! `Host` header that is not loopback is refused because a name that resolves to 127.0.0.1 would
//! otherwise be same-origin with this preview.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The file served for a request that names a directory.
pub const INDEX_FILE: &str = "index.html";

/// How long one connection may take to deliver its request head, or to accept a response.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// The most of a request head that is read, so a client cannot make the server allocate without bound.
/// A `GET` with its headers fits many times over.
const MAX_REQUEST_BYTES: u64 = 8 * 1024;

/// A loopback HTTP server over one directory of files.
#[derive(Debug)]
pub struct StaticServer {
    /// The directory served, canonicalised once so that no request pays for resolving it and so that
    /// the boundary check below compares two paths of the same shape.
    root: PathBuf,
    listener: TcpListener,
    address: SocketAddr,
}

impl StaticServer {
    /// Bind a loopback listener that serves `root`.
    ///
    /// Three refusals, each for a reason that would otherwise be a silent defect:
    ///
    /// * **Port 0.** The supervisor chooses the port and records it with the preview state
    ///   (`handoff.md` §5.3); a server that let the OS pick one would answer on a port nothing knows.
    /// * **A host that is not loopback.** The bodies served here are untrusted, so exposing them on the
    ///   network is not a preview setting to be lenient about.
    /// * **A root that is not an existing directory.** A server that came up anyway would answer 404 for
    ///   everything and be reported as a running preview, which is the "pretend" §10.4 forbids.
    pub fn bind(root: &Path, host: &str, port: u16) -> Result<Self, String> {
        if port == 0 {
            return Err(
                "serve needs the port it was given, and 0 means \"choose one for me\"; the preview \
                 supervisor records the port it asked for and would never find this server"
                    .to_string(),
            );
        }

        let address = loopback_address(host)?;

        let canonical = root.canonicalize().map_err(|error| {
            format!(
                "the directory to serve, {}, cannot be read: {error}; the works preview serves the \
                 project's BUILT works root (`dist/works`), so a release build has to run before a work \
                 can be previewed",
                root.display()
            )
        })?;
        if !canonical.is_dir() {
            return Err(format!(
                "the directory to serve, {}, is not a directory",
                canonical.display()
            ));
        }

        let listener = TcpListener::bind(SocketAddr::new(address, port))
            .map_err(|error| format!("could not bind {address}:{port}: {error}"))?;
        let bound = listener
            .local_addr()
            .map_err(|error| format!("could not read the address that was bound: {error}"))?;

        Ok(Self {
            root: canonical,
            listener,
            address: bound,
        })
    }

    /// The address that was actually bound.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The port that was actually bound.
    pub fn local_port(&self) -> u16 {
        self.address.port()
    }

    /// Answer connections until the process is terminated.
    ///
    /// There is deliberately no shutdown path: the supervisor stops a preview with a process-tree kill
    /// (`handoff.md` §5.3), so a signal handler here would be a second, weaker mechanism that only one
    /// platform would exercise.
    pub fn serve(&self) -> Result<(), String> {
        // stderr, like every other diagnostic this launcher emits: stdout is not read by the supervisor.
        eprintln!(
            "veloct-astro-sidecar: serving {} as static files on http://{}/",
            self.root.display(),
            self.address
        );

        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let root = self.root.clone();
                    // One thread per connection: a work body loads several files at once and one slow
                    // client must not hold up the rest. Nothing is shared but the root, so there is no
                    // lock on the request path.
                    std::thread::spawn(move || answer(stream, &root));
                }
                // A connection abandoned before it was accepted is not a reason to stop serving the
                // preview.
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::Interrupted | std::io::ErrorKind::ConnectionAborted
                    ) => {}
                Err(error) => {
                    return Err(format!(
                        "the static preview server stopped accepting connections: {error}"
                    ))
                }
            }
        }
    }
}

/// Answer one connection and close it.
fn answer(mut stream: TcpStream, root: &Path) {
    let _ = stream.set_read_timeout(Some(CONNECTION_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONNECTION_TIMEOUT));
    // Responses here are a few hundred bytes of head plus a file; waiting for Nagle would add latency
    // to every one of them for no benefit.
    let _ = stream.set_nodelay(true);

    // Reading through a clone, because a `BufReader` owns what it reads from and the response has to be
    // written to the original stream.
    let Ok(source) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(source).take(MAX_REQUEST_BYTES);

    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(read) if read > 0 => {}
        // Nothing was sent: a probe that connected and went away, or a port scan.
        Ok(_) => return,
        Err(_) => {
            // A request head that is not text is malformed, and a client that sent one gets an answer
            // rather than a connection that closes for no stated reason.
            let _ = write_response(&mut stream, &Response::status(400, "Bad Request"), false);
            return;
        }
    }

    // The headers are read to the blank line before anything is answered: a client still writing when
    // the socket closes sees a connection reset instead of the response.
    let mut host: Option<String> = None;
    let mut header = String::new();
    loop {
        header.clear();
        match reader.read_line(&mut header) {
            Ok(0) => break,
            Ok(_) if header == "\r\n" || header == "\n" => break,
            Ok(_) => {
                if let Some((name, value)) = header.split_once(':') {
                    if name.eq_ignore_ascii_case("host") {
                        host = Some(value.trim().to_string());
                    }
                }
            }
            Err(_) => break,
        }
    }

    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        let _ = write_response(&mut stream, &Response::status(400, "Bad Request"), false);
        return;
    };

    let response = response_for(root, method, target, host.as_deref());
    let _ = write_response(&mut stream, &response, method == "HEAD");
}

/// One response, before it is written to a socket.
struct Response {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    /// A response with no body, for a refusal.
    fn status(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        }
    }

    /// A file that was read.
    fn file(body: Vec<u8>, content_type: &'static str) -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type,
            body,
        }
    }
}

/// The response for one request.
///
/// A pure function of the root, the method, the target and the `Host` header, so every rule the server
/// enforces is a unit test rather than a socket conversation.
fn response_for(root: &Path, method: &str, target: &str, host: Option<&str>) -> Response {
    // A preview only reads: nothing in it writes, and a work body that tries gets an answer that says so.
    if method != "GET" && method != "HEAD" {
        return Response::status(405, "Method Not Allowed");
    }

    // §10.4 hardening. A page on a non-loopback origin that resolves to 127.0.0.1 (DNS rebinding) would
    // otherwise be SAME-ORIGIN with this preview and able to read the work bodies it is meant to be
    // isolated from. A browser always names the host it believes it is talking to, so requiring loopback
    // closes that door without costing a real client anything.
    if let Some(host) = host {
        if !is_loopback_host(host) {
            return Response::status(403, "Forbidden");
        }
    }

    let Some(relative) = safe_relative_path(target) else {
        return Response::status(400, "Bad Request");
    };

    let mut candidate = root.join(relative);
    if candidate.is_dir() {
        candidate = candidate.join(INDEX_FILE);
    }

    // The root is canonicalised HERE rather than trusted from the caller, because the boundary check
    // below is a path-prefix comparison and that is only meaningful between two canonical paths
    // (`\\?\C:\...` on Windows never has a non-canonical prefix). It costs one `stat` on loopback.
    let Ok(root) = root.canonicalize() else {
        return Response::status(404, "Not Found");
    };

    // `canonicalize` is also what makes the root a real boundary rather than a string prefix: a symlink
    // inside the root that points outside it resolves here and is refused.
    let Ok(canonical) = candidate.canonicalize() else {
        return Response::status(404, "Not Found");
    };
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Response::status(404, "Not Found");
    }

    match std::fs::read(&canonical) {
        Ok(body) => Response::file(body, content_type(&canonical)),
        // A file that exists but cannot be read is not a 500 the user can act on, and "not found" is what
        // an unreadable directory entry looks like from here.
        Err(_) => Response::status(404, "Not Found"),
    }
}

/// The path inside the root that a request target names, or `None` when it may not be served.
fn safe_relative_path(target: &str) -> Option<PathBuf> {
    // Origin-form only, and the query or fragment is not part of the path. An absolute-form target
    // (`http://host/path`) is refused rather than parsed: nothing that talks to a preview sends one, and
    // a client that does is not a client this server has to guess about.
    let path = target.split(['?', '#']).next()?;
    let decoded = percent_decode(path)?;
    if !decoded.starts_with('/') || decoded.contains('\0') {
        return None;
    }

    let mut relative = PathBuf::new();
    for segment in decoded.split('/') {
        match segment {
            // An empty segment is a doubled or trailing slash, and `.` is a no-op.
            "" | "." => continue,
            // **A refusal, not a normalisation.** A request that tries to leave the root is not a
            // request for a file inside it, and answering 404 for a quietly normalised path would make
            // the two indistinguishable in a log.
            ".." => return None,
            _ => {}
        }
        // A separator or a drive/stream name smuggled through percent-encoding would be a second path
        // syntax inside one segment. Both are refused rather than interpreted.
        if segment.contains('\\') || segment.contains(':') {
            return None;
        }
        relative.push(segment);
    }
    Some(relative)
}

/// Percent-decode a URL path, or `None` when it is not a valid one.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let digits = bytes.get(index + 1..index + 3)?;
            let high = hex_digit(digits[0])?;
            let low = hex_digit(digits[1])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    // Decoding is byte-wise because a percent-encoded path is bytes; what comes out has to be a path a
    // filesystem can be given, and this one is.
    String::from_utf8(decoded).ok()
}

/// The value of one hexadecimal digit.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Whether a `Host` header names the loopback interface.
///
/// The port is ignored: it is the host that decides an origin. An IPv6 literal arrives bracketed and
/// usually with a port (`[::1]:4321`).
fn is_loopback_host(host: &str) -> bool {
    let host = host.trim();
    let host = host.strip_prefix('[').unwrap_or(host);
    let host = match host.find(']') {
        Some(end) => &host[..end],
        None => host.split(':').next().unwrap_or(host),
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

/// The address to bind, refusing anything that is not loopback.
fn loopback_address(host: &str) -> Result<IpAddr, String> {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    let parsed = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .map_err(|_| format!("{host:?} is not an IP address or `localhost`"))?;
    if !parsed.is_loopback() {
        return Err(format!(
            "the works preview serves untrusted work bodies, so it binds loopback only (handoff.md \
             §10.4); {parsed} would expose them on the network"
        ));
    }
    Ok(parsed)
}

/// The content type for a file, from its extension.
///
/// A small table rather than a dependency, covering what an Astro build emits. An unknown extension is
/// `application/octet-stream` on purpose: a guessed type is worse than a generic one, and
/// `X-Content-Type-Options: nosniff` stops a client from second-guessing it.
fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "wasm" => "application/wasm",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Write one response and close.
///
/// `Connection: close` is stated rather than implied, so the body ends at the end of the stream and
/// neither side needs keep-alive framing. No `Access-Control-Allow-Origin` is sent, and that is the
/// point: the main site must not be able to read a work body, which is what §10.4's separate origin and
/// sandboxed iframe are for.
fn write_response(
    stream: &mut TcpStream,
    response: &Response,
    head_only: bool,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\
         \r\n",
        response.status,
        response.reason,
        response.content_type,
        response.body.len()
    );
    stream.write_all(head.as_bytes())?;
    if !head_only {
        stream.write_all(&response.body)?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// A directory holding the given files, creating parent directories as needed.
    fn root_with(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, body) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
        }
        dir
    }

    fn body_of(response: &Response) -> String {
        String::from_utf8_lossy(&response.body).to_string()
    }

    /// One raw HTTP request, answered to EOF: the server states `Connection: close`, so the end of the
    /// response is the end of the stream.
    fn request(port: u16, raw: &str) -> String {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
            .unwrap_or_else(|error| panic!("could not connect to {address}: {error}"));
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.write_all(raw.as_bytes()).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn a_built_file_is_served_byte_for_byte_with_its_content_type() {
        let root = root_with(&[("demo-counter/index.html", "<h1>counter</h1>")]);
        let response = response_for(root.path(), "GET", "/demo-counter/index.html", None);

        assert_eq!(response.status, 200, "{}", response.reason);
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        assert_eq!(body_of(&response), "<h1>counter</h1>");
    }

    #[test]
    fn a_directory_request_serves_the_index_inside_it() {
        let root = root_with(&[("demo-counter/index.html", "<h1>counter</h1>")]);
        // Both spellings a host would emit, because `build.format: 'directory'` plus
        // `trailingSlash: 'always'` means the built output and the URL disagree about the slash.
        for target in ["/demo-counter/", "/demo-counter"] {
            let response = response_for(root.path(), "GET", target, None);
            assert_eq!(response.status, 200, "{target}: {}", response.reason);
            assert_eq!(body_of(&response), "<h1>counter</h1>", "{target}");
        }
    }

    #[test]
    fn a_root_request_is_served_when_the_works_root_has_an_index() {
        let root = root_with(&[("index.html", "<h1>works</h1>")]);
        let response = response_for(root.path(), "GET", "/", None);
        assert_eq!(response.status, 200, "{}", response.reason);
        assert_eq!(body_of(&response), "<h1>works</h1>");
    }

    #[test]
    fn a_missing_file_and_a_directory_without_an_index_are_a_404() {
        let root = root_with(&[("demo-counter/index.html", "x")]);
        std::fs::create_dir_all(root.path().join("demo-counter/assets")).unwrap();

        for target in ["/nothing.html", "/demo-counter/assets/", "/"] {
            let response = response_for(root.path(), "GET", target, None);
            assert_eq!(response.status, 404, "{target}: {}", response.reason);
        }
    }

    #[test]
    fn a_request_that_tries_to_leave_the_root_is_refused_rather_than_resolved() {
        // A work body is untrusted input (`handoff.md` §10.4), and reading a file outside the works root
        // is exactly the failure this boundary exists to prevent.
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("outside.txt"), "must never be served").unwrap();
        let served = dir.path().join("served");
        std::fs::create_dir_all(&served).unwrap();
        std::fs::write(served.join("index.html"), "inside").unwrap();

        for target in [
            "/../outside.txt",
            "/%2e%2e/outside.txt",
            "/..%2foutside.txt",
            "/a/../../outside.txt",
        ] {
            let response = response_for(&served, "GET", target, None);
            assert_eq!(
                response.status, 400,
                "{target} must be a refusal rather than a miss, got {} {}",
                response.status, response.reason
            );
        }

        // And the file inside is still served, so the refusal is about the path rather than about
        // everything.
        assert_eq!(response_for(&served, "GET", "/index.html", None).status, 200);
    }

    #[test]
    fn a_method_that_is_not_get_or_head_is_refused() {
        let root = root_with(&[("index.html", "x")]);
        let response = response_for(root.path(), "POST", "/", None);
        assert_eq!(response.status, 405);
        assert_eq!(response.reason, "Method Not Allowed");
    }

    #[test]
    fn a_target_that_is_not_an_origin_form_path_is_refused() {
        let root = root_with(&[("index.html", "x")]);
        for target in ["http://evil.example/index.html", "/bad%zz", "/bad%2"] {
            assert_eq!(
                response_for(root.path(), "GET", target, None).status,
                400,
                "{target}"
            );
        }
    }

    #[test]
    fn the_query_string_is_not_part_of_the_path() {
        // The shell page's iframe URL may carry a cache-buster, and a built asset is still that asset.
        let root = root_with(&[("app.js", "console.log(1)")]);
        let response = response_for(root.path(), "GET", "/app.js?v=2", None);
        assert_eq!(response.status, 200, "{}", response.reason);
        assert_eq!(body_of(&response), "console.log(1)");
    }

    #[test]
    fn a_host_header_that_is_not_loopback_is_refused() {
        let root = root_with(&[("index.html", "x")]);
        for host in ["127.0.0.1:4321", "127.0.0.1", "localhost", "localhost:4321", "[::1]:4321"] {
            assert_eq!(
                response_for(root.path(), "GET", "/", Some(host)).status,
                200,
                "{host} is loopback and must be served"
            );
        }
        // A name that resolves to 127.0.0.1 would be same-origin with this preview, which is the one
        // thing §10.4 isolates the work bodies from.
        for host in ["evil.example", "evil.example:4321", "192.168.1.10:4321", "[2001:db8::1]:4321"] {
            assert_eq!(
                response_for(root.path(), "GET", "/", Some(host)).status,
                403,
                "{host} is not loopback and must be refused"
            );
        }
    }

    #[test]
    fn an_unknown_extension_is_served_as_bytes_rather_than_guessed() {
        let root = root_with(&[("works-manifest.bin", "binary-ish")]);
        let response = response_for(root.path(), "GET", "/works-manifest.bin", None);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/octet-stream");
    }

    #[test]
    fn the_manifest_the_build_emits_is_served_as_json() {
        let root = root_with(&[("works-manifest.json", "{}")]);
        assert_eq!(
            response_for(root.path(), "GET", "/works-manifest.json", None).content_type,
            "application/json; charset=utf-8"
        );
    }

    #[test]
    fn binding_refuses_a_port_the_supervisor_could_not_find() {
        let root = root_with(&[("index.html", "x")]);
        let error = StaticServer::bind(root.path(), "127.0.0.1", 0).unwrap_err();
        assert!(error.contains("port"), "{error}");
    }

    #[test]
    fn binding_refuses_a_host_that_would_expose_work_bodies_on_the_network() {
        let root = root_with(&[("index.html", "x")]);
        let error = StaticServer::bind(root.path(), "0.0.0.0", 4321).unwrap_err();
        assert!(error.contains("loopback"), "{error}");
        // And a host that is not an address at all is reported as that, rather than as a bind failure.
        let error = StaticServer::bind(root.path(), "not-a-host", 4321).unwrap_err();
        assert!(error.contains("not an IP address"), "{error}");
    }

    #[test]
    fn binding_refuses_a_directory_that_is_not_there() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("dist").join("works");
        let error = StaticServer::bind(&missing, "127.0.0.1", 4321).unwrap_err();

        // The message names the path that was wanted AND the step that produces it: "the works preview
        // failed" is not actionable.
        assert!(error.contains(&missing.display().to_string()), "{error}");
        assert!(error.contains("BUILT"), "{error}");
        assert!(error.contains("build"), "{error}");
    }

    /// A free loopback port, obtained the way the supervisor obtains one.
    fn free_port() -> u16 {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    /// A real socket conversation, so the framing rules are exercised rather than the pure function's.
    #[test]
    fn the_server_answers_a_real_request_and_closes_the_connection() {
        let root = root_with(&[("demo-counter/index.html", "<h1>counter</h1>")]);
        let port = free_port();
        let server = Arc::new(StaticServer::bind(root.path(), "127.0.0.1", port).unwrap());
        assert_eq!(
            server.local_port(),
            port,
            "the server must bind the port it was given, because the supervisor records that port"
        );

        // The accept loop is detached and blocks on `accept` until the process exits, which is the same
        // lifecycle the supervisor gives it (a tree kill, `handoff.md` §5.3).
        let running = Arc::clone(&server);
        std::thread::spawn(move || {
            let _ = running.serve();
        });

        let get = request(port, "GET /demo-counter/ HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert!(get.starts_with("HTTP/1.1 200 OK\r\n"), "{get}");
        assert!(get.contains("Content-Type: text/html; charset=utf-8"), "{get}");
        assert!(get.contains("Connection: close"), "{get}");
        assert!(get.ends_with("<h1>counter</h1>"), "{get}");

        // HEAD carries the headers the GET would have carried and no body.
        let head = request(port, "HEAD /demo-counter/ HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
        assert!(head.contains("Content-Length: 16"), "{head}");
        assert!(!head.contains("<h1>"), "{head}");

        // A refusal is answered over the socket too, so a work body that asks for a path outside the
        // root is told no rather than left with a closed connection.
        let refused = request(port, "GET /../outside.txt HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert!(refused.starts_with("HTTP/1.1 400 Bad Request\r\n"), "{refused}");
    }
}

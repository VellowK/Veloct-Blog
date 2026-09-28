//! Free-port selection.
//!
//! `handoff.md` §5.3 requires Veloct to pick an available local port rather than hard-coding one,
//! and to never leave a stale instance holding it. Two details matter:
//!
//! * The port is discovered by binding `127.0.0.1:0` and reading back what the OS assigned. That is
//!   the only race-free way to ask "give me a free port"; `portpicker` was last published in 2021
//!   and is not used.
//! * A port that was free a moment ago may not be free when the child starts, so the caller retries
//!   rather than treating the first answer as a guarantee. Two processes cannot bind the same port,
//!   but the gap between "check" and "spawn" is real.

use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::time::{Duration, Instant};

use veloct_core::{Result, VeloctError};

/// A port the OS just told us was free.
///
/// Deliberately does not hold the listener open: the child needs to bind the port itself, so the
/// reservation is released immediately. The value is therefore a strong hint, not a lock, which is
/// why [`reserve_local_port`] retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortReservation {
    pub port: u16,
}

impl PortReservation {
    pub fn as_u16(self) -> u16 {
        self.port
    }
}

/// Ask the OS for a free loopback port.
pub fn pick_free_port() -> Result<u16> {
    // Binding port 0 asks the kernel to choose; the assignment is read back from the socket.
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).map_err(|e| {
        VeloctError::Preview(format!("could not ask the OS for a free port: {e}"))
    })?;
    let port = listener
        .local_addr()
        .map_err(|e| VeloctError::Preview(format!("could not read the assigned port: {e}")))?
        .port();
    // Drop the listener so the child can bind the port.
    drop(listener);
    Ok(port)
}

/// Pick a port and confirm it is still bindable immediately afterwards.
pub fn reserve_local_port() -> Result<PortReservation> {
    // A single retry loop rather than a bare call, because between releasing the probe listener and
    // the child binding, another process on the machine could take the port.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut last_error: Option<String> = None;

    while Instant::now() < deadline {
        match pick_free_port() {
            Ok(port) => match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)) {
                Ok(probe) => {
                    drop(probe);
                    return Ok(PortReservation { port });
                }
                Err(error) => last_error = Some(error.to_string()),
            },
            Err(error) => last_error = Some(error.to_string()),
        }

        std::thread::sleep(Duration::from_millis(25));
    }

    Err(VeloctError::Preview(format!(
        "could not find a free local port within the retry window: {}",
        last_error.unwrap_or_else(|| "unknown reason".to_string())
    )))
}

/// Whether a port is currently free.
///
/// Used to report a stale holder rather than to reserve: the same check-then-use gap applies.
pub fn is_port_free(port: u16) -> bool {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_ok()
}

/// Whether a local HTTP server is answering on a port.
///
/// This is the readiness probe. An Astro dev server prints a ready line, but the only durable proof
/// that the preview can actually serve a page is a connection that succeeds.
pub fn is_http_ready(port: u16, timeout: Duration) -> bool {
    use std::io::Write;
    use std::net::TcpStream;

    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let Ok(mut stream) = TcpStream::connect_timeout(&address.into(), timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    // A minimal HTTP/1.0 request: HTTP/1.0 keeps the server from holding the connection open with
    // chunked encoding, so a single read is enough to know something answered.
    let request = "GET / HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut buffer = [0u8; 64];
    match std::io::Read::read(&mut stream, &mut buffer) {
        Ok(read) if read > 0 => {
            // Any HTTP status means a server is there, including a 404 for a route that does not
            // exist yet. Requiring 200 would fail on a project whose index is not built.
            let head = String::from_utf8_lossy(&buffer[..read]);
            head.starts_with("HTTP/")
        }
        _ => false,
    }
}

/// Wait until a port answers HTTP, or give up.
pub fn wait_for_http(port: u16, total: Duration, interval: Duration) -> bool {
    let deadline = Instant::now() + total;
    loop {
        if is_http_ready(port, Duration::from_millis(500)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_port_is_actually_free() {
        let port = pick_free_port().unwrap();
        assert_ne!(port, 0, "the OS must assign a real port");
        assert!(is_port_free(port), "the assigned port must be bindable");
    }

    #[test]
    fn two_picks_do_not_collide_while_both_are_held() {
        // The OS hands out distinct ports for concurrently held listeners.
        let first = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let second = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        assert_ne!(
            first.local_addr().unwrap().port(),
            second.local_addr().unwrap().port()
        );
    }

    #[test]
    fn a_bound_port_reports_as_not_free() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!is_port_free(port));
        drop(listener);
        assert!(is_port_free(port), "dropping the listener frees the port");
    }

    #[test]
    fn reserving_returns_a_bindable_port() {
        let reservation = reserve_local_port().unwrap();
        assert!(reservation.as_u16() > 0);
        assert!(is_port_free(reservation.as_u16()));
    }

    #[test]
    fn readiness_detects_a_real_http_server() {
        use std::io::{Read, Write};

        // A one-shot HTTP server, so the probe is tested against a real socket conversation rather
        // than a mock.
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 256];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });

        assert!(wait_for_http(port, Duration::from_secs(3), Duration::from_millis(20)));
        handle.join().unwrap();
    }

    #[test]
    fn readiness_reports_false_when_nothing_is_listening() {
        let port = pick_free_port().unwrap();
        assert!(!is_http_ready(port, Duration::from_millis(200)));
    }

    #[test]
    fn readiness_gives_up_rather_than_hanging() {
        // handoff.md section 5.3: a preview that never becomes ready must be an explicit failure,
        // not an indefinite wait.
        let port = pick_free_port().unwrap();
        let started = Instant::now();
        let ready = wait_for_http(port, Duration::from_millis(300), Duration::from_millis(50));
        assert!(!ready);
        assert!(started.elapsed() < Duration::from_secs(3), "the wait must be bounded");
    }

    #[test]
    fn a_non_http_listener_is_not_reported_as_ready() {
        use std::io::{Read, Write};

        // Something is listening, but it does not speak HTTP. A naive "port is open" probe would
        // report ready here, which would surface later as a blank preview frame.
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 256];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"not http at all");
            }
        });

        assert!(!is_http_ready(port, Duration::from_millis(500)));
    }
}

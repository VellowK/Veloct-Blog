//! EdgeOne Pages/Makers direct upload, verified against the official Makers SDK 0.1.0
//! (contract 0.1.39): https://unpkg.com/@edgeone/makers-sdk@0.1.0/dist/client.js
//! and COS request signing: https://cloud.tencent.com/document/product/436/7778.
//! No CLI installation, account auto-discovery, region fallback, or automatic write retries.

use std::collections::BTreeSet;
use std::io::{Cursor, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use veloct_core::http::{HttpClient, HttpRequest};
use veloct_core::{Result, VeloctError};

use crate::{
    DeployAdapter, DeployCapabilities, FrozenTarget, PushOutcome, PushPayload, SubTarget,
    TargetKind,
};

pub struct EdgeOnePagesAdapter {
    target: FrozenTarget,
    token: String,
    client: Arc<dyn HttpClient>,
    poll_interval: Duration,
}

impl EdgeOnePagesAdapter {
    pub fn new(target: FrozenTarget, token: String, client: Arc<dyn HttpClient>) -> Self {
        Self {
            target,
            token,
            client,
            poll_interval: Duration::from_secs(5),
        }
    }

    fn endpoint(&self) -> &'static str {
        match self.target.kind {
            TargetKind::EdgeOneMainland => "https://pages-api.cloud.tencent.com/v1",
            TargetKind::EdgeOneInternational => "https://pages-api.edgeone.ai/v1",
            _ => unreachable!("only EdgeOne targets may use the EdgeOne adapter"),
        }
    }

    fn fail(&self, sub: SubTarget, message: impl Into<String>) -> VeloctError {
        VeloctError::Deploy {
            target: self.target.id.clone(),
            sub_target: sub.as_str().into(),
            message: message.into(),
        }
    }

    fn project(&self, sub: SubTarget) -> Result<&str> {
        if !matches!(
            self.target.kind,
            TargetKind::EdgeOneMainland | TargetKind::EdgeOneInternational
        ) {
            return Err(self.fail(sub, "不是 EdgeOne 部署目标。"));
        }
        if self
            .target
            .works_project
            .as_ref()
            .is_some_and(|works| Some(works) == self.target.project.as_ref())
        {
            return Err(self.fail(
                sub,
                "主站与作品站必须使用不同的 EdgeOne 项目 ID，避免互相覆盖。",
            ));
        }
        let project = match sub {
            SubTarget::Main => self.target.project.as_deref(),
            SubTarget::Works => self.target.works_project.as_deref(),
        }
        .unwrap_or_default();
        if !identifier(project) {
            return Err(self.fail(sub, "EdgeOne 需要有效的项目 ID（不是项目名称或网址）。"));
        }
        Ok(project)
    }

    fn call(&self, sub: SubTarget, action: &str, mut body: Value) -> Result<Value> {
        body["Action"] = json!(action);
        // Never attach the Pages token to COS, and never echo vendor bodies/transport errors:
        // either can contain API tokens or the short-lived upload credentials.
        let response = self
            .client
            .execute(
                HttpRequest::post(self.endpoint())
                    .bearer(&self.token)
                    .header("User-Agent", "Veloct/1.0 EdgeOne-direct-upload")
                    .json(body)
                    .timeout(Duration::from_secs(30)),
            )
            .map_err(|_| {
                self.fail(
                    sub,
                    format!("EdgeOne {action} 网络请求失败或超时；未自动重试。"),
                )
            })?;
        let value: Value = serde_json::from_slice(&response.body).map_err(|_| {
            self.fail(
                sub,
                format!(
                    "EdgeOne {action} 响应无法解析（HTTP {}）。",
                    response.status
                ),
            )
        })?;
        let code_ok = value
            .get("Code")
            .is_some_and(|code| code == 0 || code == "0");
        let data = value.pointer("/Data/Response");
        if !response.is_success()
            || !code_ok
            || data.is_none()
            || data.is_some_and(|v| v.get("Error").is_some())
        {
            return Err(self.fail(sub, format!("EdgeOne {action} 请求被拒绝（HTTP {}）；请核对所选区域、API Token、项目权限与额度。未自动重试。", response.status)));
        }
        Ok(data.cloned().unwrap_or_default())
    }

    fn check_project(&self, sub: SubTarget, project: &str) -> Result<()> {
        let response = self.call(
            sub,
            "DescribePagesProjects",
            json!({
                "Filters": [{"Name": "ProjectId", "Values": [project]}], "Offset": 0, "Limit": 1,
            }),
        )?;
        let found = response["Projects"].as_array().and_then(|items| {
            items
                .iter()
                .find(|item| item["ProjectId"].as_str() == Some(project))
        });
        match found {
            Some(record) if record["Provider"] == "Upload" => Ok(()),
            Some(_) => Err(self.fail(
                sub,
                "EdgeOne 项目不是直接上传（Upload）类型；不会覆盖 Git 集成项目。",
            )),
            None => Err(self.fail(
                sub,
                "当前 EdgeOne 区域下找不到此项目；请核对项目 ID、区域与 Token。",
            )),
        }
    }

    fn upload(&self, sub: SubTarget, credentials: &Value, archive: Vec<u8>) -> Result<String> {
        let field = |path: &str| {
            credentials
                .pointer(path)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        };
        let (Some(bucket), Some(region), Some(prefix), Some(id), Some(key), Some(token)) = (
            field("/Bucket"),
            field("/Region"),
            field("/TargetPath"),
            field("/Credentials/TmpSecretId"),
            field("/Credentials/TmpSecretKey"),
            field("/Credentials/Token"),
        ) else {
            return Err(self.fail(sub, "EdgeOne 临时上传凭据不完整；未上传或创建部署。"));
        };
        // Construct, don't trust a returned URL. A malicious response cannot turn this into SSRF.
        if !dns_label(bucket) || !dns_label(region) {
            return Err(self.fail(sub, "EdgeOne 返回了无效的 COS 存储位置。"));
        }
        let object = format!("{}/artifact.zip", prefix.trim_matches('/'));
        if !safe_path(&object) {
            return Err(self.fail(sub, "EdgeOne 返回了无效的上传路径。"));
        }
        let host = format!("{bucket}.cos.accelerate.myqcloud.com");
        let path = format!(
            "/{}",
            object.split('/').map(encode).collect::<Vec<_>>().join("/")
        );
        let start = time::OffsetDateTime::now_utc().unix_timestamp() - 60;
        let authorization = cos_authorization(
            id,
            key,
            token,
            &host,
            &format!("/{object}"),
            start,
            start + 900,
        );
        let response = self
            .client
            .execute(
                HttpRequest::put(format!("https://{host}{path}"))
                    .header("Authorization", authorization)
                    .header("x-cos-security-token", token)
                    .header("Host", &host)
                    .header("Content-Type", "application/zip")
                    .bytes(archive)
                    .timeout(Duration::from_secs(300)),
            )
            .map_err(|_| self.fail(sub, "EdgeOne COS 上传失败或超时；未创建部署，未自动重试。"))?;
        if !response.is_success() {
            return Err(self.fail(
                sub,
                format!(
                    "EdgeOne COS 上传被拒绝（HTTP {}）；未创建部署。",
                    response.status
                ),
            ));
        }
        Ok(object)
    }

    fn wait(&self, sub: SubTarget, project: &str, deployment: &str) -> Result<()> {
        let started = Instant::now();
        for _ in 0..180 {
            if started.elapsed() >= Duration::from_secs(900) {
                break;
            }
            let response = self.call(
                sub,
                "DescribePagesDeployments",
                json!({
                    "ProjectId": project, "DeploymentIds": [deployment], "Offset": 0, "Limit": 1,
                }),
            )?;
            let record = response["Deployments"]
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .find(|v| v["DeploymentId"].as_str() == Some(deployment))
                })
                .or_else(|| {
                    response
                        .get("Deployment")
                        .filter(|v| v["DeploymentId"].as_str() == Some(deployment))
                });
            match record.and_then(|v| v["Status"].as_str()) {
                Some("Success") => return Ok(()),
                Some("Failed" | "Timeout" | "Cancelled" | "Invalid") => {
                    return Err(self.fail(
                        sub,
                        "EdgeOne 部署未成功，请在控制台查看该部署日志；已上传的其他站点不受影响。",
                    ));
                }
                Some(_) => (),
                None => return Err(self.fail(
                    sub,
                    "EdgeOne 未返回对应部署的状态，不能确认上线；请到控制台检查，勿盲目重复部署。",
                )),
            }
            std::thread::sleep(self.poll_interval);
        }
        Err(self.fail(
            sub,
            "EdgeOne 部署状态等待超时，不能确认上线；请到控制台检查。未自动重新上传。",
        ))
    }
}

impl DeployAdapter for EdgeOnePagesAdapter {
    fn target(&self) -> &FrozenTarget {
        &self.target
    }
    fn capabilities(&self) -> DeployCapabilities {
        DeployCapabilities {
            verified: true,
            automatic: true,
            needs_credential: true,
        }
    }
    fn push(&self, payload: &PushPayload) -> Result<PushOutcome> {
        let sub = payload.sub_target;
        if self.token.trim().is_empty() {
            return Err(self.fail(sub, "请先保存 EdgeOne API Token。"));
        }
        let project = self.project(sub)?;
        let archive = archive(payload).map_err(|_| {
            self.fail(
                sub,
                "发布产物无法安全打包（路径、敏感文件、重复文件或大小不合法）；请重新构建。",
            )
        })?;
        self.check_project(sub, project)?;
        let credentials = self.call(
            sub,
            "DescribePagesCosTempToken",
            json!({"ProjectId": project}),
        )?;
        let object = self.upload(sub, &credentials, archive)?;
        let response = self.call(sub, "CreatePagesDeployment", json!({
            "ProjectId": project, "Env": "Production", "ViaMeta": "Upload", "Provider": "Upload",
            "DistType": "Zip", "TempBucketPath": object,
        }))?;
        let deployment = response["DeploymentId"]
            .as_str()
            .filter(|id| identifier(id))
            .ok_or_else(|| {
                self.fail(
                    sub,
                    "EdgeOne 未返回有效部署 ID，不能确认上线；请在控制台检查。",
                )
            })?;
        self.wait(sub, project, deployment)?;
        Ok(PushOutcome {
            files: payload.file_count(),
            message: format!(
                "EdgeOne 已确认 Production 部署成功（{}）；{} 个文件。",
                deployment.replace(&self.token, "[redacted]"),
                payload.file_count()
            ),
            // Don't invent a public URL or leak eo_token access URLs; the console owns domain setup.
            url: None,
            manual: false,
        })
    }
    fn test_connection(&self) -> Result<String> {
        let project = self.project(SubTarget::Main)?;
        self.check_project(SubTarget::Main, project)?;
        Ok("EdgeOne 项目可访问，且支持直接上传。".into())
    }
}

pub(crate) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn dns_label(value: &str) -> bool {
    identifier(value)
        && value.len() <= 63
        && !value.contains('_')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

/// Reject instead of silently omitting private files. .well-known remains publishable.
/// Also used while reading disk, before symlinks or accidentally copied project directories can escape.
pub(crate) fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0', '?', '#', '\r', '\n'])
        && path.split('/').all(|part| {
            let lower = part.to_ascii_lowercase();
            !part.is_empty()
                && part != "."
                && part != ".."
                && !matches!(
                    lower.as_str(),
                    ".git" | ".veloct" | "node_modules" | ".ssh" | ".cargo" | ".cargo-home" | ".cache" | ".attachments-cache"
                )
                && lower != ".env"
                && !lower.starts_with(".env.")
                && !matches!(lower.as_str(), "id_rsa" | "id_ed25519" | ".npmrc" | ".git-credentials")
                && !lower.ends_with(".pem")
                && !lower.ends_with(".key")
                && !lower.ends_with(".p12")
                && !lower.ends_with(".pfx")
        })
}

fn archive(payload: &PushPayload) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    if payload.files.is_empty() || payload.total_bytes() > 512 * 1024 * 1024 {
        return Err("empty or oversized artifact".into());
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut seen = BTreeSet::new();
    for file in &payload.files {
        if !safe_path(&file.relative_path) || !seen.insert(&file.relative_path) {
            return Err("unsafe or duplicate artifact path".into());
        }
        zip.start_file(&file.relative_path, options)?;
        zip.write_all(&file.bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn hmac(key: &[u8], value: &str) -> String {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(value.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
fn cos_authorization(
    id: &str,
    key: &str,
    token: &str,
    host: &str,
    path: &str,
    start: i64,
    end: i64,
) -> String {
    let times = format!("{start};{end}");
    let sign_key = hmac(key.as_bytes(), &times);
    let http_string = format!(
        "put\n{path}\n\nhost={}&x-cos-security-token={}\n",
        encode(host),
        encode(token)
    );
    let string_to_sign = format!(
        "sha1\n{times}\n{}\n",
        hex::encode(Sha1::digest(http_string.as_bytes()))
    );
    let signature = hmac(sign_key.as_bytes(), &string_to_sign);
    format!(
        "q-sign-algorithm=sha1&q-ak={id}&q-sign-time={times}&q-key-time={times}&q-header-list=host;x-cos-security-token&q-url-param-list=&q-signature={signature}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PushFile, TargetConfig};
    use veloct_core::http::{FakeHttpClient, HttpBody};

    fn api(data: Value) -> String {
        json!({"Code": 0, "Data": {"Response": data}}).to_string()
    }
    fn project(provider: &str) -> String {
        api(json!({"Projects": [{"ProjectId": "proj-main", "Provider": provider}]}))
    }
    fn credentials() -> Value {
        json!({
            "Bucket": "upload-123", "Region": "ap-singapore", "TargetPath": "/temp/中文 site/",
            "Credentials": {"TmpSecretId": "temporary-id", "TmpSecretKey": "temporary-key", "Token": "temporary-token"}
        })
    }
    fn successful_client(status: &str) -> FakeHttpClient {
        FakeHttpClient::new()
            .always(200, project("Upload"))
            .always(200, api(credentials()))
            .always(200, "")
            .always(200, api(json!({"DeploymentId": "deploy-1"})))
            .always(
                200,
                api(json!({"Deployments": [{"DeploymentId": "deploy-1", "Status": status}]})),
            )
    }
    fn adapter(kind: TargetKind, client: &FakeHttpClient) -> EdgeOnePagesAdapter {
        let mut target = TargetConfig::new("fixture", kind);
        target.project = Some("proj-main".into());
        target.works_project = Some("proj-works".into());
        let mut adapter = EdgeOnePagesAdapter::new(
            (&target).into(),
            "private-pages-token".into(),
            Arc::new(client.clone()),
        );
        adapter.poll_interval = Duration::ZERO;
        adapter
    }
    fn payload(sub: SubTarget) -> PushPayload {
        PushPayload {
            sub_target: sub,
            root: "dist/site".into(),
            files: vec![
                PushFile::new("index.html", b"<h1>site</h1>".to_vec()),
                PushFile::new("assets/中文 image.png", vec![0, 1, 255]),
                PushFile::new(".well-known/security.txt", b"contact".to_vec()),
            ],
        }
    }
    #[test]
    fn official_cos_sdk_signature_vector_matches_including_unicode_and_temporary_token() {
        // Generated with cos-nodejs-sdk-v5 2.15.4 sdk/util.js getAuth(), not our implementation.
        let signature = cos_authorization(
            "fixture-id",
            "fixture-key",
            "fixture/token+=",
            "test-123.cos.accelerate.myqcloud.com",
            "/upload/中文 site/artifact.zip",
            1700000000,
            1700000900,
        );
        assert!(
            signature.ends_with("q-signature=709c00866619af97f8c9397df3c392c7cff6b67d"),
            "{signature}"
        );
    }
    #[test]
    fn both_regions_use_official_protocol_and_require_confirmed_success() {
        for kind in [
            TargetKind::EdgeOneMainland,
            TargetKind::EdgeOneInternational,
        ] {
            let client = successful_client("Success");
            let adapter = adapter(kind, &client);
            let result = adapter.push(&payload(SubTarget::Main)).unwrap();
            assert_eq!(result.files, 3);
            assert!(!result.manual);
            let requests = client.recorded();
            assert_eq!(requests.len(), 5);
            let actions: Vec<_> = requests
                .iter()
                .filter_map(|r| {
                    r.body
                        .as_json()
                        .map(|v| v["Action"].as_str().unwrap().to_owned())
                })
                .collect();
            assert_eq!(
                actions,
                [
                    "DescribePagesProjects",
                    "DescribePagesCosTempToken",
                    "CreatePagesDeployment",
                    "DescribePagesDeployments"
                ]
            );
            for request in requests.iter().filter(|r| r.body.as_json().is_some()) {
                assert_eq!(request.url, adapter.endpoint());
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|(k, v)| k == "Authorization" && v == "Bearer private-pages-token")
                );
            }
            let upload = &requests[2];
            assert_eq!(
                upload.url,
                "https://upload-123.cos.accelerate.myqcloud.com/temp/%E4%B8%AD%E6%96%87%20site/artifact.zip"
            );
            assert!(!format!("{:?}", upload.headers).contains("private-pages-token"));
            assert!(!format!("{:?}", upload.headers).contains("temporary-key"));
            let HttpBody::Bytes(bytes) = &upload.body else {
                panic!("ZIP bytes required")
            };
            let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            assert_eq!(zip.len(), 3);
            use std::io::Read;
            let mut image = Vec::new();
            zip.by_name("assets/中文 image.png")
                .unwrap()
                .read_to_end(&mut image)
                .unwrap();
            assert_eq!(image, [0, 1, 255]);
            let create = requests[3].body.as_json().unwrap();
            assert_eq!(create["Env"], "Production");
            assert_eq!(create["DistType"], "Zip");
            assert_eq!(create["TempBucketPath"], "temp/中文 site/artifact.zip");
        }
    }
    #[test]
    fn works_uses_its_own_project_and_payload() {
        let client = FakeHttpClient::new()
            .always(
                200,
                api(json!({"Projects": [{"ProjectId": "proj-works", "Provider": "Upload"}]})),
            )
            .always(200, api(credentials()))
            .always(200, "")
            .always(200, api(json!({"DeploymentId": "works-deploy"})))
            .always(
                200,
                api(json!({"Deployment": {"DeploymentId": "works-deploy", "Status": "Success"}})),
            );
        adapter(TargetKind::EdgeOneMainland, &client)
            .push(&payload(SubTarget::Works))
            .unwrap();
        assert_eq!(
            client.recorded()[3].body.as_json().unwrap()["ProjectId"],
            "proj-works"
        );
    }
    #[test]
    fn configuration_rejects_pasted_urls_and_overlapping_projects_before_saving() {
        for kind in [
            TargetKind::EdgeOneMainland,
            TargetKind::EdgeOneInternational,
        ] {
            let mut config = TargetConfig::new("fixture", kind);
            for project in ["", "https://example.com", "../id", "中文"] {
                config.project = Some(project.into());
                assert!(config.validate().is_err());
            }
            config.project = Some("main-id".into());
            assert!(config.validate().is_ok());
            config.works_project = config.project.clone();
            assert!(config.validate().is_err());
            config.works_project = Some("works-id".into());
            assert!(config.validate().is_ok());
        }
    }
    #[test]
    fn overlapping_main_and_works_projects_are_rejected_before_network() {
        let client = FakeHttpClient::new();
        let mut adapter = adapter(TargetKind::EdgeOneMainland, &client);
        adapter.target.works_project = adapter.target.project.clone();
        assert!(adapter.push(&payload(SubTarget::Main)).is_err());
        assert!(adapter.push(&payload(SubTarget::Works)).is_err());
        assert_eq!(client.request_count(), 0);
    }
    #[test]
    fn deployment_polling_has_a_bound_and_never_reuploads() {
        let mut client = successful_client("Building");
        for _ in 1..180 {
            client = client.always(
                200,
                api(json!({"Deployments": [{"DeploymentId": "deploy-1", "Status": "Building"}]})),
            );
        }
        let error = adapter(TargetKind::EdgeOneMainland, &client)
            .push(&payload(SubTarget::Main))
            .unwrap_err()
            .to_string();
        assert!(error.contains("超时"));
        assert_eq!(client.request_count(), 184);
        assert_eq!(
            client
                .recorded()
                .iter()
                .filter(|r| r.method == veloct_core::http::HttpMethod::Put)
                .count(),
            1
        );
    }
    #[test]
    fn never_uses_git_project_or_missing_project() {
        for body in [project("GitHub"), api(json!({"Projects": []}))] {
            let client = FakeHttpClient::new().always(200, body);
            assert!(
                adapter(TargetKind::EdgeOneMainland, &client)
                    .push(&payload(SubTarget::Main))
                    .is_err()
            );
            assert_eq!(client.request_count(), 1);
        }
    }
    #[test]
    fn bad_http_outer_code_nested_error_or_missing_envelope_stops_without_retry_or_secret_leak() {
        for (http, body) in [
            (401, json!({"Message": "private-pages-token"}).to_string()),
            (429, "{}".into()),
            (500, "private-pages-token".into()),
            (302, "{}".into()),
            (
                200,
                json!({"Code": 110, "Message": "private-pages-token"}).to_string(),
            ),
            (
                200,
                api(json!({"Error": {"Code": "Denied", "Message": "private-pages-token"}})),
            ),
            (200, "{}".into()),
        ] {
            let client = FakeHttpClient::new().always(http, body);
            let error = adapter(TargetKind::EdgeOneMainland, &client)
                .push(&payload(SubTarget::Main))
                .unwrap_err()
                .to_string();
            assert!(!error.contains("private-pages-token"));
            assert_eq!(client.request_count(), 1);
        }
    }
    #[test]
    fn unsafe_or_missing_cos_credentials_never_upload() {
        for data in [
            json!({}),
            {
                let mut v = credentials();
                v["Bucket"] = json!("localhost:3080/private");
                v
            },
            {
                let mut v = credentials();
                v["TargetPath"] = json!("../../private");
                v
            },
        ] {
            let client = FakeHttpClient::new()
                .always(200, project("Upload"))
                .always(200, api(data));
            let error = adapter(TargetKind::EdgeOneMainland, &client)
                .push(&payload(SubTarget::Main))
                .unwrap_err()
                .to_string();
            assert_eq!(client.request_count(), 2);
            assert!(!error.contains("temporary-"));
        }
    }
    #[test]
    fn rejected_upload_never_creates_deployment() {
        let client = FakeHttpClient::new()
            .always(200, project("Upload"))
            .always(200, api(credentials()))
            .always(403, "temporary-key temporary-token");
        let error = adapter(TargetKind::EdgeOneMainland, &client)
            .push(&payload(SubTarget::Main))
            .unwrap_err()
            .to_string();
        assert!(!error.contains("temporary-"));
        assert_eq!(client.request_count(), 3);
    }
    #[test]
    fn accepted_creation_is_not_success_until_cloud_status_succeeds() {
        for status in ["Failed", "Timeout", "Cancelled", "Invalid"] {
            let client = successful_client(status);
            assert!(
                adapter(TargetKind::EdgeOneMainland, &client)
                    .push(&payload(SubTarget::Main))
                    .is_err()
            );
            assert_eq!(client.request_count(), 5);
        }
        let client = successful_client("Building").always(
            200,
            api(json!({"Deployments": [{"DeploymentId": "deploy-1", "Status": "Success"}]})),
        );
        assert!(
            adapter(TargetKind::EdgeOneMainland, &client)
                .push(&payload(SubTarget::Main))
                .is_ok()
        );
        assert_eq!(client.request_count(), 6);
    }
    #[test]
    fn unknown_or_missing_deployment_cannot_claim_success() {
        let client = FakeHttpClient::new()
            .always(200, project("Upload"))
            .always(200, api(credentials()))
            .always(200, "")
            .always(200, api(json!({})));
        assert!(
            adapter(TargetKind::EdgeOneMainland, &client)
                .push(&payload(SubTarget::Main))
                .is_err()
        );
        assert_eq!(client.request_count(), 4);
        let client = successful_client("Success");
        let mut adapter = adapter(TargetKind::EdgeOneMainland, &client);
        adapter.target.project = None;
        assert!(adapter.push(&payload(SubTarget::Main)).is_err());
        assert_eq!(client.request_count(), 0);
    }
    #[test]
    fn private_or_escaping_artifacts_are_rejected_before_network() {
        for path in [
            "../secret",
            "/root",
            "C:/key",
            "a\\key",
            ".env",
            ".env.production",
            "a/.git/config",
            ".veloct/keys",
            "node_modules/a",
            "private.key",
            "secrets.pem",
            ".npmrc",
            ".git-credentials",
            ".attachments-cache/image.png",
            ".cargo-home/config.toml",
        ] {
            let mut payload = payload(SubTarget::Main);
            payload.files[0].relative_path = path.into();
            let client = FakeHttpClient::new();
            assert!(
                adapter(TargetKind::EdgeOneMainland, &client)
                    .push(&payload)
                    .is_err(),
                "{path}"
            );
            assert_eq!(client.request_count(), 0);
        }
        let mut payload = payload(SubTarget::Main);
        payload.files.push(payload.files[0].clone());
        assert!(archive(&payload).is_err());
    }
    #[test]
    fn disk_collection_refuses_source_and_private_files_but_keeps_well_known() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".well-known")).unwrap();
        std::fs::write(dir.path().join(".well-known/security.txt"), "contact").unwrap();
        assert_eq!(
            PushPayload::read(SubTarget::Main, dir.path())
                .unwrap()
                .file_count(),
            1
        );
        std::fs::write(dir.path().join(".env"), "fixture-secret").unwrap();
        assert!(PushPayload::read(SubTarget::Main, dir.path()).is_err());
    }
}

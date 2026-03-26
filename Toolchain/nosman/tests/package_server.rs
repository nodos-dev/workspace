use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use nosman::nosman::common::download_and_extract;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct HttpResponse {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

struct TestServer {
    base_url: String,
}

impl TestServer {
    fn start(handler: Arc<dyn Fn(HttpRequest) -> HttpResponse + Send + Sync + 'static>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let handler = Arc::clone(&handler);
                thread::spawn(move || handle_connection(stream, handler));
            }
        });
        Self { base_url }
    }

    fn client(&self) -> nodos_store_client::StoreClient {
        nodos_store_client::StoreClient::builder()
            .with_base_url(&self.base_url)
            .build()
            .expect("client")
    }

    fn authenticated_client(&self) -> nodos_store_client::StoreClient {
        nodos_store_client::StoreClient::builder()
            .with_base_url(&self.base_url)
            .with_token("test-token")
            .build()
            .expect("client")
    }
}

fn handle_connection(mut stream: TcpStream, handler: Arc<dyn Fn(HttpRequest) -> HttpResponse + Send + Sync + 'static>) {
    let request = read_request(&mut stream);
    let response = handler(request);
    let status_text = match response.status {
        200 => "OK",
        404 => "Not Found",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
        response.status,
        status_text,
        response.body.len(),
        response.content_type
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&response.body);
}

fn read_request(stream: &mut TcpStream) -> HttpRequest {
    let mut buffer = Vec::new();
    let mut header_end = None;
    while header_end.is_none() {
        let mut chunk = [0_u8; 1024];
        let read = stream.read(&mut chunk).expect("read request");
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        header_end = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    }
    let header_end = header_end.expect("header end");
    let header_bytes = &buffer[..header_end];
    let header_text = String::from_utf8(header_bytes.to_vec()).expect("header text");
    let mut lines = header_text.lines();
    let request_line = lines.next().expect("request line");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let path = request_parts.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[(header_end + 4)..].to_vec();
    while body.len() < content_length {
        let mut chunk = [0_u8; 1024];
        let read = stream.read(&mut chunk).expect("read body");
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }

    HttpRequest {
        method,
        path,
        headers,
        body,
    }
}

fn zip_bytes() -> Vec<u8> {
    let cursor = Cursor::new(Vec::<u8>::new());
    let mut writer = zip::ZipWriter::new(cursor);
    writer
        .start_file("plugin/test.txt", SimpleFileOptions::default())
        .expect("start zip file");
    writer.write_all(b"hello from package").expect("write zip");
    writer.finish().expect("finish zip").into_inner()
}

#[test]
fn client_uses_default_base_url() {
    let client = nodos_store_client::StoreClient::builder().build().unwrap();
    // The client should build without requiring a URL
    drop(client);
}

#[test]
fn client_with_custom_base_url() {
    let server = TestServer::start(Arc::new(|request| match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/v1/packages?page=1&per_page=48") => HttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({"packages": [{"name": "test.pkg", "package_type": "Plugin"}], "has_next_page": false})
                .to_string().into_bytes(),
        },
        _ => HttpResponse { status: 404, content_type: "application/json", body: b"{\"message\":\"not found\"}".to_vec() },
    }));

    let client = server.client();
    let packages = client.list_packages().expect("list packages");
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].name, "test.pkg");
}

#[test]
fn fetch_maps_release_data() {
    let server = TestServer::start(Arc::new(|request| match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/v1/packages?page=1&per_page=48") => HttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({
                "packages": [{"name": "studio.plugin", "package_type": "Plugin"}],
                "has_next_page": false
            }).to_string().into_bytes(),
        },
        ("GET", "/api/v1/packages/studio.plugin/releases?page=1&per_page=48") => HttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({
                "releases": [{
                    "id": 12, "version": "1.2.3", "tags": ["stable"],
                    "artifacts": [{"id": 44, "target_platform": "x86_64-windows"}],
                    "api_version": {"major": 1, "minor": 4, "patch": 0},
                    "dependencies": [{"name": "studio.shared", "version": "2.0.0"}],
                    "updated_at": "2026-03-10T10:00:00Z"
                }],
                "has_next_page": false
            }).to_string().into_bytes(),
        },
        _ => HttpResponse { status: 404, content_type: "application/json", body: b"{\"message\":\"not found\"}".to_vec() },
    }));

    let client = server.client();
    let packages = client.list_packages().expect("fetch packages");
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].name, "studio.plugin");
    assert_eq!(packages[0].package_type, "Plugin");

    let releases = client.get_releases("studio.plugin").expect("fetch releases");
    assert_eq!(releases.len(), 1);
    let release = &releases[0];
    assert_eq!(release.id, 12);
    assert_eq!(release.version, "1.2.3");
    assert_eq!(release.tags, vec!["stable"]);
    assert_eq!(release.artifacts.len(), 1);
    assert_eq!(release.artifacts[0].id, 44);
    assert_eq!(release.artifacts[0].target_platform, "x86_64-windows");
    let api_ver = release.api_version.as_ref().unwrap();
    assert_eq!(api_ver.major, 1);
    assert_eq!(api_ver.minor, Some(4));
    assert_eq!(api_ver.patch, Some(0));
    assert_eq!(release.dependencies.len(), 1);
    assert_eq!(release.dependencies[0].name, "studio.shared");
    assert_eq!(release.dependencies[0].version, "2.0.0");
}

#[test]
fn download_and_extract_resolves_package_server_artifact_url() {
    let zip = zip_bytes();
    let base_url_ref = Arc::new(Mutex::new(String::new()));
    let base_url_for_handler = Arc::clone(&base_url_ref);
    let server = TestServer::start(Arc::new(move |request| match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/api/v1/release-artifacts/7/download-token") => HttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({
                "download_url": format!("{}/downloads/7.zip", base_url_for_handler.lock().unwrap().as_str())
            })
            .to_string()
            .into_bytes(),
        },
        ("GET", "/downloads/7.zip") => HttpResponse {
            status: 200,
            content_type: "application/zip",
            body: zip.clone(),
        },
        _ => HttpResponse {
            status: 404,
            content_type: "application/json",
            body: b"{\"message\":\"not found\"}".to_vec(),
        },
    }));
    *base_url_ref.lock().unwrap() = server.base_url.clone();
    let client = server.client();
    let output_dir = tempdir().expect("output dir");
    download_and_extract(
        &format!("{}/api/v1/release-artifacts/7", server.base_url),
        &PathBuf::from(output_dir.path()),
        &client,
    )
    .expect("download and extract");
    assert_eq!(
        std::fs::read_to_string(output_dir.path().join("plugin").join("test.txt")).unwrap(),
        "hello from package"
    );
}

#[test]
fn publish_release_uses_package_server_api() {
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let uploads = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let requests_ref = Arc::clone(&requests);
    let uploads_ref = Arc::clone(&uploads);
    let base_url_ref = Arc::new(Mutex::new(String::new()));
    let base_url_for_handler = Arc::clone(&base_url_ref);
    let server = TestServer::start(Arc::new(move |request| {
        requests_ref
            .lock()
            .unwrap()
            .push(format!("{} {}", request.method, request.path));
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/account/me") => {
                assert_eq!(
                    request.headers.get("authorization").map(String::as_str),
                    Some("Bearer test-token")
                );
                HttpResponse {
                    status: 200,
                    content_type: "application/json",
                    body: serde_json::json!({"needs_onboarding": false}).to_string().into_bytes(),
                }
            }
            ("POST", "/api/v1/publish/sessions") => {
                assert_eq!(
                    request.headers.get("authorization").map(String::as_str),
                    Some("Bearer test-token")
                );
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["name"], "studio.plugin");
                assert_eq!(body["version"], "1.2.3");
                HttpResponse {
                    status: 200,
                    content_type: "application/json",
                    body: serde_json::json!({"session": {"id": 11}}).to_string().into_bytes(),
                }
            }
            ("POST", "/api/v1/publish/sessions/11/artifact-upload-url") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({
                    "upload_url": format!("{}/upload/11", base_url_for_handler.lock().unwrap().as_str())
                }).to_string().into_bytes(),
            },
            ("PUT", "/upload/11") => {
                uploads_ref.lock().unwrap().push(request.body);
                HttpResponse {
                    status: 200,
                    content_type: "application/json",
                    body: b"{\"message\":\"Artifact uploaded.\"}".to_vec(),
                }
            }
            ("POST", "/api/v1/publish/sessions/11/finalize") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({
                    "release": {
                        "id": 5, "version": "1.2.3", "tags": ["stable"],
                        "artifacts": [{"id": 77, "target_platform": "x86_64-windows"}],
                        "dependencies": [{"name": "studio.dep", "version": "2.0.0"}],
                        "updated_at": "2026-03-10T10:00:00Z"
                    }
                }).to_string().into_bytes(),
            },
            _ => HttpResponse { status: 404, content_type: "application/json", body: b"{\"message\":\"not found\"}".to_vec() },
        }
    }));
    *base_url_ref.lock().unwrap() = server.base_url.clone();

    let artifact_dir = tempdir().expect("artifact dir");
    let artifact_path = artifact_dir.path().join("studio.plugin-1.2.3-x86_64-windows.zip");
    std::fs::write(&artifact_path, b"artifact-body").unwrap();

    let mut client = server.authenticated_client();
    let release = client.publish_release(
        "studio.plugin",
        "Studio Plugin",
        "Package server publish test",
        "Plugin",
        "Compositing",
        "1.2.3",
        Some(&nodos_store_client::ApiVersion { major: 1, minor: Some(4), patch: Some(0) }),
        vec![nodos_store_client::PackageDependency { name: "studio.dep".to_string(), version: "2.0.0".to_string() }],
        vec!["stable".to_string()],
        "x86_64-windows",
        b"artifact-body".to_vec(),
    ).expect("publish release");

    assert_eq!(release.version, "1.2.3");
    assert_eq!(uploads.lock().unwrap()[0], b"artifact-body");
    assert!(requests.lock().unwrap().iter().any(|entry| entry == "POST /api/v1/publish/sessions"));
    assert!(requests.lock().unwrap().iter().any(|entry| entry == "POST /api/v1/publish/sessions/11/finalize"));
}

#[test]
fn delete_release_deletes_by_version_not_by_id() {
    let deleted_paths = Arc::new(Mutex::new(Vec::<String>::new()));
    let deleted_ref = Arc::clone(&deleted_paths);
    let server = TestServer::start(Arc::new(move |request| {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/account/me") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({"needs_onboarding": false}).to_string().into_bytes(),
            },
            ("GET", path) if path.starts_with("/api/v1/account/packages/studio.plugin/releases") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({
                    "releases": [
                        {"id": 101, "version": "1.0.0", "tags": [], "artifacts": [], "dependencies": [], "updated_at": "2026-03-10T00:00:00Z"},
                        {"id": 102, "version": "2.0.0", "tags": [], "artifacts": [], "dependencies": [], "updated_at": "2026-03-11T00:00:00Z"}
                    ],
                    "has_next_page": false
                }).to_string().into_bytes(),
            },
            ("DELETE", path) if path.starts_with("/api/v1/account/packages/") => {
                deleted_ref.lock().unwrap().push(path.to_string());
                HttpResponse { status: 200, content_type: "application/json", body: b"{}".to_vec() }
            },
            _ => HttpResponse { status: 404, content_type: "application/json", body: b"{\"message\":\"not found\"}".to_vec() },
        }
    }));

    let mut client = server.authenticated_client();

    // Delete only version 2.0.0 — should hit release id 102
    client.delete_release("studio.plugin", Some("2.0.0"))
        .expect("delete release");

    let deleted = deleted_paths.lock().unwrap();
    assert_eq!(deleted.len(), 1, "expected exactly one DELETE request");
    assert_eq!(deleted[0], "/api/v1/account/packages/studio.plugin/releases/102");
}

#[test]
fn delete_release_all_versions() {
    let deleted_paths = Arc::new(Mutex::new(Vec::<String>::new()));
    let deleted_ref = Arc::clone(&deleted_paths);
    let server = TestServer::start(Arc::new(move |request| {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/account/me") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({"needs_onboarding": false}).to_string().into_bytes(),
            },
            ("GET", path) if path.starts_with("/api/v1/account/packages/studio.plugin/releases") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({
                    "releases": [
                        {"id": 101, "version": "1.0.0", "tags": [], "artifacts": [], "dependencies": [], "updated_at": "2026-03-10T00:00:00Z"},
                        {"id": 102, "version": "2.0.0", "tags": [], "artifacts": [], "dependencies": [], "updated_at": "2026-03-11T00:00:00Z"}
                    ],
                    "has_next_page": false
                }).to_string().into_bytes(),
            },
            ("DELETE", path) if path.starts_with("/api/v1/account/packages/") => {
                deleted_ref.lock().unwrap().push(path.to_string());
                HttpResponse { status: 200, content_type: "application/json", body: b"{}".to_vec() }
            },
            _ => HttpResponse { status: 404, content_type: "application/json", body: b"{\"message\":\"not found\"}".to_vec() },
        }
    }));

    let mut client = server.authenticated_client();
    client.delete_release("studio.plugin", None).expect("delete all");

    let deleted = deleted_paths.lock().unwrap();
    assert_eq!(deleted.len(), 2, "expected two DELETE requests");
    assert!(deleted.contains(&"/api/v1/account/packages/studio.plugin/releases/101".to_string()));
    assert!(deleted.contains(&"/api/v1/account/packages/studio.plugin/releases/102".to_string()));
}

#[test]
fn delete_release_returns_error_when_version_not_found() {
    let server = TestServer::start(Arc::new(move |request| {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/account/me") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({"needs_onboarding": false}).to_string().into_bytes(),
            },
            ("GET", path) if path.starts_with("/api/v1/account/packages/studio.plugin/releases") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({"releases": [], "has_next_page": false}).to_string().into_bytes(),
            },
            _ => HttpResponse { status: 404, content_type: "application/json", body: b"{\"message\":\"not found\"}".to_vec() },
        }
    }));

    let mut client = server.authenticated_client();
    let result = client.delete_release("studio.plugin", Some("9.9.9"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No release found"));
}

#[test]
fn publish_release_reports_upload_failure() {
    let base_url_ref = Arc::new(Mutex::new(String::new()));
    let base_url_for_handler = Arc::clone(&base_url_ref);
    let server = TestServer::start(Arc::new(move |request| {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/v1/account/me") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({"needs_onboarding": false}).to_string().into_bytes(),
            },
            ("POST", "/api/v1/publish/sessions") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({"session": {"id": 20}}).to_string().into_bytes(),
            },
            ("POST", "/api/v1/publish/sessions/20/artifact-upload-url") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({
                    "upload_url": format!("{}/upload/20", base_url_for_handler.lock().unwrap().as_str())
                }).to_string().into_bytes(),
            },
            ("PUT", "/upload/20") => HttpResponse {
                status: 500,
                content_type: "application/json",
                body: serde_json::json!({"message": "storage backend unavailable"}).to_string().into_bytes(),
            },
            _ => HttpResponse { status: 404, content_type: "application/json", body: b"{\"message\":\"not found\"}".to_vec() },
        }
    }));
    *base_url_ref.lock().unwrap() = server.base_url.clone();

    let mut client = server.authenticated_client();
    let result = client.publish_release(
        "fail.plugin", "Fail", "test", "Plugin", "General", "0.1.0",
        None, vec![], vec![], "x86_64-windows", b"data".to_vec(),
    );

    assert!(result.is_err(), "upload failure should propagate as error");
    assert!(result.unwrap_err().to_string().contains("storage backend unavailable"));
}

#[test]
fn no_auth_header_when_no_token() {
    let seen_auth = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let seen_ref = Arc::clone(&seen_auth);
    let server = TestServer::start(Arc::new(move |request| {
        seen_ref.lock().unwrap().push(request.headers.get("authorization").cloned());
        HttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({"packages": [], "has_next_page": false}).to_string().into_bytes(),
        }
    }));

    let client = server.client(); // no token
    let _ = client.list_packages();

    let auths = seen_auth.lock().unwrap();
    assert_eq!(auths.len(), 1);
    assert!(auths[0].is_none(), "no Authorization header should be sent without a token");
}

#[test]
fn auth_header_sent_with_token() {
    let seen_auth = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let seen_ref = Arc::clone(&seen_auth);
    let server = TestServer::start(Arc::new(move |request| {
        seen_ref.lock().unwrap().push(request.headers.get("authorization").cloned());
        HttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({"packages": [], "has_next_page": false}).to_string().into_bytes(),
        }
    }));

    let client = server.authenticated_client();
    let _ = client.list_packages();

    let auths = seen_auth.lock().unwrap();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].as_deref(), Some("Bearer test-token"));
}

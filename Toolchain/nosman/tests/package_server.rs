use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use nosman::nosman::common::download_and_extract;
use nosman::nosman::index::{PackageType, SemVer};
use nosman::nosman::package::PackageIdentifier;
use nosman::nosman::package_server::{self, PublishReleaseRequest};
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
fn package_server_fetch_maps_release_data() {
    let server = TestServer::start(Arc::new(|request| match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/v1/packages?page=1&per_page=100") => HttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({
                "packages": [
                    {
                        "name": "studio.plugin",
                        "package_type": "Plugin"
                    }
                ],
                "has_next_page": false
            })
            .to_string()
            .into_bytes(),
        },
        ("GET", "/api/v1/packages/studio.plugin/releases?page=1&per_page=100") => HttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({
                "releases": [
                    {
                        "id": 12,
                        "version": "1.2.3",
                        "tags": ["stable"],
                        "artifacts": [
                            {
                                "id": 44,
                                "target_platform": "x86_64-windows"
                            }
                        ],
                        "api_version": {
                            "major": 1,
                            "minor": 4,
                            "patch": 0
                        },
                        "dependencies": [
                            {
                                "name": "studio.shared",
                                "version": "2.0.0"
                            }
                        ],
                        "updated_at": "2026-03-10T10:00:00Z"
                    }
                ],
                "has_next_page": false
            })
            .to_string()
            .into_bytes(),
        },
        _ => HttpResponse {
            status: 404,
            content_type: "application/json",
            body: b"{\"message\":\"not found\"}".to_vec(),
        },
    }));
    let packages = package_server::fetch_packages(&server.base_url).expect("fetch packages");
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].name, "studio.plugin");
    assert_eq!(packages[0].package_type, "Plugin");

    let releases = package_server::to_package_releases(
        &server.base_url,
        &PackageType::Plugin,
        package_server::fetch_package_releases(&server.base_url, "studio.plugin")
            .expect("fetch releases"),
    );
    assert_eq!(releases.len(), 1);
    let release_json = serde_json::to_value(&releases[0]).unwrap();
    assert_eq!(release_json["version"], "1.2.3");
    assert_eq!(release_json["platform"], "x86_64-windows");
    assert_eq!(release_json["dependencies"][0]["name"], "studio.shared");
    assert_eq!(release_json["plugin_api_version"]["major"], 1);
    assert_eq!(release_json["plugin_api_version"]["minor"], 4);
    assert_eq!(release_json["plugin_api_version"]["patch"], 0);
    assert_eq!(release_json["url"], format!("{}/api/v1/release-artifacts/44", server.base_url));
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
    let output_dir = tempdir().expect("output dir");
    download_and_extract(
        &format!("{}/api/v1/release-artifacts/7", server.base_url),
        &PathBuf::from(output_dir.path()),
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
                    body: serde_json::json!({
                        "needs_onboarding": false
                    })
                    .to_string()
                    .into_bytes(),
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
                    body: serde_json::json!({
                        "session": {
                            "id": 11
                        }
                    })
                    .to_string()
                    .into_bytes(),
                }
            }
            ("POST", "/api/v1/publish/sessions/11/artifact-upload-url") => HttpResponse {
                status: 200,
                content_type: "application/json",
                body: serde_json::json!({
                    "upload_url": format!("{}/upload/11", base_url_for_handler.lock().unwrap().as_str())
                })
                .to_string()
                .into_bytes(),
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
                        "id": 5,
                        "version": "1.2.3",
                        "tags": ["stable"],
                        "artifacts": [
                            {
                                "id": 77,
                                "target_platform": "x86_64-windows"
                            }
                        ],
                        "dependencies": [
                            {
                                "name": "studio.dep",
                                "version": "2.0.0"
                            }
                        ],
                        "updated_at": "2026-03-10T10:00:00Z"
                    }
                })
                .to_string()
                .into_bytes(),
            },
            _ => HttpResponse {
                status: 404,
                content_type: "application/json",
                body: b"{\"message\":\"not found\"}".to_vec(),
            },
        }
    }));
    *base_url_ref.lock().unwrap() = server.base_url.clone();
    std::env::set_var("NOSMAN_PACKAGE_SERVER_TOKEN", "test-token");
    let artifact_dir = tempdir().expect("artifact dir");
    let artifact_path = artifact_dir.path().join("studio.plugin-1.2.3-x86_64-windows.zip");
    std::fs::write(&artifact_path, b"artifact-body").unwrap();

    let release = package_server::publish_release(
        &server.base_url,
        &PublishReleaseRequest {
            name: "studio.plugin".to_string(),
            display_name: "Studio Plugin".to_string(),
            description: "Package server publish test".to_string(),
            package_type: PackageType::Plugin,
            category: "Compositing".to_string(),
            version: "1.2.3".to_string(),
            api_version: Some(SemVer::new(1, Some(4), Some(0), None)),
            dependencies: vec![PackageIdentifier {
                name: "studio.dep".to_string(),
                version: "2.0.0".to_string(),
            }],
            tags: vec!["stable".to_string()],
            target_platform: "x86_64-windows".to_string(),
            artifact_path,
        },
        false,
        false,
    )
    .expect("publish release");

    std::env::remove_var("NOSMAN_PACKAGE_SERVER_TOKEN");

    assert_eq!(release.version, "1.2.3");
    assert_eq!(uploads.lock().unwrap()[0], b"artifact-body");
    assert!(requests.lock().unwrap().iter().any(|entry| entry == "POST /api/v1/publish/sessions"));
    assert!(requests.lock().unwrap().iter().any(|entry| entry == "POST /api/v1/publish/sessions/11/finalize"));
}

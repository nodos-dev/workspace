use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use nosman::nosman::common::download_and_extract;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

#[allow(dead_code)]
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

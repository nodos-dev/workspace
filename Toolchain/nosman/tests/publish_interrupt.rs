use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use nodos_store_client::StoreClient;
use nosman::nosman::command::publish_interrupt::{
    clear_draft, publish_in_progress, watch, PublishInProgress,
};

/// Nothing is listening here, so the store can never answer.
const UNREACHABLE_STORE: &str = "http://127.0.0.1:1";

fn unreachable_client() -> StoreClient {
    StoreClient::builder()
        .with_base_url(UNREACHABLE_STORE)
        .build()
        .expect("failed to build a store client")
}

/// A store that accepts deletion of the exact draft being published.
fn client_for_store_that_discards_the_draft() -> StoreClient {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            let body = b"{\"message\":\"Publish session discarded\"}";
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    StoreClient::builder()
        .with_base_url(&base_url)
        .build()
        .expect("failed to build a store client")
}

#[test]
fn watching_names_the_release_only_while_it_is_being_published() {
    assert!(
        publish_in_progress().is_none(),
        "nothing is being published yet"
    );

    {
        let _watching = watch(42, "test.plugin", "1.2.0", "x86_64-windows");
        assert_eq!(
            publish_in_progress(),
            Some(PublishInProgress {
                session_id: 42,
                name: "test.plugin".to_string(),
                version: "1.2.0".to_string(),
                target_platform: "x86_64-windows".to_string(),
            }),
            "an interrupt has to know which release to clear"
        );
    }

    assert!(
        publish_in_progress().is_none(),
        "a publish that has ended must not be cleared up by a later interrupt"
    );
}

#[test]
fn a_store_that_cannot_be_reached_says_the_next_publish_will_clear_it() {
    let line = clear_draft(
        &unreachable_client(),
        &PublishInProgress {
            session_id: 42,
            name: "test.plugin".to_string(),
            version: "1.2.0".to_string(),
            target_platform: "x86_64-windows".to_string(),
        },
    );

    assert!(
        line.contains("test.plugin==1.2.0 for x86_64-windows"),
        "the line should name the release: {}",
        line
    );
    assert!(
        line.contains("the next publish of it will clear it"),
        "the line should say what happens to the upload that is still there: {}",
        line
    );
    assert!(
        !line.contains('\n'),
        "a connection failure chains several \"caused by\" lines onto its \
         message, and that must not break this into several lines: {}",
        line
    );
}

#[test]
fn a_store_that_discards_the_exact_draft_says_the_upload_was_removed() {
    let line = clear_draft(
        &client_for_store_that_discards_the_draft(),
        &PublishInProgress {
            session_id: 42,
            name: "test.plugin".to_string(),
            version: "1.2.0".to_string(),
            target_platform: "x86_64-windows".to_string(),
        },
    );

    assert_eq!(
        line,
        "publish of test.plugin==1.2.0 for x86_64-windows and removed its \
         unfinished upload from the store",
        "this is the line most publishers actually see, so its exact wording \
         has to be pinned down"
    );
}

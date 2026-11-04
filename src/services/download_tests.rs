//! Download protocol regressions use bounded local servers only.

use super::download::*;
use crate::domain::{
    release::{ArchiveFormat, Artifact, Integrity},
    settings::NetworkSettings,
};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, Instant},
};

#[test]
fn bounded_listener_stops_without_a_client() {
    let (listener, _) = crate::services::download::test_server::TestListener::bind().unwrap();
    let (finished, done) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let error = listener.accept().unwrap_err();
        finished.send(error.kind()).unwrap();
    });
    assert_eq!(
        done.recv_timeout(Duration::from_secs(2)).unwrap(),
        std::io::ErrorKind::TimedOut
    );
    server.join().unwrap();
}

#[test]
fn bounded_connection_stops_after_the_client_disconnects() {
    let (listener, address) = crate::services::download::test_server::TestListener::bind().unwrap();
    let started = Instant::now();
    let server = thread::spawn(move || {
        let mut socket = listener.accept().unwrap();
        let _ = crate::services::download::test_server::read_request(&mut socket);
    });
    std::net::TcpStream::connect(address).unwrap();
    server.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
}

fn artifact(url: String, bytes: &[u8]) -> Artifact {
    Artifact {
        file_name: "test.zip".into(),
        format: ArchiveFormat::Zip,
        size: Some(bytes.len() as u64),
        integrity: Some(Integrity::Sha256(hex::encode(Sha256::digest(bytes)))),
        urls: vec![url],
        signature_url: None,
    }
}

#[tokio::test]
async fn rejects_unsolicited_partial_response_instead_of_publishing_it() {
    let (listener, address) = crate::services::download::test_server::TestListener::bind().unwrap();
    let url = format!("http://{address}/artifact");
    thread::spawn(move || {
        let mut socket = listener.accept().unwrap();
        let _ = crate::services::download::test_server::read_request(&mut socket);
        crate::services::download::test_server::write_all(
            &mut socket,
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 5-7/8\r\nConnection: close\r\n\r\nabc",
        )
        .unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(
        reqwest::Client::new(),
        dir.path().into(),
        Arc::new(AtomicBool::new(false)),
    );
    let result = downloader
        .download(&artifact(url, b"abc"), &NetworkSettings::default(), |_| {})
        .await;
    assert!(
        result.is_err(),
        "an unsolicited suffix must never pass as the whole artifact"
    );
}

#[tokio::test]
async fn missing_empty_or_malformed_digest_never_reaches_network_or_cache() {
    for integrity in [
        None,
        Some(Integrity::Sha256(String::new())),
        Some(Integrity::Sha256("g".repeat(64))),
        Some(Integrity::Sha512("0".repeat(64))),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            dir.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let mut artifact = artifact("https://invalid.invalid/not-requested".into(), b"bytes");
        artifact.integrity = integrity;
        let result = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await;
        assert!(matches!(
            result,
            Err(crate::services::ServiceError::InvalidIntegrity)
        ));
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }
}

#[tokio::test]
async fn full_response_with_content_range_is_rejected() {
    let (listener, address) = crate::services::download::test_server::TestListener::bind().unwrap();
    let url = format!("http://{address}/artifact");
    let server = thread::spawn(move || {
        let mut socket = listener.accept().unwrap();
        let _ = crate::services::download::test_server::read_request(&mut socket);
        crate::services::download::test_server::write_all(
            &mut socket,
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Range: bytes 0-2/3\r\nConnection: close\r\n\r\nabc",
        )
        .unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(
        reqwest::Client::new(),
        dir.path().into(),
        Arc::new(AtomicBool::new(false)),
    );
    let result = downloader
        .download(&artifact(url, b"abc"), &NetworkSettings::default(), |_| {})
        .await;
    assert!(matches!(
        result,
        Err(crate::services::ServiceError::InvalidRange)
    ));
    server.join().unwrap();
}

#[tokio::test]
async fn rejects_a_body_larger_than_trusted_size_before_writing_the_excess() {
    let (listener, address) = crate::services::download::test_server::TestListener::bind().unwrap();
    let url = format!("http://{address}/artifact");
    thread::spawn(move || {
        let mut socket = listener.accept().unwrap();
        let _ = crate::services::download::test_server::read_request(&mut socket);
        crate::services::download::test_server::write_all(
            &mut socket,
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n10\r\n0123456789abcdef\r\n0\r\n\r\n",
        )
        .unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(
        reqwest::Client::new(),
        dir.path().into(),
        Arc::new(AtomicBool::new(false)),
    );
    let mut maximum_written = 0;
    let result = downloader
        .download(&artifact(url, b"abc"), &NetworkSettings::default(), |p| {
            maximum_written = maximum_written.max(p.downloaded)
        })
        .await;
    assert!(result.is_err());
    assert!(
        maximum_written <= 3,
        "oversized response was written to the cache: {maximum_written}"
    );
}

#[tokio::test]
async fn encoded_artifact_responses_are_rejected_before_writing_payload() {
    for encoding in ["gzip", "br", "zstd", "deflate", "gzip, br"] {
        let bytes = b"abc";
        let (listener, address) =
            crate::services::download::test_server::TestListener::bind().unwrap();
        let url = format!("http://{address}/artifact");
        let server = thread::spawn(move || {
            let mut socket = listener.accept().unwrap();
            let _ = crate::services::download::test_server::read_request(&mut socket);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Encoding: {encoding}\r\nConnection: close\r\n\r\nabc"
            );
            crate::services::download::test_server::write_all(&mut socket, response.as_bytes())
                .unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let client =
            crate::services::http::HttpClient::download_client(&NetworkSettings::default())
                .unwrap();
        let downloader =
            Downloader::new(client, dir.path().into(), Arc::new(AtomicBool::new(false)));
        let result = downloader
            .download(&artifact(url, bytes), &NetworkSettings::default(), |_| {})
            .await;
        assert!(
            matches!(result, Err(crate::services::ServiceError::InvalidRange)),
            "{encoding}: {result:?}"
        );
        assert!(
            !std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .any(
                    |entry| entry.path().extension().and_then(|value| value.to_str())
                        == Some("part")
                ),
            "{encoding} left a retained payload"
        );
        server.join().unwrap();
    }
}

#[tokio::test]
async fn cancellation_preserves_nonempty_partial_for_a_new_downloader() {
    let bytes = vec![b'x'; 131072];
    let server_bytes = bytes.clone();
    let (listener, address) = crate::services::download::test_server::TestListener::bind().unwrap();
    let url = format!("http://{address}/artifact");
    let server = thread::spawn(move || {
        let mut first = listener.accept().unwrap();
        let _ = crate::services::download::test_server::read_request(&mut first);
        let mut response =
            b"HTTP/1.1 200 OK\r\nContent-Length: 131072\r\nConnection: close\r\n\r\n".to_vec();
        response.extend(std::iter::repeat_n(b'x', 65536));
        crate::services::download::test_server::write_all(&mut first, &response).unwrap();
        let mut second = listener.accept().unwrap();
        let request = crate::services::download::test_server::read_request(&mut second).unwrap();
        let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
        let offset = text
            .split("range: bytes=")
            .nth(1)
            .and_then(|s| s.split('-').next())
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert!(offset > 0 && offset < server_bytes.len());
        let remaining = server_bytes.len() - offset;
        let mut response = format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {remaining}\r\nContent-Range: bytes {offset}-{}/{}\r\nConnection: close\r\n\r\n", server_bytes.len() - 1, server_bytes.len()).into_bytes();
        response.extend_from_slice(&server_bytes[offset..]);
        crate::services::download::test_server::write_all(&mut second, &response).unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let first = Downloader::new(reqwest::Client::new(), dir.path().into(), cancel.clone());
    let release = artifact(url, &bytes);
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        first.download(&release, &NetworkSettings::default(), |p| {
            if p.downloaded >= 8192 {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }),
    )
    .await
    .unwrap();
    assert!(matches!(
        result,
        Err(crate::services::ServiceError::Cancelled)
    ));
    let second = Downloader::new(
        reqwest::Client::new(),
        dir.path().into(),
        Arc::new(AtomicBool::new(false)),
    );
    let path = second
        .download(&release, &NetworkSettings::default(), |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    server.join().unwrap();
}

//! Transport observations affect mirror scheduling, never release identity or trust.

use crate::services::{
    ServiceError,
    http::{url_uses_accelerator, wait_for_cancellation},
};
use futures_util::{StreamExt, stream};
use reqwest::{
    Client, Url,
    header::{ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, RANGE},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, atomic::AtomicBool},
    time::{Duration, Instant},
};

const PROBE_BUDGET: Duration = Duration::from_secs(3);
const HISTORY_TTL: Duration = Duration::from_secs(10 * 60);
type History = HashMap<String, (Instant, bool)>;
static HISTORY: OnceLock<Mutex<History>> = OnceLock::new();

pub(super) fn record(url: &Url, success: bool) {
    if let Ok(mut history) = HISTORY.get_or_init(Mutex::default).lock() {
        history.retain(|_, (at, _)| at.elapsed() < HISTORY_TTL);
        if history.len() >= 64 {
            history.clear();
        }
        history.insert(
            url.origin().ascii_serialization(),
            (Instant::now(), success),
        );
    }
}

pub(super) async fn order_candidates(
    client: &Client,
    candidates: Vec<Url>,
    direct_hosts: &HashSet<String>,
    expected_size: Option<u64>,
    cancellation: &AtomicBool,
) -> Result<Vec<Url>, ServiceError> {
    let mut accelerated: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter(|(_, url)| url_uses_accelerator(url, direct_hosts))
        .map(|(index, url)| (index, url.clone()))
        .collect();
    if accelerated.is_empty() {
        return Ok(candidates);
    }
    if let Ok(history) = HISTORY.get_or_init(Mutex::default).lock() {
        accelerated.sort_by_key(|(_, url)| {
            match history.get(&url.origin().ascii_serialization()) {
                Some((at, true)) if at.elapsed() < HISTORY_TTL => 0,
                Some((at, false)) if at.elapsed() < HISTORY_TTL => 2,
                _ => 1,
            }
        });
    }
    let mut failed = HashSet::new();
    let mut winner = None;
    {
        let mut probes = stream::iter(accelerated.iter().map(|(index, url)| async move {
            let result = probe(client, url, expected_size).await;
            (*index, result)
        }))
        .buffer_unordered(3);
        let deadline = tokio::time::sleep(PROBE_BUDGET);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                biased;
                _ = wait_for_cancellation(cancellation) => return Err(ServiceError::Cancelled),
                _ = &mut deadline => break,
                next = probes.next() => match next {
                    Some((index, Ok(true))) => {winner = Some(index); break;}
                    Some((index, Ok(false) | Err(_))) => {failed.insert(index);}
                    None => break,
                }
            }
        }
    }
    // Cancel losing probes immediately: one dead mirror must not delay an already usable mirror.
    accelerated.sort_by_key(|(index, _)| (Some(*index) != winner, failed.contains(index)));
    let mut ordered: Vec<_> = accelerated.into_iter().map(|(_, url)| url).collect();
    ordered.extend(
        candidates
            .into_iter()
            .filter(|url| !url_uses_accelerator(url, direct_hosts)),
    );
    Ok(ordered)
}

async fn probe(
    client: &Client,
    url: &Url,
    expected_size: Option<u64>,
) -> Result<bool, reqwest::Error> {
    let mut response = client
        .get(url.clone())
        .header(RANGE, "bytes=0-0")
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .await?;
    if !super::has_identity_content_encoding(&response) {
        return Ok(false);
    }
    let partial = match response.status().as_u16() {
        206 => {
            if !valid_probe_range(&response, expected_size) {
                return Ok(false);
            }
            true
        }
        200 => {
            if response.headers().contains_key(CONTENT_RANGE)
                || expected_size.is_some_and(|expected| {
                    response
                        .headers()
                        .get(CONTENT_LENGTH)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok())
                        .is_some_and(|length| length != expected)
                })
            {
                return Ok(false);
            }
            false
        }
        _ => return Ok(false),
    };
    Ok(response.chunk().await?.is_some_and(|bytes| {
        if partial {
            bytes.len() == 1
        } else {
            !bytes.is_empty()
        }
    }))
}

fn valid_probe_range(response: &reqwest::Response, expected_size: Option<u64>) -> bool {
    let mut values = response.headers().get_all(CONTENT_RANGE).iter();
    let Some(value) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let Some((start, end, total)) = value.to_str().ok().and_then(super::parse_content_range) else {
        return false;
    };
    start == 0
        && end == 0
        && expected_size.is_none_or(|expected| expected == total)
        && response.content_length().is_none_or(|length| length == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::NetworkSettings;
    use std::{sync::Arc, thread};

    #[tokio::test]
    async fn usable_mirror_does_not_wait_for_a_stalled_probe_and_keeps_official_last() {
        let (slow, slow_address) = super::super::test_server::TestListener::bind().unwrap();
        let (fast, fast_address) = super::super::test_server::TestListener::bind().unwrap();
        let slow_url = Url::parse(&format!("http://{slow_address}/slow")).unwrap();
        let fast_url = Url::parse(&format!("http://{fast_address}/fast")).unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let mut socket = fast.accept().unwrap();
            let _ = super::super::test_server::read_request(&mut socket);
            super::super::test_server::write_all(&mut socket, b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\nContent-Range: bytes 0-0/9\r\nConnection: close\r\n\r\nx").unwrap();
            let _ = released.recv_timeout(Duration::from_secs(3));
            drop(slow);
        });
        let official = Url::parse("https://github.com/owner/repo/releases/file").unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            order_candidates(
                &Client::new(),
                vec![slow_url.clone(), fast_url.clone(), official.clone()],
                &HashSet::from(["127.0.0.1".into()]),
                Some(9),
                &AtomicBool::new(false),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result, vec![fast_url, slow_url, official]);
        release.send(()).unwrap();
        server.join().unwrap();
    }

    #[tokio::test]
    async fn probe_cancellation_interrupts_stalled_body() {
        let (listener, address) = super::super::test_server::TestListener::bind().unwrap();
        let url = Url::parse(&format!("http://{address}/slow")).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let signal = cancellation.clone();
        let (release, released) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let mut socket = listener.accept().unwrap();
            let _ = super::super::test_server::read_request(&mut socket);
            super::super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n",
            )
            .unwrap();
            signal.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = released.recv_timeout(Duration::from_secs(3));
        });
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            order_candidates(
                &Client::new(),
                vec![url],
                &HashSet::from(["127.0.0.1".into()]),
                None,
                &cancellation,
            ),
        )
        .await
        .unwrap();
        assert!(matches!(result, Err(ServiceError::Cancelled)));
        release.send(()).unwrap();
        server.join().unwrap();
    }

    #[tokio::test]
    async fn invalid_probe_responses_never_become_the_winner() {
        fn serve(response: &'static [u8]) -> (Url, thread::JoinHandle<()>) {
            let (listener, address) = super::super::test_server::TestListener::bind().unwrap();
            let url = Url::parse(&format!("http://{address}/probe")).unwrap();
            let server = thread::spawn(move || {
                let mut socket = listener.accept().unwrap();
                let _ = super::super::test_server::read_request(&mut socket).unwrap();
                super::super::test_server::write_all(&mut socket, response).unwrap();
            });
            (url, server)
        }

        let (wrong_range, wrong_range_server) = serve(
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\nContent-Range: bytes 1-1/3\r\nConnection: close\r\n\r\nx",
        );
        let (encoded, encoded_server) = serve(
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\nContent-Range: bytes 0-0/3\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\nx",
        );
        let (wrong_length, wrong_length_server) = serve(
            b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\n<html></html>",
        );
        let (overlong_range, overlong_range_server) = serve(
            b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/3\r\nConnection: close\r\n\r\nxx",
        );
        let (valid_listener, valid_address) =
            super::super::test_server::TestListener::bind().unwrap();
        let valid = Url::parse(&format!("http://{valid_address}/valid")).unwrap();
        let valid_server = thread::spawn(move || {
            let mut socket = valid_listener.accept().unwrap();
            let _ = super::super::test_server::read_request(&mut socket).unwrap();
            thread::sleep(Duration::from_millis(100));
            super::super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\nContent-Range: bytes 0-0/3\r\nConnection: close\r\n\r\nx",
            )
            .unwrap();
        });
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            drop(listener);
            Url::parse(&format!("http://{address}/closed")).unwrap()
        };
        let official = Url::parse("https://github.com/owner/repo/releases/file").unwrap();
        let client =
            crate::services::http::HttpClient::download_client(&NetworkSettings::default())
                .unwrap();
        let result = order_candidates(
            &client,
            vec![
                wrong_range.clone(),
                encoded.clone(),
                wrong_length.clone(),
                overlong_range.clone(),
                valid.clone(),
                closed.clone(),
                official.clone(),
            ],
            &HashSet::from(["127.0.0.1".into()]),
            Some(3),
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(result.first(), Some(&valid));
        assert_eq!(result.last(), Some(&official));
        assert!(result.contains(&closed));
        wrong_range_server.join().unwrap();
        encoded_server.join().unwrap();
        wrong_length_server.join().unwrap();
        overlong_range_server.join().unwrap();
        valid_server.join().unwrap();
    }

    #[tokio::test]
    async fn probes_three_mirrors_at_a_time() {
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let maximum = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut urls = Vec::new();
        let mut servers = Vec::new();
        for _ in 0..5 {
            let (listener, address) = super::super::test_server::TestListener::bind().unwrap();
            urls.push(Url::parse(&format!("http://{address}/probe")).unwrap());
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            servers.push(thread::spawn(move || {
                let Ok(mut socket) = listener.accept_for(Duration::from_secs(2)) else {
                    return;
                };
                let _ = super::super::test_server::read_request(&mut socket).unwrap();
                let now = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                maximum.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_secs(1);
                while active.load(std::sync::atomic::Ordering::SeqCst) < 3
                    && Instant::now() < deadline
                {
                    thread::sleep(Duration::from_millis(5));
                }
                let _ = super::super::test_server::write_all(
                    &mut socket,
                    b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\nContent-Range: bytes 0-0/1\r\nConnection: close\r\n\r\nx",
                );
                active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }));
        }
        let official = Url::parse("https://github.com/owner/repo/releases/file").unwrap();
        let client =
            crate::services::http::HttpClient::download_client(&NetworkSettings::default())
                .unwrap();
        order_candidates(
            &client,
            urls.into_iter().chain([official]).collect(),
            &HashSet::from(["127.0.0.1".into()]),
            Some(1),
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(maximum.load(std::sync::atomic::Ordering::SeqCst), 3);
        for server in servers {
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn probe_has_one_three_second_total_budget() {
        let (listener, address) = super::super::test_server::TestListener::bind().unwrap();
        let url = Url::parse(&format!("http://{address}/probe")).unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let mut socket = listener.accept().unwrap();
            let _ = super::super::test_server::read_request(&mut socket).unwrap();
            super::super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\nContent-Range: bytes 0-0/1\r\n\r\n",
            )
            .unwrap();
            let _ = released.recv_timeout(Duration::from_secs(5));
            drop(socket);
        });
        let client =
            crate::services::http::HttpClient::download_client(&NetworkSettings::default())
                .unwrap();
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(4),
            order_candidates(
                &client,
                vec![url],
                &HashSet::from(["127.0.0.1".into()]),
                Some(1),
                &AtomicBool::new(false),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(2_900));
        assert_eq!(result.len(), 1);
        release.send(()).unwrap();
        server.join().unwrap();
    }
}

//! Size-capped HTTP fetches for the admin-fixed catalog sources (models.dev pricing,
//! OpenRouter benchmarks) used by runtime refreshes and the vendored-snapshot sync bins.
//!
//! A source that starts streaming far more than its expected payload is not the expected
//! payload, so the body is rejected before it is buffered: by `Content-Length` when the
//! server sends one, and by a running total of received chunks otherwise.

use std::time::Duration;

use reqwest::{Client, RequestBuilder, StatusCode, header::ETAG};

const CATALOG_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CATALOG_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A client with the catalog connect and total request timeouts.
#[must_use]
pub fn catalog_http_client() -> Client {
    Client::builder()
        .connect_timeout(CATALOG_CONNECT_TIMEOUT)
        .timeout(CATALOG_REQUEST_TIMEOUT)
        .build()
        .expect("catalog HTTP client configuration is valid")
}

/// The result of a catalog request that the server answered with 200 or 304.
#[derive(Debug)]
pub enum CatalogFetch {
    /// HTTP 304 for a conditional (`If-None-Match`) request; there is no body.
    NotModified,
    Fetched(FetchedBody),
}

/// An HTTP 200 body read in full within the caller's size cap.
#[derive(Debug)]
pub struct FetchedBody {
    pub etag: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogFetchError {
    #[error("request failed: {0}")]
    Request(reqwest::Error),
    #[error("unexpected HTTP {}", .0.as_u16())]
    Status(StatusCode),
    #[error("response body exceeds {max_bytes} bytes")]
    TooLarge { max_bytes: usize },
    #[error("failed reading response body: {0}")]
    Body(reqwest::Error),
}

/// Send `request` and read a 200 body of at most `max_bytes`. Callers add `If-None-Match`
/// to the request for a conditional fetch; a 304 answer returns
/// [`CatalogFetch::NotModified`].
///
/// # Errors
///
/// Fails when the request cannot be sent, the status is neither 200 nor 304, the body
/// exceeds `max_bytes` (declared or received), or reading the body fails.
pub async fn fetch_bounded(
    request: RequestBuilder,
    max_bytes: usize,
) -> Result<CatalogFetch, CatalogFetchError> {
    let mut response = request.send().await.map_err(CatalogFetchError::Request)?;
    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_MODIFIED => return Ok(CatalogFetch::NotModified),
        status => return Err(CatalogFetchError::Status(status)),
    }

    let declared_len = response.content_length();
    if declared_len.is_some_and(|length| length > max_bytes as u64) {
        return Err(CatalogFetchError::TooLarge { max_bytes });
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    // `declared_len` is at most `max_bytes` here, so the reservation is bounded too.
    let mut bytes = Vec::with_capacity(declared_len.map_or(0, |length| length as usize));
    while let Some(chunk) = response.chunk().await.map_err(CatalogFetchError::Body)? {
        if bytes.len() + chunk.len() > max_bytes {
            return Err(CatalogFetchError::TooLarge { max_bytes });
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(CatalogFetch::Fetched(FetchedBody { etag, bytes }))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{Shutdown, TcpListener},
        thread,
    };

    use super::*;

    const MAX_BYTES: usize = 16;

    /// Serve one canned raw HTTP/1.1 response, so a test controls the framing
    /// (`Content-Length` versus chunked) exactly.
    fn serve_once(response: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = socket.read(&mut buf).expect("read request");
                if read == 0 {
                    return;
                }
                request.extend_from_slice(&buf[..read]);
            }
            // The client may hang up early once it rejects the response.
            let _ = socket.write_all(&response);
            let _ = socket.shutdown(Shutdown::Write);
        });
        format!("http://{addr}/catalog.json")
    }

    #[tokio::test]
    async fn oversize_content_length_is_rejected_before_the_body() {
        // The declared length is never sent in full: without the header check the
        // read would fail with a body error instead.
        let url = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\npartial"
                .to_vec(),
        );

        let result = fetch_bounded(catalog_http_client().get(url), MAX_BYTES).await;

        assert!(
            matches!(result, Err(CatalogFetchError::TooLarge { max_bytes }) if max_bytes == MAX_BYTES),
            "expected TooLarge, got {result:?}"
        );
    }

    #[tokio::test]
    async fn oversize_chunked_body_is_rejected() {
        let chunk = "a".repeat(MAX_BYTES);
        let url = serve_once(
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n\
                 {len:x}\r\n{chunk}\r\n{len:x}\r\n{chunk}\r\n0\r\n\r\n",
                len = chunk.len()
            )
            .into_bytes(),
        );

        let result = fetch_bounded(catalog_http_client().get(url), MAX_BYTES).await;

        assert!(
            matches!(result, Err(CatalogFetchError::TooLarge { max_bytes }) if max_bytes == MAX_BYTES),
            "expected TooLarge, got {result:?}"
        );
    }

    #[tokio::test]
    async fn body_of_exactly_the_cap_is_returned_with_its_etag() {
        let body = "a".repeat(MAX_BYTES);
        let url = serve_once(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        );

        let result = fetch_bounded(catalog_http_client().get(url), MAX_BYTES).await;

        let Ok(CatalogFetch::Fetched(body)) = result else {
            panic!("expected a fetched body, got {result:?}");
        };
        assert_eq!(body.bytes.len(), MAX_BYTES);
        assert_eq!(body.etag.as_deref(), Some("\"v1\""));
    }
}

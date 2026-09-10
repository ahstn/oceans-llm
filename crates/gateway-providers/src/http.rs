use std::{
    pin::Pin,
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use gateway_core::ProviderError;
use tracing::{Instrument, Span};

pub type TracedResponseStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

pub(crate) fn provider_http_client(
    total_timeout_ms: u64,
) -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_millis(total_timeout_ms))
        .build()
        .map_err(map_reqwest_error)
}

pub struct TracedResponse {
    response: reqwest::Response,
    span: Span,
    started_at: Instant,
}

struct ResponseTiming {
    span: Span,
    started_at: Option<Instant>,
}

impl ResponseTiming {
    fn finish(&mut self) {
        if let Some(started_at) = self.started_at.take() {
            self.span.record(
                "gateway.upstream.elapsed_ms",
                started_at.elapsed().as_millis() as u64,
            );
        }
    }
}

impl Drop for ResponseTiming {
    fn drop(&mut self) {
        self.finish();
    }
}

impl TracedResponse {
    pub fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }

    pub fn headers(&self) -> &reqwest::header::HeaderMap {
        self.response.headers()
    }

    pub async fn text(self) -> Result<String, reqwest::Error> {
        let Self {
            response,
            span,
            started_at,
        } = self;
        let result = response.text().instrument(span.clone()).await;
        span.record(
            "gateway.upstream.elapsed_ms",
            started_at.elapsed().as_millis() as u64,
        );
        if let Err(error) = &result {
            record_reqwest_error(&span, error);
        }
        result
    }

    pub fn bytes_stream(self) -> TracedResponseStream {
        let Self {
            response,
            span,
            started_at,
        } = self;
        let mut timing = ResponseTiming {
            span: span.clone(),
            started_at: Some(started_at),
        };
        Box::pin(async_stream::stream! {
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                if let Err(error) = &chunk {
                    timing.finish();
                    record_reqwest_error(&span, error);
                    yield chunk;
                    return;
                }
                yield chunk;
            }
            timing.finish();
        })
    }
}

pub fn join_base_url(base_url: &str, suffix: &str) -> Result<String, ProviderError> {
    let base = base_url.trim_end_matches('/');
    let endpoint = suffix.trim_start_matches('/');

    let full = format!("{base}/{endpoint}");
    url::Url::parse(&full).map_err(|error| ProviderError::Transport(error.to_string()))?;
    Ok(full)
}

pub fn map_reqwest_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else {
        ProviderError::Transport(error.to_string())
    }
}

pub async fn execute_request(
    client: &reqwest::Client,
    request: reqwest::Request,
    provider_type: &str,
    provider_key: &str,
) -> Result<TracedResponse, reqwest::Error> {
    let method = request.method().as_str().to_string();
    let url = request.url();
    let server_address = url.host_str().unwrap_or("unknown").to_string();
    let server_port = url.port().map(u64::from);
    let safe_url = safe_url(url);
    let span = tracing::info_span!(
        "http.client.request",
        otel.name = %method,
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
        http.request.method = %method,
        http.response.status_code = tracing::field::Empty,
        url.full = %safe_url,
        server.address = %server_address,
        server.port = server_port,
        error.type = tracing::field::Empty,
        gen_ai.provider.name = %provider_type,
        gateway.provider.key = %provider_key,
        gateway.upstream.elapsed_ms = tracing::field::Empty,
        gateway.upstream.request_id = tracing::field::Empty,
        gateway.upstream.cf_ray = tracing::field::Empty,
        gateway.upstream.error_chain = tracing::field::Empty,
    );
    let started_at = Instant::now();
    match client.execute(request).instrument(span.clone()).await {
        Ok(response) => {
            let status = response.status();
            span.record("http.response.status_code", status.as_u16());
            for (header, field) in [
                ("x-request-id", "gateway.upstream.request_id"),
                ("cf-ray", "gateway.upstream.cf_ray"),
            ] {
                if let Some(value) = response
                    .headers()
                    .get(header)
                    .and_then(|value| value.to_str().ok())
                {
                    span.record(field, value);
                }
            }
            if status.is_client_error() || status.is_server_error() {
                let error_type = status.as_u16().to_string();
                span.record("error.type", error_type);
                span.record("otel.status_code", "ERROR");
            }
            Ok(TracedResponse {
                response,
                span,
                started_at,
            })
        }
        Err(error) => {
            span.record(
                "gateway.upstream.elapsed_ms",
                started_at.elapsed().as_millis() as u64,
            );
            record_reqwest_error(&span, &error);
            Err(error)
        }
    }
}

fn record_reqwest_error(span: &Span, error: &reqwest::Error) {
    span.record("error.type", reqwest_error_type(error));
    span.record("otel.status_code", "ERROR");
    let chain = reqwest_error_chain(error);
    span.record("gateway.upstream.error_chain", &chain);
    tracing::warn!(parent: span, error_type = reqwest_error_type(error), error_chain = %chain,
        "upstream HTTP request failed");
}

// Keep the source chain in server diagnostics, not in client-visible errors.
fn reqwest_error_chain(error: &reqwest::Error) -> String {
    let mut messages = Vec::new();
    let mut urls = Vec::new();
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(source) = current {
        if let Some(error) = source.downcast_ref::<reqwest::Error>()
            && let Some(url) = error.url()
        {
            urls.push(url.as_str());
        }
        messages.push(source.to_string());
        current = source.source();
        if messages.len() == 8 {
            break;
        }
    }
    let mut chain = messages.join(": ");
    for url in urls {
        chain = chain.replace(url, "[upstream URL]");
    }
    chain.chars().take(2048).collect()
}

pub(crate) fn stream_read_error_message(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "Upstream response timed out while reading the stream; check the provider total deadline and request trace.".to_owned()
    } else {
        format!(
            "Upstream response stream failed ({}); check the request trace for the underlying cause.",
            reqwest_error_type(error)
        )
    }
}

fn safe_url(url: &url::Url) -> String {
    let mut safe = url.clone();
    safe.set_query(None);
    safe.set_fragment(None);
    let _ = safe.set_username("");
    let _ = safe.set_password(None);
    safe.to_string()
}

fn reqwest_error_type(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_request() {
        "request"
    } else if error.is_body() {
        "body"
    } else if error.is_decode() {
        "decode"
    } else {
        "transport"
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) async fn timed_out_body_error() -> reqwest::Error {
        use axum::{Router, body::Body, routing::get};
        use std::time::Duration;

        let app = Router::new().route(
            "/",
            get(|| async {
                let chunks = futures_util::stream::once(async {
                    Ok::<_, std::io::Error>(Bytes::from_static(b": keepalive\n\n"))
                })
                .chain(futures_util::stream::pending());
                Body::from_stream(chunks)
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = provider_http_client(500).unwrap();
        let request = client.get(format!("http://{address}/")).build().unwrap();
        let response = execute_request(&client, request, "test", "test")
            .await
            .unwrap();
        let mut stream = response.bytes_stream();
        assert!(stream.next().await.unwrap().is_ok());
        let error = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.is_timeout());
        assert_eq!(reqwest_error_type(&error), "timeout");
        assert!(reqwest_error_chain(&error).contains("timed out"));
        server.abort();
        error
    }

    #[tokio::test]
    async fn stream_deadline_preserves_timeout_cause_and_emits_failure_without_done() {
        let error = timed_out_body_error().await;
        let upstream = futures_util::stream::once(async move { Err(error) });
        let chunks = crate::streaming::normalize_openai_compat_responses_stream(upstream)
            .collect::<Vec<_>>()
            .await;
        let output = chunks
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
            .collect::<String>();
        assert!(output.contains("upstream_openai_compat_responses_stream_error"));
        assert!(output.contains("timed out"));
        assert!(!output.contains("[DONE]"));
    }

    #[tokio::test]
    async fn stream_timing_records_once_on_eof_error_or_drop() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use tracing::instrument::WithSubscriber;
        use tracing_subscriber::{Layer, layer::SubscriberExt};

        #[derive(Clone)]
        struct Records(Arc<AtomicUsize>);
        impl<S: tracing::Subscriber> Layer<S> for Records {
            fn on_record(
                &self,
                _: &tracing::span::Id,
                values: &tracing::span::Record<'_>,
                _: tracing_subscriber::layer::Context<'_, S>,
            ) {
                struct Visitor<'a>(&'a AtomicUsize);
                impl tracing::field::Visit for Visitor<'_> {
                    fn record_debug(
                        &mut self,
                        field: &tracing::field::Field,
                        _: &dyn std::fmt::Debug,
                    ) {
                        if field.name() == "gateway.upstream.elapsed_ms" {
                            self.0.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
                values.record(&mut Visitor(&self.0));
            }
        }
        let app = axum::Router::new().route("/", axum::routing::get(|| async { "chunk" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        for poll_count in [0, 1, 2] {
            let records = Records(Arc::new(AtomicUsize::new(0)));
            let subscriber = tracing_subscriber::registry().with(records.clone());
            async {
                let client = provider_http_client(1000).unwrap();
                let request = client.get(format!("http://{address}/")).build().unwrap();
                let mut stream = execute_request(&client, request, "test", "test")
                    .await
                    .unwrap()
                    .bytes_stream();
                for _ in 0..poll_count {
                    let _ = stream.next().await;
                }
                drop(stream);
            }
            .with_subscriber(subscriber)
            .await;
            assert_eq!(records.0.load(Ordering::SeqCst), 1, "polls: {poll_count}");
        }
        let records = Records(Arc::new(AtomicUsize::new(0)));
        let subscriber = tracing_subscriber::registry().with(records.clone());
        async {
            let _ = timed_out_body_error().await;
        }
        .with_subscriber(subscriber)
        .await;
        assert_eq!(records.0.load(Ordering::SeqCst), 1, "timeout then drop");
        server.abort();
    }

    #[tokio::test]
    async fn diagnostic_chain_removes_url_credentials_and_query() {
        let error = reqwest::Client::new()
            .get("ftp://user:secret@example.com/path?token=private")
            .send()
            .await
            .unwrap_err();
        let chain = reqwest_error_chain(&error);
        assert!(!chain.contains("secret"));
        assert!(!chain.contains("private"));
        assert!(!chain.is_empty());
    }

    #[test]
    fn safe_url_removes_credentials_query_and_fragment() {
        let url =
            url::Url::parse("https://user:secret@example.com/v1/responses?api_key=secret#fragment")
                .expect("url");

        assert_eq!(safe_url(&url), "https://example.com/v1/responses");
    }
}

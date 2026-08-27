use std::pin::Pin;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use gateway_core::ProviderError;
use tracing::{Instrument, Span};

pub type TracedResponseStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

pub struct TracedResponse {
    response: reqwest::Response,
    span: Span,
}

impl TracedResponse {
    pub fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }

    pub fn headers(&self) -> &reqwest::header::HeaderMap {
        self.response.headers()
    }

    pub async fn text(self) -> Result<String, reqwest::Error> {
        let Self { response, span } = self;
        let result = response.text().instrument(span.clone()).await;
        if let Err(error) = &result {
            record_reqwest_error(&span, error);
        }
        result
    }

    pub fn bytes_stream(self) -> TracedResponseStream {
        let Self { response, span } = self;
        let state = TracedResponseStreamState {
            stream: Box::pin(response.bytes_stream()),
            span: Some(span),
        };
        Box::pin(futures_util::stream::unfold(
            state,
            |mut state| async move {
                match state.stream.next().await {
                    Some(Ok(bytes)) => Some((Ok(bytes), state)),
                    Some(Err(error)) => {
                        if let Some(span) = state.span.take() {
                            record_reqwest_error(&span, &error);
                        }
                        Some((Err(error), state))
                    }
                    None => {
                        state.span.take();
                        None
                    }
                }
            },
        ))
    }
}

struct TracedResponseStreamState {
    stream: TracedResponseStream,
    span: Option<Span>,
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
    );

    match client.execute(request).instrument(span.clone()).await {
        Ok(response) => {
            let status = response.status();
            span.record("http.response.status_code", status.as_u16());
            if status.is_client_error() || status.is_server_error() {
                let error_type = status.as_u16().to_string();
                span.record("error.type", error_type);
                span.record("otel.status_code", "ERROR");
            }
            Ok(TracedResponse { response, span })
        }
        Err(error) => {
            record_reqwest_error(&span, &error);
            Err(error)
        }
    }
}

fn record_reqwest_error(span: &Span, error: &reqwest::Error) {
    span.record("error.type", reqwest_error_type(error));
    span.record("otel.status_code", "ERROR");
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
mod tests {
    use super::*;

    #[test]
    fn safe_url_removes_credentials_query_and_fragment() {
        let url =
            url::Url::parse("https://user:secret@example.com/v1/responses?api_key=secret#fragment")
                .expect("url");

        assert_eq!(safe_url(&url), "https://example.com/v1/responses");
    }
}

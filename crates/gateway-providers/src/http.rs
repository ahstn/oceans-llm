use gateway_core::ProviderError;
use tracing::Instrument;

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
) -> Result<reqwest::Response, reqwest::Error> {
    let method = request.method().as_str().to_string();
    let url = request.url();
    let server_address = url.host_str().unwrap_or("unknown").to_string();
    let server_port = url.port_or_known_default().map(u64::from);
    let safe_url = safe_url(url);
    let span_name = format!("{method} {server_address}");
    let span = tracing::info_span!(
        "http.client.request",
        otel.name = %span_name,
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

    async move {
        match client.execute(request).await {
            Ok(response) => {
                let status = response.status();
                tracing::Span::current().record("http.response.status_code", status.as_u16());
                if status.is_client_error() || status.is_server_error() {
                    let error_type = status.as_u16().to_string();
                    tracing::Span::current().record("error.type", error_type);
                    tracing::Span::current().record("otel.status_code", "ERROR");
                }
                Ok(response)
            }
            Err(error) => {
                tracing::Span::current().record("error.type", reqwest_error_type(&error));
                tracing::Span::current().record("otel.status_code", "ERROR");
                Err(error)
            }
        }
    }
    .instrument(span)
    .await
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

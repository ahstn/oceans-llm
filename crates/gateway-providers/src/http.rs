use gateway_core::ProviderError;

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

use super::*;

#[test]
fn resolves_default_endpoint_from_region() {
    let endpoint = BedrockProviderConfig::resolved_endpoint_url(
        BedrockEndpointKind::BedrockRuntime,
        "us-east-1",
        None,
    )
    .expect("endpoint");
    assert_eq!(endpoint, "https://bedrock-runtime.us-east-1.amazonaws.com");
}

#[test]
fn resolves_mantle_default_endpoint_from_region() {
    let endpoint = BedrockProviderConfig::resolved_endpoint_url(
        BedrockEndpointKind::BedrockMantle,
        "us-east-2",
        None,
    )
    .expect("endpoint");
    assert_eq!(endpoint, "https://bedrock-mantle.us-east-2.api.aws");
}

#[test]
fn normalizes_custom_endpoint_trailing_slash() {
    let endpoint = BedrockProviderConfig::resolved_endpoint_url(
        BedrockEndpointKind::BedrockRuntime,
        "us-east-1",
        Some("https://bedrock-runtime.us-west-2.amazonaws.com/"),
    )
    .expect("endpoint");
    assert_eq!(endpoint, "https://bedrock-runtime.us-west-2.amazonaws.com");
}

#[test]
fn preserves_arn_delimiters_for_all_runtime_operations() {
    let provider = static_credentials_provider(None);
    let arn = "arn:aws:bedrock:us-east-1:123456789012:application-inference-profile/abc123xyz";

    assert_eq!(
        provider.converse_endpoint(arn),
        format!("https://bedrock-runtime.us-east-1.amazonaws.com/model/{arn}/converse")
    );
    assert_eq!(
        provider.converse_stream_endpoint(arn),
        format!("https://bedrock-runtime.us-east-1.amazonaws.com/model/{arn}/converse-stream")
    );
    assert_eq!(
        provider.invoke_endpoint(arn),
        format!("https://bedrock-runtime.us-east-1.amazonaws.com/model/{arn}/invoke")
    );
}

#[test]
fn safely_encodes_non_arn_model_identifiers() {
    let provider = static_credentials_provider(None);

    assert_eq!(
        provider.converse_endpoint("vendor/model name:v1"),
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/vendor%2Fmodel%20name%3Av1/converse"
    );
}

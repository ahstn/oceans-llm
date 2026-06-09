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

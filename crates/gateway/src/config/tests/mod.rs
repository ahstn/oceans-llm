use std::{env, path::Path};

use gateway_core::{
    AuthMode, AwsBedrockApiStyle, BudgetCadence, GitHubCopilotChatApi, GlobalRole,
    ManagedApiKeySource, MembershipRole, Money4, OpenAiCompatDeveloperRole, OpenAiCompatEmptyTools,
    OpenAiCompatMaxTokensField, OpenAiCompatReasoningEffort, OpenRouterPercentilePreference,
    RequestLogRetentionWindow,
};
use gateway_providers::{BearerAuthHeader, BedrockAuthConfig, CopilotAuthConfig};
use gateway_service::RequestLogPayloadCaptureMode;
use gateway_store::StoreConnectionOptions;
use tempfile::tempdir;

use super::{
    AwsBedrockRouteCompatibilityConfig, GatewayConfig, McpOauthConfig, McpOauthProviderConfig,
    default_google_authorization_url, default_google_token_url,
};

fn write_config(path: &Path, yaml: &str) {
    std::fs::write(path, yaml).expect("write config");
}

mod auth;
mod bedrock;
mod budgets;
mod github_copilot;
mod identity;
mod infrastructure;
mod mcp;
mod models;
mod openrouter;
mod providers;
mod routes;

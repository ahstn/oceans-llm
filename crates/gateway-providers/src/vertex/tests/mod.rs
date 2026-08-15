
use std::{collections::BTreeMap, convert::Infallible, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::post,
};
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream;
use gateway_core::{
    CoreChatMessage, CoreChatRequest, CoreEmbeddingsRequest, ProviderClient, ProviderRequestContext,
};
use serde_json::{Map, Value, json};
use tokio::{net::TcpListener, sync::Mutex};

use super::{
    JsonObjectParser, PublisherFamily, SseEventParser, VertexAuthConfig, VertexProvider,
    VertexProviderConfig, extract_google_candidate_text, map_anthropic_request,
    map_google_embedding_request, map_google_request, normalize_anthropic_response,
    normalize_anthropic_stream, normalize_google_response, normalize_google_stream,
    parse_upstream_model,
};
use gateway_core::ProviderError;

mod anthropic_request;
mod anthropic_thinking;
mod embeddings;
mod google_request;
mod google_tools;
mod provider;
mod response;
mod streaming;

fn context(upstream_model: &str) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "req-1".to_string(),
        model_key: "fast".to_string(),
        provider_key: "vertex-prod".to_string(),
        upstream_model: upstream_model.to_string(),
        extra_headers: Map::new(),
        extra_body: Map::new(),
        request_headers: std::collections::BTreeMap::new(),
        compatibility: Default::default(),
    }
}
fn vertex_provider_for_test(api_host: String) -> VertexProvider {
    VertexProvider::new(VertexProviderConfig {
        provider_key: "vertex-prod".to_string(),
        project_id: "proj-123".to_string(),
        location: "global".to_string(),
        api_host,
        auth: VertexAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
        default_headers: BTreeMap::new(),
        request_timeout_ms: 5_000,
    })
    .expect("provider")
}

fn chat_request(messages: Vec<CoreChatMessage>) -> CoreChatRequest {
    CoreChatRequest {
        model: "fast".to_string(),
        messages,
        stream: false,
        extra: BTreeMap::new(),
    }
}

fn embedding_request(input: Value) -> CoreEmbeddingsRequest {
    CoreEmbeddingsRequest {
        model: "fast".to_string(),
        input,
        extra: BTreeMap::new(),
    }
}

async fn start_router(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr.to_string()
}

fn openai_stream_events(rendered: &str) -> Vec<Value> {
    rendered
        .split("\n\n")
        .filter_map(|frame| {
            frame
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .filter(|data| *data != "[DONE]")
                .and_then(|data| serde_json::from_str::<Value>(data).ok())
        })
        .collect()
}

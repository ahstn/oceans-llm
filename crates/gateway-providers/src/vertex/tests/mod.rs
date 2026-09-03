use std::{collections::BTreeMap, convert::Infallible, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
};
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream;
use gateway_core::{
    CoreChatMessage, CoreChatRequest, CoreEmbeddingsRequest, ProviderClient, ProviderError,
    ProviderRequestContext,
};
use serde_json::{Map, Value, json};
use tokio::{net::TcpListener, sync::Mutex};

use super::{
    PublisherFamily, VertexAuthConfig, VertexProvider, VertexProviderConfig,
    anthropic::map_vertex_anthropic_request,
    embeddings::{
        VERTEX_PREDICT_MAX_INSTANCES, VERTEX_PREDICT_MAX_TOKENS, estimated_tokens,
        map_google_embedding_request, predict_max_instances,
    },
    error::VertexAdapterError,
    gemini::{GeminiModel, ThinkingControl},
    google_request::map_google_request,
    google_response::{map_google_usage, normalize_google_response},
    google_stream::{GoogleStreamState, normalize_google_stream},
    parse_upstream_model, vertex_api_host_for_location,
};

mod anthropic_request;
mod embeddings;
mod gemini;
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
        owner_user_id: None,
        extra_headers: Map::new(),
        extra_body: Map::new(),
        request_headers: std::collections::BTreeMap::new(),
        compatibility: Default::default(),
    }
}
fn vertex_provider_for_test(api_host: String) -> VertexProvider {
    vertex_provider_with_headers(api_host, BTreeMap::new())
}

fn vertex_provider_with_headers(
    api_host: String,
    default_headers: BTreeMap<String, String>,
) -> VertexProvider {
    VertexProvider::new(VertexProviderConfig {
        provider_key: "vertex-prod".to_string(),
        project_id: "proj-123".to_string(),
        location: "global".to_string(),
        api_host,
        auth: VertexAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
        default_headers,
        request_timeout_ms: 5_000,
        batch: None,
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

/// Maps a chat request for a Google model, deriving `model_id` from the context.
fn google_body(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
) -> Result<Value, ProviderError> {
    let (_, _, model_id) = parse_upstream_model(&context.upstream_model)?;
    map_google_request(request, context, model_id, stream)
}

/// Maps a chat request for an Anthropic model with no provider default headers.
fn anthropic_body(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
) -> Result<Value, ProviderError> {
    map_vertex_anthropic_request(request, context, stream, &BTreeMap::new())
}

/// Encodes upstream JSON objects as the `alt=sse` frames Vertex streams.
fn google_sse_frames(objects: &[Value]) -> String {
    objects
        .iter()
        .map(|object| format!("data: {object}\r\n\r\n"))
        .collect()
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

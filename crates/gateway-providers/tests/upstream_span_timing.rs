//! The exported upstream `CLIENT` span must cover the whole streamed body, not
//! only the wait for response headers. tracing-opentelemetry ends a span when
//! it is last exited, so a span that is only held open by the body stream
//! ends too early.

use std::time::Duration;

use axum::{Router, body::Body, response::Response, routing::post};
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{ProviderClient, ProviderRequestContext, openai_chat_request_to_core};
use gateway_providers::{OpenAiCompatConfig, OpenAiCompatProvider};
use opentelemetry::trace::{SpanKind, TracerProvider as _};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use serde_json::json;
use tracing_subscriber::layer::SubscriberExt;

const BODY_DELAY: Duration = Duration::from_millis(300);

async fn slow_event_stream() -> Response {
    let first = futures_util::stream::once(async {
        Ok::<_, std::io::Error>(Bytes::from_static(
            b"data: {\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        ))
    });
    let last = futures_util::stream::once(async {
        tokio::time::sleep(BODY_DELAY).await;
        Ok(Bytes::from_static(b"data: [DONE]\n\n"))
    });
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(first.chain(last)))
        .unwrap()
}

#[tokio::test]
async fn upstream_client_span_covers_the_streamed_body() {
    let exporter = InMemorySpanExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    // This test binary holds one test, so the global subscriber is set before any span exists.
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer("test"))),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let app = Router::new().route("/v1/chat/completions", post(slow_event_stream));
        axum::serve(listener, app).await.unwrap();
    });

    let provider =
        OpenAiCompatProvider::new(OpenAiCompatConfig::new("upstream".into(), base_url)).unwrap();
    let request = openai_chat_request_to_core(
        &serde_json::from_value(json!({
            "model": "m",
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}],
        }))
        .unwrap(),
    );
    let context: ProviderRequestContext = serde_json::from_value(json!({
        "request_id": "request-1",
        "model_key": "m",
        "provider_key": "upstream",
        "upstream_model": "m",
    }))
    .unwrap();

    let mut stream = provider
        .chat_completions_stream(&request, &context)
        .await
        .unwrap();
    while let Some(chunk) = stream.next().await {
        chunk.unwrap();
    }
    drop(stream);
    server.abort();

    let spans = exporter.get_finished_spans().unwrap();
    let client = spans
        .iter()
        .find(|span| span.span_kind == SpanKind::Client)
        .expect("upstream client span exported");
    let duration = client.end_time.duration_since(client.start_time).unwrap();
    assert!(
        duration >= BODY_DELAY,
        "client span lasted {duration:?}, but the body took at least {BODY_DELAY:?}"
    );
}

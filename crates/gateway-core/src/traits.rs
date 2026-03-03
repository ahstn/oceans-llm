use std::{collections::HashMap, pin::Pin, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    domain::{ApiKeyRecord, GatewayModel, ModelRoute, ProviderConnection, ProviderRequestContext},
    error::{ProviderError, RouteError, StoreError},
    protocol::openai::{ChatCompletionsRequest, EmbeddingsRequest},
};

#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn get_api_key_by_public_id(
        &self,
        public_id: &str,
    ) -> Result<Option<ApiKeyRecord>, StoreError>;

    async fn touch_api_key_last_used(&self, api_key_id: Uuid) -> Result<(), StoreError>;
}

#[async_trait]
pub trait ModelRepository: Send + Sync {
    async fn get_model_by_key(&self, model_key: &str) -> Result<Option<GatewayModel>, StoreError>;
    async fn list_models_for_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<GatewayModel>, StoreError>;
    async fn list_routes_for_model(&self, model_id: Uuid) -> Result<Vec<ModelRoute>, StoreError>;
}

#[async_trait]
pub trait ProviderRepository: Send + Sync {
    async fn get_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<ProviderConnection>, StoreError>;
}

#[async_trait]
pub trait StoreHealth: Send + Sync {
    async fn ping(&self) -> Result<(), StoreError>;
}

pub trait RoutePlanner: Send + Sync {
    fn plan_routes(&self, routes: &[ModelRoute]) -> Result<Vec<ModelRoute>, RouteError>;
}

pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>;

#[async_trait]
pub trait ProviderClient: Send + Sync {
    fn provider_key(&self) -> &str;
    fn provider_type(&self) -> &str;

    async fn chat_completions(
        &self,
        request: &ChatCompletionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError>;

    async fn chat_completions_stream(
        &self,
        request: &ChatCompletionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError>;

    async fn embeddings(
        &self,
        request: &EmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError>;
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ProviderClient>>,
}

impl ProviderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn ProviderClient>) {
        self.providers
            .insert(provider.provider_key().to_string(), provider);
    }

    #[must_use]
    pub fn get(&self, provider_key: &str) -> Option<Arc<dyn ProviderClient>> {
        self.providers.get(provider_key).cloned()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

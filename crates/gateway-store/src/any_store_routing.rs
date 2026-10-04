use async_trait::async_trait;
use gateway_core::{
    ResponseRouteOrigin, RouteBindingReceipt, RouteSelection, RouteSelectionRequest,
    RoutingRepository, StoreError,
};
use time::OffsetDateTime;

use crate::AnyStore;

#[async_trait]
impl RoutingRepository for AnyStore {
    async fn select_route(
        &self,
        request: &RouteSelectionRequest,
    ) -> Result<RouteSelection, StoreError> {
        match self {
            Self::Libsql(store) => store.select_route(request).await,
            Self::Postgres(store) => store.select_route(request).await,
        }
    }

    async fn refresh_route_binding(
        &self,
        receipt: &RouteBindingReceipt,
        now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.refresh_route_binding(receipt, now).await,
            Self::Postgres(store) => store.refresh_route_binding(receipt, now).await,
        }
    }

    async fn get_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        now: OffsetDateTime,
    ) -> Result<Option<ResponseRouteOrigin>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_response_route_origin(owner_key, response_id_hash, now)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_response_route_origin(owner_key, response_id_hash, now)
                    .await
            }
        }
    }

    async fn record_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        origin: &ResponseRouteOrigin,
        now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .record_response_route_origin(owner_key, response_id_hash, origin, now)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .record_response_route_origin(owner_key, response_id_hash, origin, now)
                    .await
            }
        }
    }
}

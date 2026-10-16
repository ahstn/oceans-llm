//! Client IP resolution through trusted proxies.
//!
//! The resolver lives in `gateway_client_ip`, which the admin UI proxy also uses,
//! so both resolve the client the same way.

pub use gateway_client_ip::{TrustedProxies, resolve_client_ip};

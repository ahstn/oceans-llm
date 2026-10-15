//! Client IP resolution through trusted proxies.
//!
//! The resolver lives in `admin_ui::client_ip` so the admin UI proxy can resolve
//! the client the same way before it signs the address.

pub use admin_ui::client_ip::{TrustedProxies, resolve_client_ip};

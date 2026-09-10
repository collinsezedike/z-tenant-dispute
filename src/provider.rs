//! Shared payment-provider selection and secret lookup.
//!
//! Each exported function accepts an optional `provider` field (defaulting
//! to `stripe`, so anything already registered/tested against Stripe keeps
//! working unchanged) and branches between provider-specific request/response
//! handling in `order.rs`, `dispute.rs`, and `evidence.rs`. The WIT interface
//! and PII-placeholder mechanism are identical either way; only the HTTP
//! shapes differ.

#[derive(serde::Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Stripe,
    Paystack,
}

#[cfg(target_arch = "wasm32")]
use crate::host::{interfaces::kv_store, tenant::tenant_context};

/// Reads `stripe_secret_key` or `paystack_secret_key` from the tenant's
/// `secrets` KV map, matching the provider on the request.
#[cfg(target_arch = "wasm32")]
pub fn get_secret(provider: Provider) -> Result<alloc::string::String, alloc::string::String> {
    let key_name: &[u8] = match provider {
        Provider::Stripe => b"stripe_secret_key",
        Provider::Paystack => b"paystack_secret_key",
    };
    let tid = tenant_context::tenant_did();
    let map_name = alloc::format!("z:{}:secrets", hex::encode(&tid));
    let bytes = kv_store::get(&map_name, key_name)
        .map_err(|e| alloc::format!("kv read: {e}"))?
        .ok_or_else(|| {
            alloc::format!(
                "{} not found in z:<tid>:secrets. Populate it via the tenant SDK before use",
                alloc::string::String::from_utf8_lossy(key_name)
            )
        })?;
    alloc::string::String::from_utf8(bytes).map_err(|e| e.to_string())
}

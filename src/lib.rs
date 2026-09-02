//! z-tenant-dispute v0.1.0 — restaurant chargeback & dispute agent.
//!
//! Given a disputed transaction, this contract:
//!   - `check-order`: looks up the underlying payment/order status (no PII).
//!   - `get-payment-dispute`: looks up the chargeback/dispute status (no PII).
//!   - `submit-dispute-evidence`: assembles and submits dispute evidence to
//!     the payment processor (Stripe, in this reference build). The
//!     disputing customer's name and contact info are NEVER passed in as a
//!     contract argument: the contract templates `{{profile.<field>}}`
//!     markers into the evidence body and the host's
//!     `http-with-placeholders` interface resolves them from the calling
//!     user's profile at dispatch time, so plaintext customer PII never
//!     enters WASM memory.
//!
//! The Stripe secret key is read from the z: KV map `secrets` (key:
//! `stripe_secret_key`). This map is created and populated by the tenant SDK
//! before the contract runs.
//!
//! # Host-capability requirements
//!
//! Declare in manifest (access to a user's profile is gated by the on-chain
//! agent delegation grant, not a per-field allowlist):
//! ```json
//! {
//!   "host_capabilities": [
//!     "kv_store", "logging", "tenant_context", "http", "http_with_placeholders"
//!   ]
//! }
//! ```
//!
//! # Setup
//!
//! Before first use, the tenant SDK must create the `secrets` KV map and
//! write the Stripe secret key:
//! ```text
//! // Via the tenant SDK (before contract first use):
//! z_sdk.kv("secrets").set("stripe_secret_key", "sk_test_your_key_here")
//! ```
#![warn(clippy::style, missing_debug_implementations)]
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

extern crate alloc;

pub const CONTRACT_VERSION: &str = "0.1.0";

wit_bindgen::generate!({
    world: "tenant-dispute",
    path: "wit",
    additional_derives: [
        serde::Deserialize,
        serde::Serialize,
    ],
    generate_all,
});

mod dispute;
mod evidence;
mod order;

struct Component;

#[cfg(target_arch = "wasm32")]
impl exports::z::tenant_dispute::contracts::Guest for Component {
    fn check_order(
        req: exports::z::tenant_dispute::contracts::GenericInput,
    ) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
        let input = req.input.ok_or("check-order: missing input")?;
        order::check_order(&input)
    }

    fn get_payment_dispute(
        req: exports::z::tenant_dispute::contracts::GenericInput,
    ) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
        let input = req.input.ok_or("get-payment-dispute: missing input")?;
        dispute::get_payment_dispute(&input)
    }

    fn submit_dispute_evidence(
        req: exports::z::tenant_dispute::contracts::GenericInput,
    ) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
        let input = req.input.ok_or("submit-dispute-evidence: missing input")?;
        evidence::submit_dispute_evidence(&input)
    }
}

#[cfg(target_arch = "wasm32")]
export!(Component);

#[cfg(test)]
mod tests {
    use super::CONTRACT_VERSION;

    #[test]
    fn contract_version_is_semver() {
        let parts: Vec<&str> = CONTRACT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "CONTRACT_VERSION must be MAJOR.MINOR.PATCH");
        for part in parts {
            assert!(part.parse::<u32>().is_ok(), "each part must be a number");
        }
    }
}

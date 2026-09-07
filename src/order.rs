//! check_order: looks up the payment/order record behind a transaction.
//!
//! Supports two providers, selected by the request's `provider` field
//! (defaults to `stripe`):
//!   - Stripe: treats a PaymentIntent as the order record.
//!   - Paystack: verifies a transaction by its reference.
//! Both paths are plain synchronous HTTP calls — no PII either way. To swap
//! in a different order-management system entirely, add a new `Provider`
//! variant and a matching branch here — the WIT interface, secret-handling
//! pattern, and PII-placeholder mechanism (used only in `evidence.rs`) stay
//! the same.

use crate::provider::Provider;

#[derive(serde::Deserialize)]
pub struct CheckOrderReq {
    /// Stripe PaymentIntent id, or Paystack transaction reference.
    #[serde(alias = "payment_intent_id", alias = "reference")]
    pub order_ref: String,
    #[serde(default)]
    pub provider: Provider,
}

#[derive(serde::Serialize)]
pub struct OrderStatus {
    pub id: String,
    pub status: String,
    pub amount: i64,
    pub currency: String,
    /// Stripe: unix seconds as a string. Paystack: ISO-8601 `created_at`.
    pub created: alloc::string::String,
}

const STRIPE_BASE: &str = "https://api.stripe.com/v1";
const PAYSTACK_BASE: &str = "https://api.paystack.co";

/// Entry point called from `lib.rs`. `input` is the raw JSON bytes from the
/// node's `generic-input.input` field.
pub fn check_order(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: CheckOrderReq = serde_json::from_slice(input)
        .map_err(|e| alloc::format!("check-order: bad input: {e}"))?;

    #[cfg(target_arch = "wasm32")]
    {
        let resp = match req.provider {
            Provider::Stripe => check_order_stripe(&req.order_ref)?,
            Provider::Paystack => check_order_paystack(&req.order_ref)?,
        };
        serde_json::to_vec(&resp).map_err(|e| e.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = req;
        Err("check_order is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::interfaces::{http as http_iface, logging};

#[cfg(target_arch = "wasm32")]
fn check_order_stripe(payment_intent_id: &str) -> Result<OrderStatus, String> {
    let api_key = crate::provider::get_secret(Provider::Stripe)?;

    let resp = http_iface::call(&http_iface::Request {
        method: http_iface::Verb::Get,
        url: alloc::format!("{STRIPE_BASE}/payment_intents/{payment_intent_id}"),
        headers: Some(bearer_header(&api_key)),
        payload: None,
    })
    .map_err(|e| alloc::format!("stripe payment-intent lookup: {e}"))?;

    if resp.code != 200 {
        let body = alloc::string::String::from_utf8_lossy(&resp.payload);
        return Err(alloc::format!(
            "Stripe payment-intent lookup failed: HTTP {} — {body}",
            resp.code
        ));
    }

    let pi: serde_json::Value =
        serde_json::from_slice(&resp.payload).map_err(|e| e.to_string())?;

    let id = pi["id"].as_str().ok_or("missing id")?.to_string();
    let status = pi["status"].as_str().ok_or("missing status")?.to_string();
    let amount = pi["amount"].as_i64().ok_or("missing amount")?;
    let currency = pi["currency"].as_str().ok_or("missing currency")?.to_string();
    let created = pi["created"]
        .as_i64()
        .ok_or("missing created")?
        .to_string();

    let _ = logging::info(&alloc::format!(
        "check-order[stripe]: {id} status={status} amount={amount} {currency}"
    ));

    Ok(OrderStatus {
        id,
        status,
        amount,
        currency,
        created,
    })
}

#[cfg(target_arch = "wasm32")]
fn check_order_paystack(reference: &str) -> Result<OrderStatus, String> {
    let api_key = crate::provider::get_secret(Provider::Paystack)?;

    let resp = http_iface::call(&http_iface::Request {
        method: http_iface::Verb::Get,
        url: alloc::format!("{PAYSTACK_BASE}/transaction/verify/{reference}"),
        headers: Some(bearer_header(&api_key)),
        payload: None,
    })
    .map_err(|e| alloc::format!("paystack transaction verify: {e}"))?;

    if resp.code != 200 {
        let body = alloc::string::String::from_utf8_lossy(&resp.payload);
        return Err(alloc::format!(
            "Paystack transaction verify failed: HTTP {} — {body}",
            resp.code
        ));
    }

    let wrapper: serde_json::Value =
        serde_json::from_slice(&resp.payload).map_err(|e| e.to_string())?;
    let data = &wrapper["data"];

    let id = data["id"]
        .as_i64()
        .map(|n| n.to_string())
        .or_else(|| data["id"].as_str().map(|s| s.to_string()))
        .ok_or("missing data.id")?;
    let status = data["status"].as_str().ok_or("missing data.status")?.to_string();
    let amount = data["amount"].as_i64().ok_or("missing data.amount")?;
    let currency = data["currency"]
        .as_str()
        .ok_or("missing data.currency")?
        .to_string();
    let created = data["created_at"].as_str().unwrap_or_default().to_string();

    let _ = logging::info(&alloc::format!(
        "check-order[paystack]: {id} status={status} amount={amount} {currency}"
    ));

    Ok(OrderStatus {
        id,
        status,
        amount,
        currency,
        created,
    })
}

#[cfg(target_arch = "wasm32")]
fn bearer_header(api_key: &str) -> alloc::vec::Vec<(alloc::string::String, alloc::string::String)> {
    alloc::vec![(
        "Authorization".to_string(),
        alloc::format!("Bearer {api_key}"),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_order_non_wasm_returns_err() {
        let input = serde_json::to_vec(&serde_json::json!({
            "order_ref": "pi_abc123",
        }))
        .unwrap();
        let result = check_order(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }

    #[test]
    fn check_order_bad_input_returns_err() {
        let result = check_order(b"not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad input"));
    }

    #[test]
    fn check_order_accepts_legacy_stripe_field_name() {
        let input = serde_json::to_vec(&serde_json::json!({
            "payment_intent_id": "pi_abc123",
        }))
        .unwrap();
        // Still routes to the (unimplemented-off-wasm) provider path rather
        // than failing input parsing — proves the `alias` keeps the old
        // field name working.
        let result = check_order(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }

    #[test]
    fn check_order_accepts_paystack_reference_field_and_provider() {
        let input = serde_json::to_vec(&serde_json::json!({
            "reference": "T123456",
            "provider": "paystack",
        }))
        .unwrap();
        let result = check_order(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }
}

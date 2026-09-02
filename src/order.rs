//! check_order: looks up the payment/order record behind a transaction.
//!
//! Reference build treats a Stripe PaymentIntent as the order record (many
//! restaurant POS stacks process card payments through Stripe under the
//! hood). Swap `STRIPE_BASE` + the request/response shapes for your own POS
//! provider's order-lookup endpoint in a production deployment — the
//! contract, WIT interface, and secret-handling pattern stay the same.

#[derive(serde::Deserialize)]
pub struct CheckOrderReq {
    pub payment_intent_id: String,
}

#[derive(serde::Serialize)]
pub struct OrderStatus {
    pub id: String,
    pub status: String,
    pub amount: i64,
    pub currency: String,
    pub created: i64,
}

const STRIPE_BASE: &str = "https://api.stripe.com/v1";

/// Entry point called from `lib.rs`. `input` is the raw JSON bytes from the
/// node's `generic-input.input` field.
pub fn check_order(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: CheckOrderReq = serde_json::from_slice(input)
        .map_err(|e| alloc::format!("check-order: bad input: {e}"))?;

    #[cfg(target_arch = "wasm32")]
    {
        let resp = check_order_wasm(req)?;
        serde_json::to_vec(&resp).map_err(|e| e.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = req;
        Err("check_order is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::{
    interfaces::{http as http_iface, kv_store, logging},
    tenant::tenant_context,
};

#[cfg(target_arch = "wasm32")]
fn check_order_wasm(req: CheckOrderReq) -> Result<OrderStatus, String> {
    let api_key = get_api_key()?;

    let resp = http_iface::call(&http_iface::Request {
        method: http_iface::Verb::Get,
        url: alloc::format!("{STRIPE_BASE}/payment_intents/{}", req.payment_intent_id),
        headers: Some(stripe_headers(&api_key)),
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
    let created = pi["created"].as_i64().ok_or("missing created")?;

    let _ = logging::info(&alloc::format!(
        "check-order: {id} status={status} amount={amount} {currency}"
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
fn get_api_key() -> Result<alloc::string::String, alloc::string::String> {
    let tid = tenant_context::tenant_did();
    let map_name = alloc::format!("z:{}:secrets", hex::encode(&tid));
    let bytes = kv_store::get(&map_name, b"stripe_secret_key")
        .map_err(|e| alloc::format!("kv read: {e}"))?
        .ok_or("stripe_secret_key not found in z:<tid>:secrets — populate it via the tenant SDK before use")?;
    alloc::string::String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn stripe_headers(
    api_key: &str,
) -> alloc::vec::Vec<(alloc::string::String, alloc::string::String)> {
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
            "payment_intent_id": "pi_abc123",
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
}

//! get_payment_dispute: looks up the current state of a chargeback.
//!
//! Reference build reads a Stripe Dispute object. No customer PII is
//! required to look up a dispute by id, and none is sent in this call.

#[derive(serde::Deserialize)]
pub struct GetDisputeReq {
    pub dispute_id: String,
}

#[derive(serde::Serialize)]
pub struct DisputeStatus {
    pub id: String,
    pub status: String,
    pub reason: String,
    pub amount: i64,
    pub currency: String,
    pub evidence_due_by: alloc::string::String,
}

const STRIPE_BASE: &str = "https://api.stripe.com/v1";

/// Entry point called from `lib.rs`. `input` is the raw JSON bytes from the
/// node's `generic-input.input` field.
pub fn get_payment_dispute(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: GetDisputeReq = serde_json::from_slice(input)
        .map_err(|e| alloc::format!("get-payment-dispute: bad input: {e}"))?;

    #[cfg(target_arch = "wasm32")]
    {
        let resp = get_payment_dispute_wasm(req)?;
        serde_json::to_vec(&resp).map_err(|e| e.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = req;
        Err("get_payment_dispute is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::{
    interfaces::{http as http_iface, kv_store, logging},
    tenant::tenant_context,
};

#[cfg(target_arch = "wasm32")]
fn get_payment_dispute_wasm(req: GetDisputeReq) -> Result<DisputeStatus, String> {
    let api_key = get_api_key()?;

    let resp = http_iface::call(&http_iface::Request {
        method: http_iface::Verb::Get,
        url: alloc::format!("{STRIPE_BASE}/disputes/{}", req.dispute_id),
        headers: Some(stripe_headers(&api_key)),
        payload: None,
    })
    .map_err(|e| alloc::format!("stripe dispute lookup: {e}"))?;

    if resp.code != 200 {
        let body = alloc::string::String::from_utf8_lossy(&resp.payload);
        return Err(alloc::format!(
            "Stripe dispute lookup failed: HTTP {} — {body}",
            resp.code
        ));
    }

    let d: serde_json::Value =
        serde_json::from_slice(&resp.payload).map_err(|e| e.to_string())?;

    let id = d["id"].as_str().ok_or("missing id")?.to_string();
    let status = d["status"].as_str().ok_or("missing status")?.to_string();
    let reason = d["reason"].as_str().ok_or("missing reason")?.to_string();
    let amount = d["amount"].as_i64().ok_or("missing amount")?;
    let currency = d["currency"].as_str().ok_or("missing currency")?.to_string();
    let evidence_due_by = d["evidence_details"]["due_by"]
        .as_i64()
        .map(|t| t.to_string())
        .unwrap_or_default();

    let _ = logging::info(&alloc::format!(
        "get-payment-dispute: {id} status={status} reason={reason}"
    ));

    Ok(DisputeStatus {
        id,
        status,
        reason,
        amount,
        currency,
        evidence_due_by,
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
    fn get_payment_dispute_non_wasm_returns_err() {
        let input = serde_json::to_vec(&serde_json::json!({
            "dispute_id": "dp_abc123",
        }))
        .unwrap();
        let result = get_payment_dispute(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }

    #[test]
    fn get_payment_dispute_bad_input_returns_err() {
        let result = get_payment_dispute(b"not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad input"));
    }
}

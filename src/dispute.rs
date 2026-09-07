//! get_payment_dispute: looks up the current state of a chargeback.
//!
//! Supports Stripe and Paystack, selected by the request's `provider` field
//! (defaults to `stripe`). Neither path needs customer PII to look up a
//! dispute by id, and none is sent.
//!
//! The Paystack response's exact field names (`dueAt` etc.) are taken from
//! public docs/search rather than a live call at time of writing — parsed
//! defensively (soft fallbacks, not hard failures) pending validation
//! against a real sandbox dispute.

use crate::provider::Provider;

#[derive(serde::Deserialize)]
pub struct GetDisputeReq {
    pub dispute_id: String,
    #[serde(default)]
    pub provider: Provider,
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
const PAYSTACK_BASE: &str = "https://api.paystack.co";

/// Entry point called from `lib.rs`. `input` is the raw JSON bytes from the
/// node's `generic-input.input` field.
pub fn get_payment_dispute(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: GetDisputeReq = serde_json::from_slice(input)
        .map_err(|e| alloc::format!("get-payment-dispute: bad input: {e}"))?;

    #[cfg(target_arch = "wasm32")]
    {
        let resp = match req.provider {
            Provider::Stripe => get_dispute_stripe(&req.dispute_id)?,
            Provider::Paystack => get_dispute_paystack(&req.dispute_id)?,
        };
        serde_json::to_vec(&resp).map_err(|e| e.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = req;
        Err("get_payment_dispute is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::interfaces::{http as http_iface, logging};

#[cfg(target_arch = "wasm32")]
fn get_dispute_stripe(dispute_id: &str) -> Result<DisputeStatus, String> {
    let api_key = crate::provider::get_secret(Provider::Stripe)?;

    let resp = http_iface::call(&http_iface::Request {
        method: http_iface::Verb::Get,
        url: alloc::format!("{STRIPE_BASE}/disputes/{dispute_id}"),
        headers: Some(bearer_header(&api_key)),
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
        "get-payment-dispute[stripe]: {id} status={status} reason={reason}"
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
fn get_dispute_paystack(dispute_id: &str) -> Result<DisputeStatus, String> {
    let api_key = crate::provider::get_secret(Provider::Paystack)?;

    let resp = http_iface::call(&http_iface::Request {
        method: http_iface::Verb::Get,
        url: alloc::format!("{PAYSTACK_BASE}/dispute/{dispute_id}"),
        headers: Some(bearer_header(&api_key)),
        payload: None,
    })
    .map_err(|e| alloc::format!("paystack dispute lookup: {e}"))?;

    if resp.code != 200 {
        let body = alloc::string::String::from_utf8_lossy(&resp.payload);
        return Err(alloc::format!(
            "Paystack dispute lookup failed: HTTP {} — {body}",
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
    // Best-effort: Paystack's category/reason field name is not confirmed
    // against a live call yet — fall back to empty rather than hard-fail.
    let reason = data["category"].as_str().unwrap_or_default().to_string();
    let amount = data["amount"].as_i64().unwrap_or(0);
    let currency = data["currency"].as_str().unwrap_or_default().to_string();
    let evidence_due_by = data["dueAt"].as_str().unwrap_or_default().to_string();

    let _ = logging::info(&alloc::format!(
        "get-payment-dispute[paystack]: {id} status={status}"
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

    #[test]
    fn get_payment_dispute_accepts_paystack_provider() {
        let input = serde_json::to_vec(&serde_json::json!({
            "dispute_id": "1",
            "provider": "paystack",
        }))
        .unwrap();
        let result = get_payment_dispute(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }
}

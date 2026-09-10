//! get_payment_dispute: looks up the current state of a chargeback.
//!
//! Supports Stripe and Paystack, selected by the request's `provider` field
//! (defaults to `stripe`). Neither path needs customer PII to look up a
//! dispute by id, and none is sent.
//!
//! The Paystack response fields are verified against Paystack's published
//! OpenAPI spec (github.com/PaystackOSS/openapi, `dist/paystack.yaml`,
//! `DisputeFetchResponse` schema), not a live call, but the actual
//! contract Paystack publishes, not a guess. One field (`dueAt`) is typed
//! only as `nullable: true` in that spec with no explicit type, so its
//! string-ness is inferred by analogy with sibling fields and parsed
//! defensively rather than hard-failed.

use crate::provider::Provider;

#[derive(serde::Deserialize)]
pub struct GetDisputeReq {
    pub dispute_id: String,
    #[serde(default)]
    pub provider: Provider,
}

#[derive(Debug, serde::Serialize)]
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

/// Parses a Stripe Dispute response into `DisputeStatus`. Pure function.
/// See the fixture-based tests below, one against a real captured live
/// response.
fn parse_stripe_dispute_response(d: &serde_json::Value) -> Result<DisputeStatus, String> {
    let id = d["id"].as_str().ok_or("missing id")?.to_string();
    let status = d["status"].as_str().ok_or("missing status")?.to_string();
    let reason = d["reason"].as_str().ok_or("missing reason")?.to_string();
    let amount = d["amount"].as_i64().ok_or("missing amount")?;
    let currency = d["currency"].as_str().ok_or("missing currency")?.to_string();
    let evidence_due_by = d["evidence_details"]["due_by"]
        .as_i64()
        .map(|t| t.to_string())
        .unwrap_or_default();

    Ok(DisputeStatus {
        id,
        status,
        reason,
        amount,
        currency,
        evidence_due_by,
    })
}

/// Parses a Paystack Dispute response into `DisputeStatus`. Pure function.
/// See the fixture-based test below, built from Paystack's verified
/// `DisputeFetchResponse` OpenAPI schema (not a live call, Paystack has no
/// self-serve way to create a test dispute; see README "Testing a dispute
/// end-to-end").
fn parse_paystack_dispute_response(wrapper: &serde_json::Value) -> Result<DisputeStatus, String> {
    let data = &wrapper["data"];

    let id = data["id"]
        .as_i64()
        .map(|n| n.to_string())
        .or_else(|| data["id"].as_str().map(|s| s.to_string()))
        .ok_or("missing data.id")?;
    let status = data["status"].as_str().ok_or("missing data.status")?.to_string();
    let reason = data["category"].as_str().ok_or("missing data.category")?.to_string();
    // The Dispute object has no top-level `amount` field (confirmed against
    // Paystack's published OpenAPI spec), only `refund_amount`, which is a
    // resolution-time figure, and the original disputed amount nested at
    // `data.transaction.amount`. The latter is the correct match for
    // Stripe's `dispute.amount` semantics ("the amount in question"), so
    // that's what this reads.
    let amount = data["transaction"]["amount"]
        .as_i64()
        .ok_or("missing data.transaction.amount")?;
    let currency = data["currency"].as_str().ok_or("missing data.currency")?.to_string();
    // `dueAt` is confirmed as the real field name (Paystack OpenAPI spec),
    // but the spec declares it only as `nullable: true` with no explicit
    // type, inferred as an ISO-8601 string by analogy with the sibling
    // `createdAt`/`updatedAt`/`resolvedAt` fields, which the spec does type
    // as strings. Soft fallback here, not a hard failure, since that
    // inference isn't 100% certain without a live response to check.
    let evidence_due_by = data["dueAt"].as_str().unwrap_or_default().to_string();

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
            "Stripe dispute lookup failed: HTTP {}: {body}",
            resp.code
        ));
    }

    let d: serde_json::Value =
        serde_json::from_slice(&resp.payload).map_err(|e| e.to_string())?;
    let result = parse_stripe_dispute_response(&d)?;

    let _ = logging::info(&alloc::format!(
        "get-payment-dispute[stripe]: {} status={} reason={}",
        result.id, result.status, result.reason
    ));

    Ok(result)
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
            "Paystack dispute lookup failed: HTTP {}: {body}",
            resp.code
        ));
    }

    let wrapper: serde_json::Value =
        serde_json::from_slice(&resp.payload).map_err(|e| e.to_string())?;
    let result = parse_paystack_dispute_response(&wrapper)?;

    let _ = logging::info(&alloc::format!(
        "get-payment-dispute[paystack]: {} status={}",
        result.id, result.status
    ));

    Ok(result)
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

    /// Real response captured from a live `get-payment-dispute` call
    /// against Stripe test mode during this contract's development
    /// (dispute `du_1UD7wGLIEmw77WfU9BIC0xDT`, created via
    /// `pm_card_createDispute`), not a hand-typed guess at the shape.
    #[test]
    fn parse_stripe_dispute_response_matches_live_capture() {
        let fixture = serde_json::json!({
            "id": "du_1UD7wGLIEmw77WfU9BIC0xDT",
            "object": "dispute",
            "status": "needs_response",
            "reason": "fraudulent",
            "amount": 500,
            "currency": "usd",
            "evidence_details": { "due_by": 1789516799, "has_evidence": false }
        });
        let result = parse_stripe_dispute_response(&fixture).unwrap();
        assert_eq!(result.id, "du_1UD7wGLIEmw77WfU9BIC0xDT");
        assert_eq!(result.status, "needs_response");
        assert_eq!(result.reason, "fraudulent");
        assert_eq!(result.amount, 500);
        assert_eq!(result.currency, "usd");
        assert_eq!(result.evidence_due_by, "1789516799");
    }

    #[test]
    fn parse_stripe_dispute_response_missing_field_errs() {
        let fixture = serde_json::json!({ "id": "du_x", "status": "needs_response" });
        let result = parse_stripe_dispute_response(&fixture);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing reason"));
    }

    /// Fixture built from Paystack's verified `DisputeFetchResponse`
    /// OpenAPI schema (github.com/PaystackOSS/openapi,
    /// `dist/paystack.yaml`), field names and nesting are confirmed
    /// against the published spec, not guessed. Not a live capture:
    /// Paystack has no self-serve way to create a test dispute (see
    /// README "Testing a dispute end-to-end"), so this is the strongest
    /// verification available without one.
    #[test]
    fn parse_paystack_dispute_response_matches_verified_schema() {
        let fixture = serde_json::json!({
            "status": true,
            "message": "Dispute retrieved",
            "data": {
                "id": 4734583785_i64,
                "refund_amount": 500000,
                "currency": "NGN",
                "status": "awaiting-merchant-feedback",
                "resolution": null,
                "domain": "test",
                "transaction": {
                    "id": 1983692332,
                    "reference": "rrsgjyd5yv",
                    "amount": 500000,
                    "currency": "NGN"
                },
                "transaction_reference": null,
                "category": "chargeback",
                "customer": {
                    "id": 1,
                    "first_name": "Test",
                    "last_name": "Customer",
                    "email": "customer@example.com",
                    "customer_code": "CUS_xxx",
                    "phone": "+2348012345678",
                    "metadata": {},
                    "risk_action": "default",
                    "international_format_phone": "+2348012345678"
                },
                "bin": "408408",
                "last4": "4081",
                "dueAt": "2026-09-20T00:00:00.000Z",
                "resolvedAt": null,
                "evidence": null,
                "attachments": null,
                "note": null,
                "history": [],
                "messages": [],
                "createdAt": "2026-09-06T00:00:00.000Z",
                "updatedAt": "2026-09-06T00:00:00.000Z"
            }
        });
        let result = parse_paystack_dispute_response(&fixture).unwrap();
        assert_eq!(result.id, "4734583785");
        assert_eq!(result.status, "awaiting-merchant-feedback");
        assert_eq!(result.reason, "chargeback");
        assert_eq!(result.amount, 500000);
        assert_eq!(result.currency, "NGN");
        assert_eq!(result.evidence_due_by, "2026-09-20T00:00:00.000Z");
    }

    #[test]
    fn parse_paystack_dispute_response_missing_category_errs() {
        let fixture = serde_json::json!({
            "status": true,
            "data": { "id": 1, "status": "pending" }
        });
        let result = parse_paystack_dispute_response(&fixture);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing data.category"));
    }

    /// Regression test for the real bug found and fixed during this
    /// contract's development: an earlier version read `data.amount`,
    /// which does not exist on Paystack's Dispute object. This locks in
    /// that the correct field (`data.transaction.amount`) is what gets
    /// read, and that a dispute missing the nested transaction amount
    /// errors instead of silently returning 0.
    #[test]
    fn parse_paystack_dispute_response_rejects_top_level_amount() {
        let fixture = serde_json::json!({
            "status": true,
            "data": {
                "id": 1,
                "status": "pending",
                "category": "chargeback",
                "currency": "NGN",
                "amount": 999999, // must NOT be read, not a real field on this object
                "transaction": {}
            }
        });
        let result = parse_paystack_dispute_response(&fixture);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing data.transaction.amount"));
    }
}

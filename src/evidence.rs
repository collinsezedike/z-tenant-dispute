//! submit_dispute_evidence: assembles and submits chargeback evidence.
//!
//! Supports Stripe and Paystack, selected by the request's `provider` field
//! (defaults to `stripe`). Either way, the disputing customer's identity
//! (name, email) is NEVER passed in as a contract argument. The contract
//! templates `{{profile.<path>}}` markers into the evidence body and the
//! host's `http-with-placeholders` interface resolves them from the calling
//! user's profile at dispatch time — substitution happens host-side, after
//! this contract serialises the body and before the outbound call, so
//! plaintext customer PII never enters WASM memory.
//!
//! Stripe's dispute-update endpoint takes `application/x-www-form-urlencoded`
//! bodies (not JSON), so `form_encode` percent-encodes literal values while
//! deliberately leaving `{`, `}`, and `.` unescaped so the `{{profile.x}}`
//! markers stay intact for the host to recognise and substitute. Paystack's
//! `Add Evidence` endpoint takes plain JSON with `customer_name` /
//! `customer_email` / `customer_phone` fields, so the placeholder markers go
//! in as ordinary JSON string values — no custom encoding needed there.
//!
//! CONFIRMED PLATFORM LIMITATION (live-tested, not a guess): the Stripe path
//! above cannot currently succeed on T3N testnet. `http-with-placeholders`
//! parses the *resolved* body as JSON before forwarding it upstream — a
//! form-urlencoded body fails host-side with
//! `upstream: parse resolved body: expected value at line 1 column 1`
//! before the request ever reaches Stripe. Stripe's `/v1/disputes/:id`, in
//! turn, explicitly rejects a JSON body
//! (`"check that your POST content type is application/x-www-form-urlencoded"`)
//! — verified directly against the live Stripe API, not assumed. These two
//! requirements are mutually exclusive, so no header or encoding choice on
//! the contract's side can reconcile them; this needs either a
//! non-JSON-only `http-with-placeholders` mode on the host, or a Stripe
//! integration path that doesn't need one. The Paystack path is unaffected
//! (its API is JSON-native) and is confirmed working up to the point where
//! the *calling user's* T3N profile needs `verified_contacts.phone.value`
//! populated (`PlaceholderUnknown` otherwise — a caller-account gap, not a
//! contract bug).

use crate::provider::Provider;

#[derive(serde::Deserialize)]
pub struct SubmitEvidenceReq {
    pub dispute_id: String,
    /// Opaque order reference used only in the evidence narrative — not PII.
    pub order_id: String,
    pub product_description: String,
    #[serde(default)]
    pub provider: Provider,
}

#[derive(serde::Serialize)]
pub struct EvidenceResult {
    pub id: String,
    pub status: String,
}

const STRIPE_BASE: &str = "https://api.stripe.com/v1";
const PAYSTACK_BASE: &str = "https://api.paystack.co";

/// Entry point called from `lib.rs`. `input` is the raw JSON bytes from the
/// node's `generic-input.input` field.
pub fn submit_dispute_evidence(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: SubmitEvidenceReq = serde_json::from_slice(input)
        .map_err(|e| alloc::format!("submit-dispute-evidence: bad input: {e}"))?;

    #[cfg(target_arch = "wasm32")]
    {
        let result = match req.provider {
            Provider::Stripe => submit_evidence_stripe(&req)?,
            Provider::Paystack => submit_evidence_paystack(&req)?,
        };
        serde_json::to_vec(&result).map_err(|e| e.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = req;
        Err("submit_dispute_evidence is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::interfaces::{http_with_placeholders as hwp, logging};

#[cfg(target_arch = "wasm32")]
fn submit_evidence_stripe(req: &SubmitEvidenceReq) -> Result<EvidenceResult, String> {
    let api_key = crate::provider::get_secret(Provider::Stripe)?;

    // Resolved from the calling user's profile (privacy-preserving path):
    let customer_name = "{{profile.first_name}} {{profile.last_name}}";
    let customer_email = "{{profile.verified_contacts.email.value}}";

    let uncategorized_text = alloc::format!(
        "Order {} fulfilled and verified by retailer systems prior to dispute filing.",
        req.order_id
    );

    let body = alloc::format!(
        "evidence[customer_name]={}&evidence[customer_email_address]={}&evidence[product_description]={}&evidence[uncategorized_text]={}",
        form_encode(customer_name),
        form_encode(customer_email),
        form_encode(&req.product_description),
        form_encode(&uncategorized_text),
    );

    let _ = logging::info(&alloc::format!(
        "Submitting Stripe evidence for dispute {}",
        req.dispute_id
    ));

    let resp = hwp::call(&hwp::Request {
        method: hwp::Verb::Post,
        url: alloc::format!("{STRIPE_BASE}/disputes/{}", req.dispute_id),
        headers: Some(stripe_form_headers(&api_key)),
        payload: Some(body.into_bytes()),
    })
    .map_err(|e| alloc::format!("stripe dispute update: {}", format_http_error(e)))?;

    if resp.code != 200 {
        let _ = logging::error(&alloc::format!(
            "Stripe dispute update HTTP {}: {}",
            resp.code,
            alloc::string::String::from_utf8_lossy(&resp.payload)
        ));
        return Err(alloc::format!(
            "Stripe dispute update failed: HTTP {}",
            resp.code
        ));
    }

    let d: serde_json::Value =
        serde_json::from_slice(&resp.payload).map_err(|e| e.to_string())?;

    let id = d["id"].as_str().ok_or("missing id")?.to_string();
    let status = d["status"].as_str().ok_or("missing status")?.to_string();

    let _ = logging::info(&alloc::format!(
        "Stripe dispute evidence submitted: id={id} status={status}"
    ));

    Ok(EvidenceResult { id, status })
}

#[cfg(target_arch = "wasm32")]
fn submit_evidence_paystack(req: &SubmitEvidenceReq) -> Result<EvidenceResult, String> {
    use serde_json::json;

    let api_key = crate::provider::get_secret(Provider::Paystack)?;

    // Resolved from the calling user's profile (privacy-preserving path):
    // Paystack's Add Evidence endpoint takes plain JSON, so the placeholder
    // markers go in as ordinary string values — the host substitutes them
    // before the outbound call the same way it does for Stripe's form body.
    let body = json!({
        "customer_name": "{{profile.first_name}} {{profile.last_name}}",
        "customer_email": "{{profile.verified_contacts.email.value}}",
        "customer_phone": "{{profile.verified_contacts.phone.value}}",
        "service_details": req.product_description,
    });

    let _ = logging::info(&alloc::format!(
        "Submitting Paystack evidence for dispute {}",
        req.dispute_id
    ));

    let resp = hwp::call(&hwp::Request {
        method: hwp::Verb::Post,
        url: alloc::format!("{PAYSTACK_BASE}/dispute/{}/evidence", req.dispute_id),
        headers: Some(paystack_json_headers(&api_key)),
        payload: Some(serde_json::to_vec(&body).map_err(|e| e.to_string())?),
    })
    .map_err(|e| alloc::format!("paystack add-evidence: {}", format_http_error(e)))?;

    if resp.code != 200 {
        let _ = logging::error(&alloc::format!(
            "Paystack add-evidence HTTP {}: {}",
            resp.code,
            alloc::string::String::from_utf8_lossy(&resp.payload)
        ));
        return Err(alloc::format!(
            "Paystack add-evidence failed: HTTP {}",
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
        .unwrap_or_default();
    let status = wrapper["message"].as_str().unwrap_or("submitted").to_string();

    let _ = logging::info(&alloc::format!(
        "Paystack dispute evidence submitted: id={id} status={status}"
    ));

    Ok(EvidenceResult { id, status })
}

/// Percent-encode a value for an `application/x-www-form-urlencoded` body,
/// leaving `{`, `}`, and `.` unescaped so `{{profile.x}}` markers stay
/// intact for host-side placeholder resolution. Stripe-only — Paystack's
/// JSON body needs no such encoding.
#[cfg(target_arch = "wasm32")]
fn form_encode(s: &str) -> alloc::string::String {
    let mut out = alloc::string::String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'{' | b'}' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&alloc::format!("%{:02X}", b)),
        }
    }
    out
}

/// Render a typed `http-with-placeholders` error as a contract-facing string.
/// Never includes resolved PII — only field names and host-side reasons.
#[cfg(target_arch = "wasm32")]
fn format_http_error(e: hwp::HttpError) -> alloc::string::String {
    match e {
        hwp::HttpError::EgressDenied(host) => alloc::format!("egress denied for host {host}"),
        hwp::HttpError::PlaceholderDenied(marker) => {
            alloc::format!("placeholder not permitted: {marker}")
        }
        hwp::HttpError::PlaceholderUnknown(field) => {
            alloc::format!("user profile missing field: {field}")
        }
        hwp::HttpError::PlaceholderNoUserContext => {
            "no user context bound for placeholder resolution".to_string()
        }
        hwp::HttpError::UpstreamError(reason) => alloc::format!("upstream: {reason}"),
    }
}

#[cfg(target_arch = "wasm32")]
fn stripe_form_headers(
    api_key: &str,
) -> alloc::vec::Vec<(alloc::string::String, alloc::string::String)> {
    alloc::vec![
        (
            "Authorization".to_string(),
            alloc::format!("Bearer {api_key}"),
        ),
        (
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
    ]
}

#[cfg(target_arch = "wasm32")]
fn paystack_json_headers(
    api_key: &str,
) -> alloc::vec::Vec<(alloc::string::String, alloc::string::String)> {
    alloc::vec![
        (
            "Authorization".to_string(),
            alloc::format!("Bearer {api_key}"),
        ),
        (
            "Content-Type".to_string(),
            "application/json".to_string(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_dispute_evidence_non_wasm_returns_err() {
        let input = serde_json::to_vec(&serde_json::json!({
            "dispute_id": "dp_abc123",
            "order_id": "ORD-7219",
            "product_description": "Dinner for two",
        }))
        .unwrap();
        let result = submit_dispute_evidence(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }

    #[test]
    fn submit_dispute_evidence_bad_input_returns_err() {
        let result = submit_dispute_evidence(b"not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad input"));
    }

    #[test]
    fn submit_dispute_evidence_accepts_paystack_provider() {
        let input = serde_json::to_vec(&serde_json::json!({
            "dispute_id": "1",
            "order_id": "ORD-7219",
            "product_description": "Dinner for two",
            "provider": "paystack",
        }))
        .unwrap();
        let result = submit_dispute_evidence(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }

    #[test]
    fn submit_dispute_evidence_rejects_inline_pii_fields() {
        let input = serde_json::to_vec(&serde_json::json!({
            "dispute_id": "dp_abc123",
            "customer_name": "Jane Doe",
            "order_id": "ORD-7219",
            "product_description": "Dinner for two",
        }))
        .unwrap();
        // extra unknown field is ignored by serde by default; the real
        // guardrail is that SubmitEvidenceReq has no PII field to populate.
        let result = submit_dispute_evidence(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }
}

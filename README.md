# z-tenant-dispute

A B2B restaurant chargeback & dispute agent for the Terminal 3 Network (T3N)
ADK Agent Build Challenge. Runs as a Rust contract compiled to a WASM
component inside T3N's trusted execution environment (TEE).

## What it does

A restaurant group gives the agent a disputed transaction. The agent:

1. **`check-order`** — looks up the underlying payment/order status (no PII
   involved; a plain synchronous HTTP call).
2. **`get-payment-dispute`** — looks up the chargeback's status, reason code,
   and evidence deadline (no PII; plain synchronous HTTP call).
3. **`submit-dispute-evidence`** — assembles and submits dispute evidence to
   the payment processor. This is the only function that touches the
   disputing customer's identity, and it never receives that PII as a
   contract argument: the contract templates `{{profile.first_name}}` /
   `{{profile.verified_contacts.email.value}}` markers into the evidence
   body, and the host's `http-with-placeholders` interface resolves them
   from the calling user's profile at dispatch time. Plaintext customer PII
   never enters WASM memory.

## Why this needs a TEE

A restaurant group's payment-processor credentials and its customers'
identities are exactly the combination an enterprise security team cares
about. Isolating both — secrets that never sit in plaintext config, and PII
that never crosses into contract memory — turns "trust us" into an
architectural guarantee instead of a policy.

## Reference build: Stripe test mode

This reference implementation targets the Stripe API in test mode:

- `check-order` reads a Stripe **PaymentIntent** (many restaurant POS stacks
  process card payments through Stripe under the hood).
- `get-payment-dispute` reads a Stripe **Dispute**.
- `submit-dispute-evidence` updates a Stripe **Dispute**'s evidence via its
  form-urlencoded update endpoint.

To point this at a different POS/payment stack in production, swap the
`STRIPE_BASE` constant and the request/response shapes in `src/order.rs` and
`src/dispute.rs` — the WIT interface, secret-handling pattern, and PII
placeholder mechanism stay the same.

## Setup

Before first use, the tenant SDK must create the `secrets` KV map and write
a Stripe **test-mode** secret key:

```text
z_sdk.kv("secrets").set("stripe_secret_key", "sk_test_your_key_here")
```

## Host capabilities required

```json
{
  "host_capabilities": [
    "kv_store", "logging", "tenant_context", "http", "http_with_placeholders"
  ]
}
```

## Build

```bash
cargo build --release
```

Targets `wasm32-wasip2` (see `.cargo/config.toml`), producing a WASM
component per `crate-type = ["cdylib", "lib"]`.

## Testing a dispute end-to-end (Stripe test mode)

Stripe's test mode supports triggering a synthetic dispute using a special
test card number (`4000000000000259`) when creating a PaymentIntent — the
dispute appears on the PaymentIntent shortly after the charge succeeds. See
Stripe's testing docs for the current list of dispute-triggering test cards.

## Known issue filed against T3N

While setting up this project, the ADK quickstart's `fetchTrustedManifest("testnet")`
consistently failed with `Trust manifest ... is malformed` on SDK versions
5.4.0 and 5.5.0, even though the raw manifest endpoint
(`https://cn-api.sg.testnet.t3n.terminal3.io/api/trust-manifest`) returns a
well-formed 200 JSON response when queried directly with `curl`. Filed as a
bug per the challenge's submission requirements.

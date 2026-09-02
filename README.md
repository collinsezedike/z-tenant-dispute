# z-tenant-dispute

A B2B e-commerce chargeback & dispute agent for the Terminal 3 Network (T3N)
ADK Agent Build Challenge. Runs as a Rust contract compiled to a WASM
component inside T3N's trusted execution environment (TEE).

## What it does

An online retailer's payments/ops team gives the agent a disputed
transaction. The agent:

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

E-commerce chargeback volume is high enough that most retailers run a
dedicated ops function just to fight them, and doing so means handing an
automation system both payment-processor credentials and disputing
customers' identities — exactly the combination an enterprise security team
cares about. Isolating both — secrets that never sit in plaintext config,
and PII that never crosses into contract memory — turns "trust us" into an
architectural guarantee instead of a policy.

## Reference build: Stripe test mode

This reference implementation targets the Stripe API in test mode:

- `check-order` reads a Stripe **PaymentIntent** representing the order.
- `get-payment-dispute` reads a Stripe **Dispute**.
- `submit-dispute-evidence` updates a Stripe **Dispute**'s evidence via its
  form-urlencoded update endpoint.

To point this at a different order/payment stack in production, swap the
`STRIPE_BASE` constant and the request/response shapes in `src/order.rs` and
`src/dispute.rs` — the WIT interface, secret-handling pattern, and PII
placeholder mechanism stay the same.

## Setup

Before first use, the tenant SDK must write a Stripe **test-mode** secret
key into the tenant's `secrets` KV map — see `driver/` below for the actual
script pattern.

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

## Driver scripts (`driver/`)

Node/TypeScript scripts that drive the ADK auth flow and registration —
what actually produced the deployment status below:

- `driver/quickstart.ts` — authenticates against T3N testnet and prints the
  resulting `tenantDid`.
- `driver/register.ts` — builds on the same auth flow, reads the compiled
  `../target/wasm32-wasip2/release/z_tenant_dispute.wasm`, and registers it
  via `tenant.contracts.register()`.
- `driver/verify.ts` — lists the calling tenant's registered contracts via
  `tenant.contracts.listDetailed()`, to confirm a registration went through.

```bash
cd driver
pnpm install
cp .env.example .env   # fill in T3N_API_KEY from the ADK claim page
npx tsx quickstart.ts
npx tsx register.ts
npx tsx verify.ts
```

`package.json` pins `@terminal3/t3n-sdk` to exactly `5.2.0` — see "Known
issue" below for why that pin is load-bearing, not incidental.

Not yet exercised in this environment: seeding `stripe_secret_key` into the
tenant's `secrets` KV map and invoking the registered contract's three
functions against live Stripe test data (needs a Stripe test-mode account,
not obtained during this build). Seeding follows the pattern documented in
the ADK docs:

```typescript
await tenant.executeControl("map-entry-set", {
  map_name: tenant.canonicalName("secrets"),
  key:      "stripe_secret_key",
  value:    process.env.STRIPE_SECRET_KEY!,
});
```

## Testing a dispute end-to-end (Stripe test mode)

Stripe's test mode supports triggering a synthetic dispute using a special
test card number (`4000000000000259`) when creating a PaymentIntent — the
dispute appears on the PaymentIntent shortly after the charge succeeds. See
Stripe's testing docs for the current list of dispute-triggering test cards.

## Deployment status

Registered and live on T3N testnet:

```json
{
  "name": "z:cbdb56e651c06e2c34a40a87f27490a6faa05826:dispute-contracts",
  "short_name": "dispute-contracts",
  "version": "0.1.0",
  "status": "active"
}
```

Confirmed via `tenant.contracts.listDetailed()` after registering through
`tenant.contracts.register()`.

## Known issue filed against T3N: `fetchTrustedManifest` regression

The ADK quickstart's `fetchTrustedManifest("testnet")` fails with
`Trust manifest ... is malformed` on every published SDK version from
`5.3.0` through the current latest (`5.7.0` at the time of writing) —
including the versions the quickstart docs currently have you install fresh.

**Root cause**, confirmed by resolving the SDK's own minified string table
directly (not guesswork): `isSignedTrustManifest()` — the shape check
`fetchTrustedManifest` runs before signature verification — requires the
manifest response to include an `rtmr1_allowlist: string[]` field. The live
testnet endpoint (`https://cn-api.sg.testnet.t3n.terminal3.io/api/trust-manifest`)
returns a well-formed, signed 200 JSON response, but it has never emitted
`rtmr1_allowlist` — only `cluster`, `version`, `peer_ids`, `rtmr3_allowlist`,
`signed_at`, and `signature`. That field requirement was added in SDK
`5.3.0`; every version before it (`5.0.0`–`5.2.0`) validates the exact same
live manifest successfully. There is no safe client-side workaround: the
manifest's `signature` is a server-side hash over the canonical payload
minus `signature` itself, so patching in a synthetic `rtmr1_allowlist`
client-side to pass the shape check breaks signature verification instead.

**Workaround used to unblock this submission:** pin
`@terminal3/t3n-sdk` to `5.2.0` (the last version before the regression) in
the tenant-side Node/TypeScript project that drives auth and registration.
The contract itself (this repo) is unaffected — the regression is entirely
in the JS/TS SDK's client-side manifest validator, not in anything the Rust
contract does.

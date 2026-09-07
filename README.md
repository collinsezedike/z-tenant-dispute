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

All three functions accept an optional `provider` field (`"stripe"` or
`"paystack"`, defaulting to `"stripe"`) so a single deployed contract can
serve either payment processor — see "Two providers" below.

## Why this needs a TEE

E-commerce chargeback volume is high enough that most retailers run a
dedicated ops function just to fight them, and doing so means handing an
automation system both payment-processor credentials and disputing
customers' identities — exactly the combination an enterprise security team
cares about. Isolating both — secrets that never sit in plaintext config,
and PII that never crosses into contract memory — turns "trust us" into an
architectural guarantee instead of a policy.

## Two providers: Stripe and Paystack (test mode)

Every function's JSON input carries an optional `provider` field
(`"stripe"` or `"paystack"`, default `"stripe"`) — see `src/provider.rs`.
Both paths ship in this same contract build:

| Function | Stripe | Paystack |
| --- | --- | --- |
| `check-order` | `GET /v1/payment_intents/:id` | `GET /transaction/verify/:reference` |
| `get-payment-dispute` | `GET /v1/disputes/:id` | `GET /dispute/:id` |
| `submit-dispute-evidence` | `POST /v1/disputes/:id` (form-urlencoded) | `POST /dispute/:id/evidence` (JSON) |
| Secret KV key | `stripe_secret_key` | `paystack_secret_key` |

Paystack's `Add Evidence` endpoint takes plain JSON
(`customer_name`/`customer_email`/`customer_phone`), so the PII placeholder
markers go in as ordinary string values — no custom encoding needed there.
Stripe's dispute-update endpoint is form-urlencoded, which is why
`evidence.rs` has a small hand-rolled `form_encode()` that percent-encodes
literal values while leaving `{`, `}`, `.` unescaped so the `{{profile.x}}`
markers survive intact for the host to substitute.

Paystack's response fields are verified against Paystack's published
OpenAPI spec (`github.com/PaystackOSS/openapi`, `dist/paystack.yaml`) rather
than guessed from docs/search — an earlier pass had a real bug here
(`get-payment-dispute` read a top-level `amount` field that doesn't exist
on Paystack's Dispute object; the correct field is `data.transaction.amount`,
found only after pulling the actual schema) — see the code comments in
`src/dispute.rs` for the full trail. One field (`dueAt`) is typed only as
`nullable: true` in the spec with no explicit type, so its string-ness is
inferred by analogy with sibling fields and parsed defensively rather than
hard-failed.

To add a third provider, or point either path at a different order/payment
stack in production, add a `Provider` variant and a matching branch in
`src/order.rs` / `src/dispute.rs` / `src/evidence.rs` — the WIT interface,
secret-handling pattern, and PII placeholder mechanism stay the same
regardless of how many providers are wired in.

## Setup

Before first use, the tenant SDK must write the relevant provider's
**test-mode** secret key into the tenant's `secrets` KV map — see `driver/`
below for the actual script pattern.

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
- `driver/setup-secrets-map.ts` — creates the tenant's `secrets` KV map
  (`tenant.maps.create()`) with readers/writers scoped to the current
  contract id. **Required once before the first `seed-secret.ts` call** —
  `map-entry-set` writes into an existing map, it does not create one, and
  fails with `map not found` otherwise.
- `driver/grant-egress.ts` — sets a self-grant (`t3n.agentAuthUpdate()`) so
  the calling DID may invoke the contract's functions and reach
  `api.paystack.co` / `api.stripe.com`. **Also required once** — without it,
  outbound calls fail with `host/http.egress_denied` even though auth, KV
  read, and dispatch all succeed first.
- `driver/seed-secret.ts` — writes a named secret into the tenant's
  `secrets` KV map (`stripe_secret_key` or `paystack_secret_key`) via
  `tenant.executeControl("map-entry-set", ...)`.
- `driver/invoke.ts` — calls one exported function on the registered
  contract via `tenant.contracts.execute()`, for live end-to-end testing.
- `driver/init-stripe-test-intent.ts` — creates a real Stripe test-mode
  PaymentIntent (test-data setup only, doesn't go through the contract) so
  there's a genuine id to run `check-order` against. Reads the seeded
  `stripe_secret_key` back and uses it only inside a `fetch()` call, never
  passed to a subprocess.

```bash
cd driver
pnpm install
cp .env.example .env   # fill in T3N_API_KEY from the ADK claim page
npx tsx quickstart.ts
npx tsx register.ts
npx tsx verify.ts

# one-time setup (per contract_id — re-run after a version bump):
npx tsx setup-secrets-map.ts 917
npx tsx grant-egress.ts

# seed a secret, then invoke (Paystack example):
npx tsx seed-secret.ts paystack_secret_key sk_test_xxxxxxxx
npx tsx invoke.ts check-order '{"order_ref":"T123456","provider":"paystack"}'
```

`package.json` pins `@terminal3/t3n-sdk` to exactly `5.2.0` — see "Known
issue" below for why that pin is load-bearing, not incidental.

**Security note:** never pass a secret key as a shell/CLI argument to a
subprocess you don't fully control the error handling of — an earlier pass
at a test-data-setup script leaked a raw key into a thrown error message
that included the full command line. `seed-secret.ts` and `invoke.ts` both
take the key via `argv`/`.env` and use it only inside SDK calls, never
inside a shelled-out command.

## Testing a dispute end-to-end

**Stripe test mode** supports triggering a synthetic dispute using a special
test card number (`4000000000000259`) when creating a PaymentIntent — the
dispute appears on the PaymentIntent shortly after the charge succeeds. See
Stripe's testing docs for the current list of dispute-triggering test cards.

**Paystack test mode**: checked (Paystack's OpenAPI spec plus general web
search) and there does not appear to be a publicly documented equivalent to
Stripe's dispute-triggering test cards — Paystack's test-mode docs cover
card/PIN/OTP test values for payment flows but not synthetic dispute
creation. `check-order` (transaction verify) is fully testable against
Paystack's sandbox as-is; exercising `get-payment-dispute` and
`submit-dispute-evidence` end-to-end needs either a real dispute or
whatever manual test-data support Paystack's dashboard/support team can
provide, since there's no self-serve way to fabricate one.

## Deployment status

Registered and live on T3N testnet (v0.2.0, dual-provider):

```json
{
  "name": "z:cbdb56e651c06e2c34a40a87f27490a6faa05826:dispute-contracts",
  "short_name": "dispute-contracts",
  "version": "0.2.0",
  "status": "active"
}
```

Confirmed via `tenant.contracts.listDetailed()` after registering through
`tenant.contracts.register()`. Note: re-registering the same tail with a new
version allocates a new `contract_id` (869 → 917 going from v0.1.0 to
v0.2.0) — the tenant SDK docs call this out explicitly, and it holds in
practice.

**Live-verified**: `check-order` against both providers, invoked end-to-end
through T3N (auth → contract dispatch → KV secret read → real outbound HTTP
→ parsed response back through the WIT boundary).

Paystack, against a real (2022) transaction reference:

```json
{
  "id": "1983692332",
  "status": "success",
  "amount": 500000,
  "currency": "NGN",
  "created": "2022-07-29T23:25:08.000Z"
}
```

Stripe, against a freshly created test-mode PaymentIntent:

```json
{
  "id": "pi_3UD7joLIEmw77WfU1iRrUsJe",
  "status": "requires_payment_method",
  "amount": 5000,
  "currency": "usd",
  "created": "1788807472"
}
```

`get-payment-dispute` against Stripe, using `pm_card_createDispute` (see
"Testing a dispute end-to-end" below) to create a real disputed
PaymentIntent:

```json
{
  "id": "du_1UD7wGLIEmw77WfU9BIC0xDT",
  "status": "needs_response",
  "reason": "fraudulent",
  "amount": 500,
  "currency": "usd",
  "evidence_due_by": "1789516799"
}
```

`submit-dispute-evidence` — **confirmed non-functional on Stripe, for a
platform-level reason, not a contract bug** — see "Known issue #2" below.
Confirmed reaching real placeholder resolution on Paystack (fails only on a
caller-profile gap, `PlaceholderUnknown: verified_contacts.phone.value` —
not a contract or platform bug, just this test tenant's own T3N user
profile not having that field populated).

## Known issues filed against T3N

### Issue #1: `fetchTrustedManifest` regression

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

### Issue #2: `http-with-placeholders` requires a JSON body, incompatible with form-urlencoded upstream APIs

Live-tested, not a guess: a contract using `http-with-placeholders` to call
an upstream API that requires `application/x-www-form-urlencoded` (Stripe's
classic REST API, for one — `/v1/disputes/:id` and much of the rest of
their API) cannot currently work on T3N.

**Root cause:** the host parses the *placeholder-resolved* body as JSON
before forwarding it upstream, regardless of the `Content-Type` header the
contract sets. A form-urlencoded body fails host-side with
`upstream: parse resolved body: expected value at line 1 column 1` before
the request ever reaches the upstream API — confirmed via this repo's own
`submit-dispute-evidence` (Stripe path) failing with exactly that error.
Stripe's API, on the other side, explicitly rejects a JSON body for this
endpoint (`"check that your POST content type is application/x-www-form-urlencoded"`
— verified directly against the live Stripe API, not assumed). The two
requirements are mutually exclusive; no encoding choice on the contract's
side can reconcile them.

**Impact:** any T3N contract that needs to send PII-bearing data to a
form-urlencoded-only upstream API cannot use `http-with-placeholders` for
that call today. This isn't specific to Stripe or to disputes — it affects
every upstream API on Stripe's classic REST surface, and presumably any
other provider whose API predates JSON-body support.

**Suggested fix on T3N's end:** either accept non-JSON bodies through
`http-with-placeholders` (skip the parse-and-reserialize step for bodies
that aren't valid JSON, doing raw substring substitution instead — which is
what this contract's own `form_encode()` was written assuming would
happen), or document the JSON-only constraint explicitly so contract
authors don't discover it by shipping a call that can never succeed.

**Status in this submission:** the Stripe evidence-submission code path is
implemented exactly to Stripe's real API spec (verified against Stripe's
docs) and left in place rather than removed, since it's correct code
blocked by a platform constraint outside this contract's control — see the
comment block at the top of `src/evidence.rs`. The Paystack path is
unaffected (JSON-native) and is confirmed reaching real placeholder
resolution — see "Deployment status" above.

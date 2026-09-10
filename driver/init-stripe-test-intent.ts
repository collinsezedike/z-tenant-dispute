// Creates a real Stripe test-mode PaymentIntent, purely so we have a
// genuine id to test `check-order` against. Does NOT go through the
// contract; test-data setup only. Reads the already-seeded
// `stripe_secret_key` back via the tenant control-plane and uses it only
// inside a native fetch() call, never passed to a subprocess / shell
// command, so it can't leak via a shelled-out error message the way an
// earlier version of this kind of script did for Paystack.
//
// Usage:
//   npx tsx init-stripe-test-intent.ts

import "dotenv/config";
import {
  T3nClient,
  TenantClient,
  NODE_URLS,
  setEnvironment,
  loadWasmComponent,
  eth_get_address,
  metamask_sign,
  createEthAuthInput,
  fetchTrustedManifest,
} from "@terminal3/t3n-sdk";

setEnvironment("testnet");

const T3N_API_KEY = process.env.T3N_API_KEY;
if (!T3N_API_KEY) {
  throw new Error("Missing T3N_API_KEY in environment (.env)");
}

const wasmComponent = await loadWasmComponent();
const address = eth_get_address(T3N_API_KEY);

const t3n = new T3nClient({
  trustAnchor: await fetchTrustedManifest("testnet"),
  wasmComponent,
  handlers: {
    EthSign: metamask_sign(address, undefined, T3N_API_KEY),
  },
});

await t3n.handshake();
const did = await t3n.authenticate(createEthAuthInput(address));
const tenantDid = did.value;

const tenant = new TenantClient({
  t3n,
  tenantDid,
  baseUrl: NODE_URLS.testnet,
});

const stripeKey = await tenant.maps.entryGet("secrets", "stripe_secret_key");
if (!stripeKey) {
  throw new Error("stripe_secret_key not found. Run seed-secret.ts first");
}

const resp = await fetch("https://api.stripe.com/v1/payment_intents", {
  method: "POST",
  headers: {
    Authorization: `Bearer ${stripeKey}`,
    "Content-Type": "application/x-www-form-urlencoded",
  },
  body: new URLSearchParams({
    amount: "5000",
    currency: "usd",
    "payment_method_types[]": "card",
  }).toString(),
});

const body = await resp.json();
if (!resp.ok) {
  throw new Error(`Stripe create PaymentIntent failed: HTTP ${resp.status}: ${JSON.stringify(body)}`);
}

console.log("PaymentIntent id:", body.id);
console.log("status:", body.status);

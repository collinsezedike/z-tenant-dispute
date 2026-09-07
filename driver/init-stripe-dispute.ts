// Creates a real Stripe test-mode PaymentIntent using the documented
// `pm_card_createDispute` PaymentMethod (docs.stripe.com/testing —
// "Testing Disputes"), which succeeds and then gets auto-disputed shortly
// after — giving us a genuine dispute id to test `get-payment-dispute` and
// `submit-dispute-evidence` against. Test-data setup only, doesn't go
// through the contract. Reads `stripe_secret_key` back and uses it only
// inside fetch() calls, never passed to a subprocess.
//
// Usage:
//   npx tsx init-stripe-dispute.ts

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
  throw new Error("stripe_secret_key not found — run seed-secret.ts first");
}

function authHeaders() {
  return {
    Authorization: `Bearer ${stripeKey}`,
    "Content-Type": "application/x-www-form-urlencoded",
  };
}

const piResp = await fetch("https://api.stripe.com/v1/payment_intents", {
  method: "POST",
  headers: authHeaders(),
  body: new URLSearchParams({
    amount: "500",
    currency: "usd",
    payment_method: "pm_card_createDispute",
    "payment_method_types[]": "card",
    confirm: "true",
  }).toString(),
});
const pi = await piResp.json();
if (!piResp.ok) {
  throw new Error(`create PaymentIntent failed: HTTP ${piResp.status} — ${JSON.stringify(pi)}`);
}
console.log("PaymentIntent id:", pi.id, "status:", pi.status);

// The dispute appears shortly after confirmation — poll a few times.
let disputeId: string | undefined;
for (let attempt = 1; attempt <= 8 && !disputeId; attempt++) {
  await new Promise((r) => setTimeout(r, 3000));
  const listResp = await fetch(
    `https://api.stripe.com/v1/disputes?payment_intent=${pi.id}`,
    { headers: authHeaders() }
  );
  const list = await listResp.json();
  if (listResp.ok && list.data && list.data.length > 0) {
    disputeId = list.data[0].id;
  } else {
    console.log(`attempt ${attempt}: no dispute yet`);
  }
}

if (!disputeId) {
  throw new Error("No dispute appeared after polling — try re-running or check Stripe dashboard.");
}

console.log("Dispute id:", disputeId);
console.log("order_id (payment_intent):", pi.id);

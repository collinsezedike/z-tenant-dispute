// Sets a self-grant so the caller's own DID may invoke dispute-contracts'
// functions and reach the payment-processor hosts they call out to.
// Without this, outbound calls fail with `host/http.egress_denied` even
// though the contract itself executes fine (auth + KV read both succeed
// first) — the allowlist is enforced against the *caller's* grant, not
// anything declared by the contract.
//
// Uses `t3n.agentAuthUpdate()` directly per the ADK docs'
// (developers/adk/tips) documented self-grant pattern — the SDK's newer
// delegation APIs (`setGrants`, `MemberDelegationDoc`) are org-scoped and
// don't apply to an individually self-admitted testnet tenant like this one.
//
// Usage:
//   npx tsx grant-egress.ts

import "dotenv/config";
import {
  T3nClient,
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
const tenantIdHex = tenantDid.slice("did:t3n:".length);

const CONTRACT_TAIL = "dispute-contracts";
const scriptName = `z:${tenantIdHex}:${CONTRACT_TAIL}`;

await t3n.agentAuthUpdate({
  agents: [
    {
      agentDid: tenantDid, // self-grant: caller invokes on its own behalf
      scripts: [
        {
          scriptName,
          functions: ["check-order", "get-payment-dispute", "submit-dispute-evidence"],
          allowedHosts: ["api.paystack.co", "api.stripe.com"],
        },
      ],
    },
  ],
});

console.log(`Granted ${tenantDid} egress to api.paystack.co + api.stripe.com on ${scriptName}`);

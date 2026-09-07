// One-time setup: creates the tenant's `secrets` KV map before any secret
// can be written to it. `map-entry-set` (used by seed-secret.ts) writes into
// an existing map — it does not create one, and fails with `map not found`
// if this hasn't been run first.
//
// Readers/writers are scoped to the current contract id only (least
// privilege) — matches the pattern in the ADK docs
// (developers/adk/tips/create-kv-maps). The writers restriction doesn't
// affect control-plane writes (seed-secret.ts uses map-entry-set, which
// bypasses it and always works for the map owner).
//
// IMPORTANT: re-registering the contract (a version bump, e.g. 0.2.0 ->
// 0.3.0) allocates a NEW contract_id — see README "Deployment status". This
// operation is idempotent (per the docs, "safe to re-run during
// redeployment"), so just re-run it with the new id after a redeploy.
//
// Usage:
//   npx tsx setup-secrets-map.ts <contract-id>
// Example:
//   npx tsx setup-secrets-map.ts 917

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

const [contractIdArg] = process.argv.slice(2);
if (!contractIdArg) {
  console.error("Usage: npx tsx setup-secrets-map.ts <contract-id>");
  process.exit(1);
}
const contractId = Number(contractIdArg);
if (!Number.isInteger(contractId)) {
  throw new Error(`contract-id must be an integer, got: ${contractIdArg}`);
}

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

const result = await tenant.maps.create({
  tail: "secrets",
  visibility: "private",
  writers: { only: [contractId] },
  readers: { only: [contractId] },
});

console.log(`Created z:<tid>:secrets, readable by contract id ${contractId}`);
console.log(JSON.stringify(result, null, 2));

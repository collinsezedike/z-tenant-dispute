// Seeds a named secret into the tenant's `secrets` KV map.
//
// Usage:
//   npx tsx seed-secret.ts <key-name> <value>
// Example:
//   npx tsx seed-secret.ts paystack_secret_key sk_test_xxxxxxxx

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

const [keyName, value] = process.argv.slice(2);
if (!keyName || !value) {
  console.error("Usage: npx tsx seed-secret.ts <key-name> <value>");
  process.exit(1);
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

await tenant.executeControl("map-entry-set", {
  map_name: tenant.canonicalName("secrets"),
  key: keyName,
  value,
});

console.log(`Seeded ${keyName} into z:<tid>:secrets for ${tenantDid}`);

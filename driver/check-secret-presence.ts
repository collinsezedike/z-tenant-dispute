// Confirms a secret key is present in the tenant's secrets map WITHOUT
// printing its value. Usage: npx tsx check-secret-presence.ts <key-name>
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

const [keyName] = process.argv.slice(2);
if (!keyName) {
  console.error("Usage: npx tsx check-secret-presence.ts <key-name>");
  process.exit(1);
}

setEnvironment("testnet");
const T3N_API_KEY = process.env.T3N_API_KEY!;
const wasmComponent = await loadWasmComponent();
const address = eth_get_address(T3N_API_KEY);
const t3n = new T3nClient({
  trustAnchor: await fetchTrustedManifest("testnet"),
  wasmComponent,
  handlers: { EthSign: metamask_sign(address, undefined, T3N_API_KEY) },
});
await t3n.handshake();
const did = await t3n.authenticate(createEthAuthInput(address));
const tenant = new TenantClient({ t3n, tenantDid: did.value, baseUrl: NODE_URLS.testnet });

const value = await tenant.maps.entryGet("secrets", keyName);
console.log(value ? `present (length ${value.length})` : "NOT present");

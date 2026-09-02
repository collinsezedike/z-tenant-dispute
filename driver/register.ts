import "dotenv/config";
import { readFile } from "fs/promises";
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

console.log("Connected as:", tenantDid);

const tenant = new TenantClient({
  t3n,
  tenantDid,
  baseUrl: NODE_URLS.testnet,
});

const WASM_PATH = "../target/wasm32-wasip2/release/z_tenant_dispute.wasm";
const CONTRACT_TAIL = "dispute-contracts";
const CONTRACT_VERSION = "0.1.0";

const wasmBytes = await readFile(WASM_PATH);
console.log(`Read ${wasmBytes.length} bytes from ${WASM_PATH}`);

const result = await tenant.contracts.register({
  tail: CONTRACT_TAIL,
  version: CONTRACT_VERSION,
  wasm: wasmBytes,
});

const tenantIdHex = tenantDid.slice("did:t3n:".length);
const scriptName = `z:${tenantIdHex}:${CONTRACT_TAIL}`;

console.log(`Registered ${scriptName} as contract id ${result.contract_id}`);
console.log("Full result:", JSON.stringify(result, null, 2));

// Invokes one exported function on the registered dispute-contracts
// contract.
//
// Usage:
//   npx tsx invoke.ts <function-name> '<json-input>'
// Examples:
//   npx tsx invoke.ts check-order '{"order_ref":"T123456","provider":"paystack"}'
//   npx tsx invoke.ts get-payment-dispute '{"dispute_id":"1","provider":"paystack"}'
//   npx tsx invoke.ts submit-dispute-evidence '{"dispute_id":"1","order_id":"ORD-1","product_description":"Dinner for two","provider":"paystack"}'

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

const [functionName, inputJson] = process.argv.slice(2);
if (!functionName || !inputJson) {
  console.error(
    "Usage: npx tsx invoke.ts <function-name> '<json-input>'"
  );
  process.exit(1);
}

const parsedInput = JSON.parse(inputJson);

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

const CONTRACT_TAIL = "dispute-contracts";
const CONTRACT_VERSION = "0.2.0";

const result = await tenant.contracts.execute(CONTRACT_TAIL, {
  version: CONTRACT_VERSION,
  functionName,
  input: parsedInput,
});

console.log(JSON.stringify(result, null, 2));

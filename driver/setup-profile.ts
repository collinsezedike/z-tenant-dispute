// Attempts to populate this test tenant's own T3N user profile
// (first_name/last_name/verified email+phone) so the
// `{{profile.verified_contacts.*}}` placeholders used by
// submit-dispute-evidence can actually resolve. Diagnostic/setup script —
// prints raw OTP flow responses to learn whether testnet runs skip_otp.
//
// Usage:
//   npx tsx setup-profile.ts

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
console.log("Authenticated as:", did.value);

const emailResult = await t3n.otpRequest({
  emailChannel: { emailAddress: "z-tenant-dispute-test@example.com" },
});
console.log("otpRequest (email) result:", JSON.stringify(emailResult, null, 2));

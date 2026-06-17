// JS NATIVE URI-parser probe (WHATWG URL, Node). For each input, reports what
// `new URL(value)` thinks. WHATWG URL requires an absolute URL (a base-less
// URL must be absolute), so successful construction ~ "valid absolute URL".
// WHATWG URL is famously LENIENT and NORMALIZING (lowercases scheme/host,
// rewrites backslashes for special schemes, percent-encodes, etc).
//
// Emits JSON Lines: {"id","engine":"js-native","valid":bool,"detail":string}
// Run: node native.mjs ../native_inputs.json
import { readFileSync } from "node:fs";

const path = process.argv[2] ?? "../native_inputs.json";
const corpus = JSON.parse(readFileSync(path, "utf8"));

for (const inp of corpus.inputs) {
  let valid = false;
  let detail = "";
  try {
    const u = new URL(inp.value);
    valid = true;
    detail = `href=${u.href}`; // shows normalization
  } catch (e) {
    valid = false;
    detail = `error: ${e.message}`;
  }
  process.stdout.write(
    JSON.stringify({ id: inp.id, engine: "js-native", valid, detail }) + "\n",
  );
}

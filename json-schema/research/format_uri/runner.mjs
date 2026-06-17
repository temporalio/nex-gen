// JS runner for the PINNED `uri` check. Anchors the body with ^...$ and the `u`
// flag (JS `$` without `m` = end of input, no trailing-\n exception).
//
// Emits JSON Lines: {"id","engine":"js","compiled":bool,"matched":bool|null}
// Run: node runner.mjs [corpus.json] [pinned_body.json]
import { readFileSync } from "node:fs";

const corpusPath = process.argv[2] ?? "corpus.json";
const bodyPath = process.argv[3] ?? "pinned_body.json";
const corpus = JSON.parse(readFileSync(corpusPath, "utf8"));
const { body } = JSON.parse(readFileSync(bodyPath, "utf8"));

let re = null;
let compiled = false;
try {
  re = new RegExp("^" + body + "$", "u");
  compiled = true;
} catch (e) {
  process.stderr.write(`JS COMPILE ERROR: ${e.message}\n`);
}

for (const p of corpus.pairs) {
  const matched = compiled ? re.test(p.value) : null;
  process.stdout.write(
    JSON.stringify({ id: p.id, engine: "js", compiled, matched }) + "\n",
  );
}

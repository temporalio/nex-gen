// JS runner (node). Uses `new RegExp(p, "u")` then `.test(v)` (unanchored),
// mirroring the pinned runtime semantics for the TypeScript/JS target.
//
// Reads corpus.json (argv[2] or ./corpus.json) and emits JSON Lines to stdout:
//   {"id","engine":"js","compiled":bool,"matched":bool|null}
//
// Run: node runner.mjs [corpus.json]
import { readFileSync } from "node:fs";

const path = process.argv[2] ?? "corpus.json";
const corpus = JSON.parse(readFileSync(path, "utf8"));

for (const p of corpus.pairs) {
  let compiled = false;
  let matched = null;
  try {
    const re = new RegExp(p.pattern, "u");
    compiled = true;
    matched = re.test(p.instance);
  } catch {
    compiled = false;
    matched = null;
  }
  process.stdout.write(
    JSON.stringify({ id: p.id, engine: "js", compiled, matched }) + "\n",
  );
}

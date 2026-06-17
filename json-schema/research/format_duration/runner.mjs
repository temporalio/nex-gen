// JS runner (node) for the `duration` format conformance corpus.
//
// Compiles the SINGLE generator-owned pinned regex (from corpus.json's
// `pinned_regex`) with `new RegExp(p, "u")` (the u flag is the pinned choice
// for the TS/JS target) and tests each corpus value. The pinned regex is fully
// anchored (^...$), so `.test` gives the whole-string verdict.
//
// Reads corpus.json (argv[2] or ./corpus.json) and emits JSON Lines to stdout:
//   {"id","engine":"js","compiled":bool,"matched":bool|null}
//
// Run: node runner.mjs [corpus.json]
import { readFileSync } from "node:fs";

const path = process.argv[2] ?? "corpus.json";
const corpus = JSON.parse(readFileSync(path, "utf8"));

let re = null;
let compiled = false;
try {
  re = new RegExp(corpus.pinned_regex, "u");
  compiled = true;
} catch {
  compiled = false;
}

for (const k of corpus.cases) {
  const matched = compiled ? re.test(k.value) : null;
  process.stdout.write(
    JSON.stringify({ id: k.id, engine: "js", compiled, matched }) + "\n",
  );
}

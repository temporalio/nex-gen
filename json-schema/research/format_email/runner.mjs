// JS runner (node). Compiles the pinned email regex with `new RegExp(p, "u")`
// (u flag = code-point `.`; harmless here since the regex uses no bare `.`) and
// applies `.test(v)`. JS `$` (no `m` flag) is end-of-input only -- same as Go --
// so no anchor normalization is applied.
//
// Emits JSON Lines: {"id","engine":"js","compiled":bool,"matched":bool|null}
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

for (const p of corpus.pairs) {
  const matched = compiled ? re.test(p.instance) : null;
  process.stdout.write(
    JSON.stringify({ id: p.id, engine: "js", compiled, matched }) + "\n",
  );
}

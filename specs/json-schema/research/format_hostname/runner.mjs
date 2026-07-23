// JS runner for the `hostname` format conformance corpus (TypeScript target).
//
// Implements the PINNED generator-owned check:
//   1. module-level RegExp with the `u` flag, fully anchored ^...$
//   2. a total-length guard (1..=253 CODE POINTS) OUTSIDE the regex.
//      IMPORTANT: use [...v].length (code points), not v.length (UTF-16 units),
//      so the length agrees with the other targets on astral input. (Hostnames
//      are ASCII so this only matters for the non-ASCII reject rows, where the
//      regex already rejects; kept correct on principle.)
// Verdict = (regex matches) AND (length in range).
//
// JS `$` (no `m` flag) matches end-of-input only -- the portable choice.
//
// Emits JSON Lines: {"id","engine":"js","valid","regex","len_ok"}
// Run: node runner.mjs [corpus.json]
import { readFileSync } from "node:fs";

const HOST_RE =
  /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$/u;
const MAX_TOTAL_LEN = 253;

const path = process.argv[2] ?? "corpus.json";
const corpus = JSON.parse(readFileSync(path, "utf8"));

for (const k of corpus.cases) {
  const n = [...k.instance].length; // code points
  const lenOk = n >= 1 && n <= MAX_TOTAL_LEN;
  const regex = HOST_RE.test(k.instance);
  process.stdout.write(
    JSON.stringify({
      id: k.id,
      engine: "js",
      valid: regex && lenOk,
      regex,
      len_ok: lenOk,
    }) + "\n",
  );
}

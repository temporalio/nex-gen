#!/usr/bin/env python3
"""Cross-engine agreement check for \\s/\\S normalization.

Pipeline:
  1. Build the Rust rewrite probe and use its `normalize_corpus` binary to turn
     corpus.json (raw \\s/\\S patterns) into corpus.normalized.json (canonical
     explicit ASCII whitespace classes). Un-normalizable pairs are dropped.
  2. Run the four target engines (Go/JS/Python/Java) over BOTH corpora, using
     the SAME runners as json-schema/research/pattern_conformance (which mirror
     the pinned runtime semantics: Go RE2, JS `u`, Python re.ASCII, Java default
     flags, all UNANCHORED search).
  3. Report, per pair:
       - ORIGINAL: do the four engines agree? (expect DIVERGENCE on NBSP/U+3000
         and on U+000B vertical tab)
       - NORMALIZED: do the four engines agree? (MUST be unanimous)

Exit nonzero if any NORMALIZED pair diverges.

Run: python3 agree.py   (needs go/node/python3/java/cargo on PATH)
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROBE = HERE / "rewrite_probe"
CONF = HERE.parent / "pattern_conformance"  # reuse the existing runners
CORPUS = HERE / "corpus.json"
NORM_CORPUS = HERE / "corpus.normalized.json"

ENGINES = ["go", "js", "python", "java"]


def run(cmd, cwd=None, capture_err=False):
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8")
    if proc.returncode != 0:
        sys.stderr.write(f"command failed: {cmd}\n{proc.stderr}\n")
        sys.exit(1)
    if capture_err:
        return proc.stdout, proc.stderr
    return proc.stdout


def parse_lines(text):
    out = {}
    for line in text.splitlines():
        line = line.strip()
        if line:
            rec = json.loads(line)
            out[rec["id"]] = rec
    return out


def engine_results(corpus_path):
    return {
        "go": parse_lines(run(["go", "run", str(CONF / "runner.go"), str(corpus_path)])),
        "js": parse_lines(run(["node", str(CONF / "runner.mjs"), str(corpus_path)])),
        "python": parse_lines(run(["python3", str(CONF / "runner.py"), str(corpus_path)])),
        "java": parse_lines(run(["java", str(CONF / "Runner.java"), str(corpus_path)])),
    }


def main():
    print("Building rust rewrite probe...", file=sys.stderr)
    run(["cargo", "build", "--release", "--quiet"], cwd=PROBE)
    norm_bin = PROBE / "target" / "release" / "normalize_corpus"

    out, err = run([str(norm_bin), str(CORPUS)], capture_err=True)
    NORM_CORPUS.write_text(out, encoding="utf-8")
    sys.stderr.write(err)

    orig = json.loads(CORPUS.read_text(encoding="utf-8"))
    norm = json.loads(NORM_CORPUS.read_text(encoding="utf-8"))
    orig_pairs = {p["id"]: p for p in orig["pairs"]}
    norm_pairs = {p["id"]: p for p in norm["pairs"]}

    orig_res = engine_results(CORPUS)
    norm_res = engine_results(NORM_CORPUS)

    def agree(res, pid):
        vals = {e: res[e][pid]["matched"] for e in ENGINES}
        compiled = {e: res[e][pid]["compiled"] for e in ENGINES}
        return vals, compiled, (len(set(vals.values())) == 1 and all(compiled.values()))

    print("=" * 78)
    print("WHITESPACE NORMALIZATION -- CROSS-ENGINE AGREEMENT")
    print("=" * 78)

    orig_divergences = []
    norm_divergences = []

    print("\n--- (a) ORIGINAL \\s/\\S patterns (may diverge) ---")
    for pid in orig_pairs:
        vals, comp, ok = agree(orig_res, pid)
        if not ok:
            orig_divergences.append(pid)
            p = orig_pairs[pid]
            print(f"  DIVERGE {pid}: pattern={p['pattern']!r} instance={p['instance']!r}")
            print(f"          {vals}")
    if not orig_divergences:
        print("  (no divergences among originals -- unexpected)")
    else:
        print(f"  -> {len(orig_divergences)} original pairs diverge across engines.")

    print("\n--- (b) NORMALIZED patterns (MUST all agree) ---")
    for pid in norm_pairs:
        vals, comp, ok = agree(norm_res, pid)
        if not ok:
            norm_divergences.append(pid)
            p = norm_pairs[pid]
            print(f"  DIVERGE {pid}: pattern={p['pattern']!r} instance={p['instance']!r}")
            print(f"          compiled={comp} matched={vals}")
    if not norm_divergences:
        print(f"  OK: all {len(norm_pairs)} normalized pairs agree across Go/JS/Python/Java.")

    print("\n--- side-by-side (id | orig-agree | orig-vals -> norm-agree | norm-vals) ---")
    for pid in norm_pairs:
        ov, _, ook = agree(orig_res, pid)
        nv, _, nok = agree(norm_res, pid)
        flip = "  <== fixed" if (not ook and nok) else ""
        print(f"  {pid:22} orig={'AGREE' if ook else 'DIVER'} {str(ov):46} -> norm={'AGREE' if nok else 'DIVER'} {str(nv)}{flip}")

    print("\n--- summary ---")
    print(f"  original divergences:   {len(orig_divergences)}")
    print(f"  normalized divergences: {len(norm_divergences)}")
    ok = not norm_divergences
    print("\nVERDICT:", "PASS -- normalization makes all engines agree" if ok
          else "FAIL -- normalized patterns still diverge")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

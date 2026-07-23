#!/usr/bin/env python3
"""Compare the Ruby runner's pinned `format` check against the corpus and against
the four current targets (Go, JS, Python, Java), to establish whether Ruby is a
future-conformant target for the asserted-v1 `format` subset.

The pinned check is generator-owned (pinned regex + calendar arithmetic), so Ruby
should agree value-for-value. The only Ruby-specific pinning is anchoring: Ruby's
`^`/`$` are line anchors, so the pinned pattern uses `\\A`/`\\z` (start/strict-end
of string) -- exactly what runner.rb encodes.

Run: python3 compare_ruby.py
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE / "corpus.json"
REF_ENGINES = ["go", "js", "python", "java"]


def run(cmd, cwd=None):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8")
    if p.returncode != 0:
        sys.stderr.write(f"command failed: {cmd}\n{p.stderr}\n")
        sys.exit(1)
    return p.stdout


def parse_lines(text):
    out = {}
    for line in text.splitlines():
        line = line.strip()
        if line:
            r = json.loads(line)
            out[r["id"]] = r
    return out


def main():
    corpus = json.loads(CORPUS.read_text(encoding="utf-8"))
    pairs = {p["id"]: p for p in corpus["pairs"]}

    ref = {
        "go": parse_lines(run(["go", "run", str(HERE / "runner.go"), str(CORPUS)])),
        "js": parse_lines(run(["node", str(HERE / "runner.mjs"), str(CORPUS)])),
        "python": parse_lines(run(["python3", str(HERE / "runner.py"), str(CORPUS)])),
        "java": parse_lines(run(["java", str(HERE / "Runner.java"), str(CORPUS)])),
    }
    ruby = parse_lines(run(["ruby", str(HERE / "runner.rb"), str(CORPUS)]))

    corpus_disagree = []   # (pid, expect, ruby)
    ref_disagree = []      # (pid, ref_vals, ruby)
    native_notes = []      # (pid, fmt, val, pinned, native)
    for pid, p in pairs.items():
        exp = p["expect_valid"]
        rv = ruby[pid]["valid"]
        if rv != exp:
            corpus_disagree.append((pid, exp, rv))
        ref_vals = {e: ref[e][pid]["valid"] for e in REF_ENGINES}
        # reference is the agreed value (they all agree; guard anyway)
        ref_val = next(iter(set(ref_vals.values()))) if len(set(ref_vals.values())) == 1 else None
        if ref_val is None or rv != ref_val:
            ref_disagree.append((pid, ref_vals, rv))
        nat = ruby[pid].get("native")
        if nat is not None and nat != rv:
            native_notes.append((pid, p["format"], p["value"], rv, nat))

    print("=" * 72)
    print("RUBY FORMAT CONFORMANCE vs corpus + 4-engine reference")
    print("=" * 72)
    print(f"total pairs: {len(pairs)}")
    print()

    print("--- corpus agreement (ruby pinned check vs expect_valid) ---")
    if not corpus_disagree:
        print("  OK: Ruby agreed with the corpus on every pair.")
    else:
        for pid, exp, rv in corpus_disagree:
            p = pairs[pid]
            print(f"  DISAGREE {pid} [{p['format']}]: expect={exp} ruby={rv} value={p['value']!r}")
    print()

    print("--- cross-engine agreement (ruby vs go/js/python/java) ---")
    if not ref_disagree:
        print("  OK: Ruby agreed with all four current targets on every pair.")
    else:
        for pid, refv, rv in ref_disagree:
            p = pairs[pid]
            print(f"  DIVERGENCE {pid} [{p['format']}] value={p['value']!r}: ref={refv} ruby={rv}")
    print()

    print("--- (informational) Ruby native parser vs pinned check ---")
    if not native_notes:
        print("  (none)")
    else:
        print("  Documentation only -- the pinned check is the verdict:")
        for pid, fmt, val, pinned, nat in native_notes:
            print(f"    {pid} [{fmt}]: pinned={pinned} native={nat} value={val!r}")
    print()

    ok = not corpus_disagree and not ref_disagree
    print("VERDICT:", "PASS - Ruby is future-conformant with the owned check"
          if ok else "FAIL - see divergences above")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

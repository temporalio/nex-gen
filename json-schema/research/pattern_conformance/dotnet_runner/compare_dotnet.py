#!/usr/bin/env python3
"""Compare the .NET (C#) runner against the reference agreed by the four current
engines (Go, JS, Python, Java) under the FINAL pinned design.

Reference construction (read-only; does not modify existing files):
  * Run go/js/python runners as-is.
  * Apply the FINAL gate: skip pairs flagged expect_gate_reject (lookaround /
    backref) PLUS pairs that the final gate additionally rejects:
      - inline flag groups `(?...)`          (case-inline-flag)
      - `\\s` / `\\S`                          (space-* / notspace-*)
    These are excluded from the match comparison because the design rejects
    them at load time.
  * `$`-normalization: the design normalizes a trailing `$` to end-of-input on
    every target (Go/JS keep `$`, Python `\\Z`, Java `\\z`, .NET `\\z`), so the
    reference for a `$`-bearing pair is the Go/JS result (end-of-input, no
    trailing-\\n exception). Go and JS already have that semantics, so their raw
    output IS the normalized reference.

The .NET runner already applies `$`->`\\z` and RegexOptions.ECMAScript, so its
output is directly comparable to the Go/JS reference on the accepted subset.

Run: python3 compare_dotnet.py
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PC = HERE.parent            # pattern_conformance/
CORPUS = PC / "corpus.json"

# Pairs the FINAL gate rejects beyond the corpus's expect_gate_reject flag.
GATE_REJECT_EXTRA = {
    "case-inline-flag",   # inline (?i) flag group
    "space-ascii-hit", "space-tab", "space-nbsp", "space-ideographic",
    "notspace-on-nbsp",   # \s / \S
}


def run(cmd, cwd=None):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8")
    if p.returncode != 0:
        sys.stderr.write(f"FAILED: {cmd}\n{p.stderr}\n")
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

    go = parse_lines(run(["go", "run", str(PC / "runner.go"), str(CORPUS)]))
    js = parse_lines(run(["node", str(PC / "runner.mjs"), str(CORPUS)]))
    py = parse_lines(run(["python3", str(PC / "runner.py"), str(CORPUS)]))
    dotnet = parse_lines(run(
        ["dotnet", "run", "--project", str(HERE / "DotnetRunner"), "-c", "Release", "--", str(CORPUS)]
    ))

    # Accepted subset = gate-accepted (not expect_gate_reject, not extra-rejected).
    accepted = [
        pid for pid, p in pairs.items()
        if not p.get("expect_gate_reject", False) and pid not in GATE_REJECT_EXTRA
    ]

    # Reference = Go/JS agreement (they already have end-of-input `$`).
    divergences = []
    ref_disagree = []
    for pid in accepted:
        ref_go = go[pid]["matched"]
        ref_js = js[pid]["matched"]
        if ref_go != ref_js:
            ref_disagree.append((pid, ref_go, ref_js))
            continue
        ref = ref_go
        got = dotnet[pid]["matched"]
        if not dotnet[pid]["compiled"]:
            divergences.append((pid, pairs[pid]["pattern"], pairs[pid]["instance"],
                                ref, "COMPILE-FAIL"))
        elif got != ref:
            divergences.append((pid, pairs[pid]["pattern"], pairs[pid]["instance"],
                                ref, got))

    print("=" * 72)
    print(".NET (C#) vs reference (Go/JS, final pinned design)")
    print("=" * 72)
    print(f"total pairs:            {len(pairs)}")
    print(f"accepted subset:        {len(accepted)}")
    print(f"excluded (gate-reject): {sorted(set(pairs) - set(accepted))}")
    print()
    if ref_disagree:
        print("!! Go/JS reference disagreed on:")
        for pid, g, j in ref_disagree:
            print(f"   {pid}: go={g} js={j}")
        print()
    print("--- .NET divergences from reference ---")
    if not divergences:
        print("  NONE: .NET matched the Go/JS reference on every accepted pair.")
    else:
        for pid, pat, inst, ref, got in divergences:
            print(f"  DIVERGENCE {pid}:")
            print(f"    pattern  = {pat!r}")
            print(f"    instance = {inst!r}")
            print(f"    reference(go/js) = {ref}")
            print(f"    dotnet           = {got}")
    print()
    print(f"accepted pairs agreeing: {len(accepted) - len(divergences)}/{len(accepted)}")
    sys.exit(0 if not divergences else 2)


if __name__ == "__main__":
    main()

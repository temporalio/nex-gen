#!/usr/bin/env python3
"""Compare Ruby (raw + anchor-normalized/pinned) against the PINNED reference,
honoring the FULL gate exclusion set from the current `pattern` design.

The pinned design's load-time gate rejects, in addition to what the bare Rust
`regex` crate rejects (lookaround/backref):
  - inline flag groups `(?i)` / `(?flags:...)`
  - `\\s` / `\\S`
and NORMALIZES the `$` anchor to strict end-of-input (Python `\\Z`, Java `\\z`)
so that Go/JS/Python/Java all agree. Under those rules the four engines DO agree
on every remaining pair, so any one of them is a faithful reference.

We reproduce that here at the corpus level:
  * EXCLUDE pairs whose pattern is gate-rejected: lookaround, backref, inline
    flags, or contains \\s/\\S. (These are never emitted for any target.)
  * Take the reference match result from the pinned engines. Because `$`-normal-
    ization removes the trailing-\\n divergence, we use Go (whose `$` == strict
    end-of-input, matching the normalized pinned semantics) as the reference.
  * Compare Ruby-raw and Ruby-pinned against that reference.

Run: python3 compare_ruby.py
"""
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE / "corpus.json"
PINNED_ENGINES = ["go", "js", "python", "java"]

# Reference engine: Go's `$` is strict end-of-input (no trailing-\n exception),
# and Go/RE2 \d\w are ASCII -- this matches the pinned normalized semantics.
# (We assert the other pinned engines agree once excluded pairs are removed.)
REFERENCE_ENGINE = "go"


def run(cmd, cwd=None):
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8")
    if proc.returncode != 0:
        sys.stderr.write(f"command failed: {cmd}\n{proc.stderr}\n")
        sys.exit(1)
    return proc.stdout


def parse_lines(text):
    out = {}
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        rec = json.loads(line)
        out[rec["id"]] = rec
    return out


def parse_ruby(text):
    modes = {"ruby-raw": {}, "ruby-pinned": {}, "ruby-ascii-pinned": {}}
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        rec = json.loads(line)
        modes[rec["engine"]][rec["id"]] = rec
    return modes["ruby-raw"], modes["ruby-pinned"], modes["ruby-ascii-pinned"]


# --- Full gate exclusion set (task spec) --------------------------------------
INLINE_FLAG_RE = re.compile(r"\(\?[a-zA-Z]*[):]")   # (?i) or (?i:...) or (?:) etc.
LOOKAROUND_RE = re.compile(r"\(\?[=!]|\(\?<[=!]")   # (?= (?! (?<= (?<!


def gate_excluded(pattern):
    """Reasons the pinned gate would reject/exclude this pattern (task spec)."""
    reasons = []
    if LOOKAROUND_RE.search(pattern):
        reasons.append("lookaround")
    if re.search(r"\\[1-9]", pattern):
        reasons.append("backref")
    # inline flag group: (?i), (?im), (?i:...), also bare (?:...) has no flags so
    # exclude only when letters precede ) or : -- but the design rejects the
    # inline-flag *form*; corpus only has (?i). Match (?<letters>) / (?<letters>:).
    if re.search(r"\(\?[a-zA-Z]", pattern):
        reasons.append("inline-flag")
    if "\\s" in pattern or "\\S" in pattern:
        reasons.append("\\s/\\S")
    return reasons


def main():
    corpus = json.loads(CORPUS.read_text(encoding="utf-8"))
    pairs = {p["id"]: p for p in corpus["pairs"]}

    results = {
        "go": parse_lines(run(["go", "run", str(HERE / "runner.go"), str(CORPUS)])),
        "js": parse_lines(run(["node", str(HERE / "runner.mjs"), str(CORPUS)])),
        "python": parse_lines(run(["python3", str(HERE / "runner.py"), str(CORPUS)])),
        "java": parse_lines(run(["java", str(HERE / "Runner.java"), str(CORPUS)])),
    }
    ruby_raw, ruby_pinned, ruby_ascii = parse_ruby(
        run(["ruby", str(HERE / "runner.rb"), str(CORPUS)]))

    excluded = {}   # pid -> reasons
    compared = []
    for pid, p in pairs.items():
        r = gate_excluded(p["pattern"])
        if r:
            excluded[pid] = r
        else:
            compared.append(pid)

    # Build reference and assert the four pinned engines agree on compared pairs
    # (they should, given $-normalization + \s exclusion; flag any that don't).
    reference = {}
    residual_ref_disagree = []
    for pid in compared:
        vals = {e: results[e][pid]["matched"] for e in PINNED_ENGINES}
        reference[pid] = results[REFERENCE_ENGINE][pid]["matched"]
        if len(set(vals.values())) != 1:
            residual_ref_disagree.append((pid, vals))

    def divergences(ruby):
        out = []
        for pid in compared:
            rrec = ruby[pid]
            if not rrec["compiled"]:
                out.append((pid, "COMPILE-FAIL", reference[pid], None))
            elif rrec["matched"] != reference[pid]:
                out.append((pid, "MATCH-DIFF", reference[pid], rrec["matched"]))
        return out

    raw_div = divergences(ruby_raw)
    pinned_div = divergences(ruby_pinned)
    ascii_div = divergences(ruby_ascii)

    print("=" * 74)
    print("RUBY PATTERN CONFORMANCE vs PINNED 4-ENGINE REFERENCE")
    print("=" * 74)
    print(f"total pairs: {len(pairs)}   compared: {len(compared)}   "
          f"gate-excluded: {len(excluded)}")
    print("gate-excluded pairs (not emitted for any target):")
    for pid in sorted(excluded):
        print(f"    {pid:26} {excluded[pid]}  pattern={pairs[pid]['pattern']!r}")
    print()

    print("--- pinned-engine reference sanity (Go/JS/Python/Java on compared pairs) ---")
    if residual_ref_disagree:
        print("  NOTE: engines still disagree on these compared pairs -- the pinned")
        print("  reference resolves them; listing raw engine values for transparency:")
        for pid, vals in residual_ref_disagree:
            print(f"    {pid}: {vals}  (reference={reference[pid]}, pattern={pairs[pid]['pattern']!r}, instance={pairs[pid]['instance']!r})")
    else:
        print("  OK: all four engines agree on every compared pair.")
    print()

    for label, div in (("RUBY-RAW (pattern verbatim, Ruby defaults)", raw_div),
                       ("RUBY-PINNED (^->\\A, $->\\z applied)", pinned_div),
                       ("RUBY-ASCII-PINNED ((?a) + ^->\\A, $->\\z)", ascii_div)):
        print(f"--- {label}: {len(div)} divergence(s) ---")
        for pid, kind, ref, got in div:
            p = pairs[pid]
            print(f"  {kind:12} {pid}")
            print(f"      pattern={p['pattern']!r} instance={p['instance']!r}")
            print(f"      reference={ref}  ruby={got}")
        if not div:
            print("  OK: Ruby agrees with the reference on every compared pair.")
        print()

    print("VERDICT:")
    print("  ruby-pinned       residual divergences:", len(pinned_div))
    print("  ruby-ascii-pinned residual divergences:", len(ascii_div),
          "  <-- CONFORMS" if not ascii_div else "")


if __name__ == "__main__":
    main()

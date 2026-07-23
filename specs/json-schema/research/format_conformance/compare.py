#!/usr/bin/env python3
"""Run the pinned `format` check in every runtime against corpus.json, align by
pair id, and report:

  (a) Corpus-agreement: any pair where a runtime's pinned-check `valid` disagrees
      with the corpus `expect_valid`.
  (b) Cross-runtime agreement: any pair where the runtimes disagree with each
      other on `valid`.

This proves P1 for the asserted-v1 `format` subset: a single generator-OWNED
check (pinned regex + calendar arithmetic) is implementable identically in every
language. It also cross-checks the corpus verdicts against the Rust load-time
gate (which additionally proves the pinned patterns COMPILE in the pure-Rust
`regex` crate).

The four current targets (Go, JS, Python, Java) plus the Rust gate are compared
here. Ruby and .NET (prospective) have their own scripts: compare_ruby.py and
dotnet_runner/compare_dotnet.py.

Prints a summary and exits nonzero on any disagreement.

Run: python3 compare.py
(assumes go/node/python3/java/cargo on PATH; paths resolved relative to this file.)
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE / "corpus.json"

ENGINES = ["rust", "go", "js", "python", "java"]


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


def main():
    corpus = json.loads(CORPUS.read_text(encoding="utf-8"))
    pairs = {p["id"]: p for p in corpus["pairs"]}

    print("Building rust runner (cargo build --release)...", file=sys.stderr)
    run(["cargo", "build", "--release", "--quiet"], cwd=HERE / "rust_runner")
    rust_bin = HERE / "rust_runner" / "target" / "release" / "rust_runner"

    results = {
        "rust": parse_lines(run([str(rust_bin), str(CORPUS)])),
        "go": parse_lines(run(["go", "run", str(HERE / "runner.go"), str(CORPUS)])),
        "js": parse_lines(run(["node", str(HERE / "runner.mjs"), str(CORPUS)])),
        "python": parse_lines(run(["python3", str(HERE / "runner.py"), str(CORPUS)])),
        "java": parse_lines(run(["java", str(HERE / "Runner.java"), str(CORPUS)])),
    }

    total = len(pairs)

    # (a) corpus-agreement per engine
    corpus_disagree = []  # (pid, engine, expect, got)
    for pid, p in pairs.items():
        exp = p["expect_valid"]
        for eng in ENGINES:
            got = results[eng][pid]["valid"]
            if got != exp:
                corpus_disagree.append((pid, eng, exp, got))

    # (b) cross-runtime agreement
    cross_disagree = []  # (pid, {eng: valid})
    for pid, p in pairs.items():
        vals = {eng: results[eng][pid]["valid"] for eng in ENGINES}
        if len(set(vals.values())) > 1:
            cross_disagree.append((pid, vals))

    # native-vs-pinned divergence (informational only; never a failure)
    native_diverge = []  # (pid, engine, pinned, native)
    for pid, p in pairs.items():
        for eng in ("go", "js", "python", "java"):
            rec = results[eng][pid]
            nat = rec.get("native")
            if nat is not None and nat != rec["valid"]:
                native_diverge.append((pid, eng, rec["valid"], nat, p["format"], p["value"]))

    print("=" * 72)
    print("FORMAT CONFORMANCE REPORT (rust gate + go/js/python/java)")
    print("=" * 72)
    print(f"total pairs: {total}")
    by_fmt = {}
    for p in pairs.values():
        by_fmt[p["format"]] = by_fmt.get(p["format"], 0) + 1
    print(f"by format:   {by_fmt}")
    print()

    print("--- (a) corpus agreement: pinned check must equal expect_valid ---")
    if not corpus_disagree:
        print("  OK: every engine agreed with the corpus on every pair.")
    else:
        for pid, eng, exp, got in corpus_disagree:
            p = pairs[pid]
            print(f"  DISAGREE {pid} [{p['format']}] {eng}: expect={exp} got={got} value={p['value']!r}")
    print()

    print("--- (b) cross-runtime agreement: the five engines must agree ---")
    if not cross_disagree:
        print("  OK: rust, go, js, python, java agreed on every pair.")
    else:
        for pid, vals in cross_disagree:
            p = pairs[pid]
            print(f"  DIVERGENCE {pid} [{p['format']}] value={p['value']!r}")
            for eng in ENGINES:
                print(f"      {eng:7} valid = {vals[eng]}")
    print()

    print("--- (informational) native typed parser vs pinned check ---")
    if not native_diverge:
        print("  (no native-vs-pinned divergences recorded)")
    else:
        print("  These are DOCUMENTATION only -- the pinned check is the verdict.")
        print("  They show why we do NOT delegate to native parsers (P1 hazard):")
        for pid, eng, pinned, nat, fmt, val in native_diverge:
            print(f"    {pid} [{fmt}] {eng}: pinned={pinned} native={nat} value={val!r}")
    print()

    print("--- summary ---")
    print(f"  corpus disagreements:     {len(corpus_disagree)}")
    print(f"  cross-runtime divergences:{len(cross_disagree)}")
    print(f"  native-vs-pinned notes:   {len(native_diverge)} (informational)")
    ok = not corpus_disagree and not cross_disagree
    print()
    print("VERDICT:", "PASS - the pinned check is identical across all five engines and matches the corpus"
          if ok else "FAIL - see disagreements above")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

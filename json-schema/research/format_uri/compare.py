#!/usr/bin/env python3
"""Run all 7 engines against the PINNED `uri` check + corpus.json, align by id,
and report:

  (a) Compile-acceptance: does every engine COMPILE the pinned anchored regex?
      (The Rust gate proves RE2-safety; the others must accept it too.)
  (b) Match-agreement: do all 7 engines agree on the boolean match per value?
  (c) Expectation check: does the agreed verdict equal corpus `expect`?

Exits nonzero on any compile failure or match divergence.
Run: python3 compare.py
(assumes go/node/python3/java/ruby/dotnet/cargo on PATH)
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE / "corpus.json"
BODY = HERE / "pinned_body.json"

ENGINES = ["rust", "go", "js", "python", "java", "ruby", "dotnet"]


def run(cmd, cwd=None):
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8")
    if proc.returncode != 0:
        sys.stderr.write(f"command failed: {cmd}\n{proc.stdout}\n{proc.stderr}\n")
        sys.exit(1)
    if proc.stderr.strip():
        sys.stderr.write(proc.stderr)
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

    print("Building rust gate (cargo build --release)...", file=sys.stderr)
    run(["cargo", "build", "--release", "--quiet"], cwd=HERE / "rust_runner")
    rust_bin = HERE / "rust_runner" / "target" / "release" / "uri_gate"

    results = {
        "rust":   parse_lines(run([str(rust_bin), str(CORPUS), str(BODY)])),
        "go":     parse_lines(run(["go", "run", str(HERE / "runner.go"), str(CORPUS), str(BODY)])),
        "js":     parse_lines(run(["node", str(HERE / "runner.mjs"), str(CORPUS), str(BODY)])),
        "python": parse_lines(run(["python3", str(HERE / "runner.py"), str(CORPUS), str(BODY)])),
        "java":   parse_lines(run(["java", str(HERE / "Runner.java"), str(CORPUS), str(BODY)])),
        "ruby":   parse_lines(run(["ruby", str(HERE / "runner.rb"), str(CORPUS), str(BODY)])),
        "dotnet": parse_lines(run(
            ["dotnet", "run", "--project", str(HERE / "dotnet_runner" / "DotnetRunner"),
             "-c", "Release", "--", str(CORPUS), str(BODY)])),
    }

    # (a) compile-acceptance
    compile_failures = []
    for eng in ENGINES:
        # each engine reports the same compiled flag on every row; check first
        any_id = next(iter(pairs))
        if not results[eng][any_id]["compiled"]:
            compile_failures.append(eng)

    # (b) match-agreement + (c) expectation
    match_divergences = []
    expectation_mismatches = []
    for pid, p in pairs.items():
        if compile_failures:
            break
        vals = {eng: results[eng][pid]["matched"] for eng in ENGINES}
        if len(set(vals.values())) > 1:
            match_divergences.append((pid, p["value"], vals))
        else:
            agreed = next(iter(vals.values()))
            if agreed != p["expect"]:
                expectation_mismatches.append((pid, p["value"], agreed, p["expect"]))

    total = len(pairs)
    agree = total - len(match_divergences)

    print("=" * 78)
    print("PINNED `uri` CHECK CONFORMANCE REPORT")
    print("=" * 78)
    print(f"total pairs: {total}")
    print()

    print("--- (a) compile-acceptance: all 7 engines compile the pinned regex ---")
    if not compile_failures:
        print("  OK: rust/go/js/python/java/ruby/dotnet all compiled the pinned pattern.")
    else:
        print(f"  FAIL: compile failed in {compile_failures}")
    print()

    print("--- (b) match-agreement: all 7 engines agree per value ---")
    if not match_divergences:
        print("  OK: all 7 engines agreed on every corpus value.")
    else:
        for pid, val, vals in match_divergences:
            print(f"  DIVERGENCE {pid}: value={val!r}")
            for eng in ENGINES:
                print(f"      {eng:7} matched = {vals[eng]}")
    print()

    print("--- (c) expectation check: agreed verdict == corpus `expect` ---")
    if not expectation_mismatches:
        print("  OK: agreed verdict matched `expect` for every value.")
    else:
        for pid, val, agreed, exp in expectation_mismatches:
            print(f"  MISMATCH {pid}: value={val!r} agreed={agreed} expect={exp}")
    print()

    print("--- summary ---")
    print(f"  compile failures:     {len(compile_failures)}")
    print(f"  match divergences:    {len(match_divergences)}")
    print(f"  fully agreeing pairs: {agree}/{total}")
    print(f"  expectation mismatches: {len(expectation_mismatches)}  (design notes, not P1 failures)")
    print()

    ok = not compile_failures and not match_divergences
    print("VERDICT:", "PASS - pinned check is identical across all 7 engines"
          if ok else "FAIL - see divergences above")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

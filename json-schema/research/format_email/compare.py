#!/usr/bin/env python3
"""Run all SEVEN engines against corpus.json with the PINNED email regex, align
by pair id, and report:

  (a) Compile-acceptance: does the pinned regex compile in every engine?
      (The Rust `regex` crate is the gate; if it compiles there and everywhere,
       the regex is in the portable/RE2-safe subset.)

  (b) Verdict-agreement: do all seven engines agree, value-for-value, on
      valid/invalid for every corpus instance?

  (c) Intent check: does the agreed verdict match the corpus `expect_valid`?

Engines: rust (gate+runtime), go, js, python, java, ruby, dotnet.
Exits nonzero on any divergence or intent mismatch.

Run: python3 compare.py
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE / "corpus.json"

ENGINES = ["rust", "go", "js", "python", "java", "ruby", "dotnet"]


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
    run(["cargo", "build", "--release", "--quiet"], cwd=HERE / "gate_runner")
    rust_bin = HERE / "gate_runner" / "target" / "release" / "email_runner"

    print("Restoring/building dotnet runner...", file=sys.stderr)
    dotnet_proj = HERE / "dotnet_runner" / "EmailRunner"

    results = {
        "rust": parse_lines(run([str(rust_bin), str(CORPUS)])),
        "go": parse_lines(run(["go", "run", str(HERE / "runner.go"), str(CORPUS)])),
        "js": parse_lines(run(["node", str(HERE / "runner.mjs"), str(CORPUS)])),
        "python": parse_lines(run(["python3", str(HERE / "runner.py"), str(CORPUS)])),
        "java": parse_lines(run(["java", str(HERE / "Runner.java"), str(CORPUS)])),
        "ruby": parse_lines(run(["ruby", str(HERE / "runner.rb"), str(CORPUS)])),
        "dotnet": parse_lines(
            run(["dotnet", "run", "--project", str(dotnet_proj), "-c", "Release", "--", str(CORPUS)])
        ),
    }

    # --- (a) compile acceptance ---
    compile_failures = []  # (engine)
    for eng in ENGINES:
        # any pair record for this engine carries the compile flag (same for all)
        any_rec = next(iter(results[eng].values()))
        if not any_rec["compiled"]:
            compile_failures.append(eng)

    # --- (b) verdict agreement + (c) intent ---
    verdict_divergences = []  # (id, instance, {eng: matched})
    intent_mismatches = []    # (id, instance, expect, agreed)
    for pid, p in pairs.items():
        vals = {eng: results[eng][pid]["matched"] for eng in ENGINES}
        distinct = set(v for v in vals.values() if v is not None)
        if len(distinct) > 1:
            verdict_divergences.append((pid, p["instance"], vals))
            continue
        if not distinct:
            continue  # nothing compiled
        agreed = distinct.pop()
        if agreed != p["expect_valid"]:
            intent_mismatches.append((pid, p["instance"], p["expect_valid"], agreed))

    total = len(pairs)
    agreeing = total - len(verdict_divergences)

    print("=" * 72)
    print("EMAIL FORMAT CONFORMANCE REPORT (pinned RE2-safe regex)")
    print("=" * 72)
    print(f"pinned_regex: {corpus['pinned_regex']}")
    print(f"total pairs:  {total}")
    print(f"engines:      {ENGINES}")
    print()

    print("--- (a) compile-acceptance (pinned regex compiles in every engine) ---")
    if not compile_failures:
        print("  OK: pinned regex compiled in all seven engines.")
    else:
        for eng in compile_failures:
            print(f"  FAIL: pinned regex did NOT compile in {eng}")
    print()

    print("--- (b) verdict-agreement (all seven engines agree per instance) ---")
    if not verdict_divergences:
        print("  OK: all seven engines agreed on every instance.")
    else:
        for pid, inst, vals in verdict_divergences:
            print(f"  DIVERGENCE {pid}: instance={inst!r}")
            for eng in ENGINES:
                print(f"      {eng:7} matched = {vals[eng]}")
    print()

    print("--- (c) intent check (agreed verdict == corpus expect_valid) ---")
    if not intent_mismatches:
        print("  OK: every agreed verdict matched the corpus intent.")
    else:
        for pid, inst, exp, agreed in intent_mismatches:
            print(f"  MISMATCH {pid}: instance={inst!r} expect_valid={exp} agreed={agreed}")
    print()

    print("--- summary ---")
    print(f"  engines agreeing on every instance: {agreeing}/{total}")
    print(f"  compile failures:                   {len(compile_failures)}")
    print(f"  verdict divergences:                {len(verdict_divergences)}")
    print(f"  intent mismatches:                  {len(intent_mismatches)}")

    ok = not compile_failures and not verdict_divergences and not intent_mismatches
    print()
    print("VERDICT:", "PASS - pinned email regex is identical across all 7 engines"
          if ok else "FAIL - see divergences/mismatches above")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

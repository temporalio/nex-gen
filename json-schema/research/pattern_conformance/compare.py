#!/usr/bin/env python3
"""Run all five engines against corpus.json, align by pair id, and report:

  (a) Compile-acceptance check: any pattern the Rust GATE accepts but a runtime
      engine (Go/JS/Python/Java) fails to compile. This is the critical
      "Rust-accepted subset of every runtime engine" property.

  (b) Match-agreement check: any {pattern, instance} where the four runtime
      engines disagree on the boolean match result.

Prints a summary and exits nonzero if any divergence is found.

Run: python3 compare.py
(assumes go/node/python3/java/cargo on PATH; must be run from this directory,
or from anywhere -- paths are resolved relative to this file.)
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE / "corpus.json"

RUNTIME_ENGINES = ["go", "js", "python", "java"]


def run(cmd, cwd=None):
    proc = subprocess.run(
        cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8"
    )
    if proc.returncode != 0:
        sys.stderr.write(f"command failed: {cmd}\n{proc.stderr}\n")
        sys.exit(1)
    return proc.stdout


def parse_lines(text):
    """JSON Lines -> {id: record}."""
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
    gate_rejected = []
    gate_accepted = []
    for pid, p in pairs.items():
        if results["rust"][pid]["compiled"]:
            gate_accepted.append(pid)
        else:
            gate_rejected.append(pid)

    # --- Check gate-reject expectations ---
    gate_expectation_problems = []
    for pid, p in pairs.items():
        expect_reject = p.get("expect_gate_reject", False)
        actually_rejected = not results["rust"][pid]["compiled"]
        if expect_reject != actually_rejected:
            gate_expectation_problems.append(
                (pid, p["pattern"], expect_reject, actually_rejected)
            )

    # --- (a) Compile-acceptance: Rust-accepted subset of each runtime ---
    compile_violations = []  # (pid, pattern, engine)
    for pid in gate_accepted:
        for eng in RUNTIME_ENGINES:
            if not results[eng][pid]["compiled"]:
                compile_violations.append((pid, pairs[pid]["pattern"], eng))

    # --- (b) Match-agreement across the four runtimes (gate-accepted only) ---
    match_divergences = []  # (pid, pattern, instance, {eng: matched})
    for pid in gate_accepted:
        # skip if any runtime failed to compile (already flagged in (a))
        if any(not results[eng][pid]["compiled"] for eng in RUNTIME_ENGINES):
            continue
        vals = {eng: results[eng][pid]["matched"] for eng in RUNTIME_ENGINES}
        if len(set(vals.values())) > 1:
            match_divergences.append(
                (pid, pairs[pid]["pattern"], pairs[pid]["instance"], vals)
            )

    agreements = len(gate_accepted) - len(compile_violations) - len(match_divergences)

    # ---------------- report ----------------
    print("=" * 72)
    print("PATTERN CONFORMANCE REPORT")
    print("=" * 72)
    print(f"total pairs:          {total}")
    print(f"gate-rejected (Rust): {len(gate_rejected)}  {sorted(gate_rejected)}")
    print(f"gate-accepted (Rust): {len(gate_accepted)}")
    print()

    print("--- gate expectation check (expect_gate_reject vs actual) ---")
    if not gate_expectation_problems:
        print("  OK: every pattern's gate outcome matched its expectation.")
    else:
        for pid, pat, exp, act in gate_expectation_problems:
            print(f"  MISMATCH {pid}: pattern={pat!r} expected_reject={exp} actual_reject={act}")
    print()

    print("--- (a) compile-acceptance: Rust-accepted must compile everywhere ---")
    if not compile_violations:
        print("  OK: every gate-accepted pattern compiled in Go, JS, Python, and Java.")
    else:
        for pid, pat, eng in compile_violations:
            print(f"  VIOLATION {pid}: pattern={pat!r} FAILED to compile in {eng}")
    print()

    print("--- (b) match-agreement: four runtimes must agree ---")
    if not match_divergences:
        print("  OK: Go, JS, Python, and Java agreed on every gate-accepted pair.")
    else:
        for pid, pat, inst, vals in match_divergences:
            print(f"  DIVERGENCE {pid}:")
            print(f"    pattern  = {pat!r}")
            print(f"    instance = {inst!r}")
            for eng in RUNTIME_ENGINES:
                print(f"      {eng:7} matched = {vals[eng]}")
    print()

    print("--- summary ---")
    print(f"  gate-accepted pairs fully agreeing: {agreements}/{len(gate_accepted)}")
    print(f"  compile-acceptance violations:      {len(compile_violations)}")
    print(f"  match divergences:                  {len(match_divergences)}")
    print(f"  gate expectation problems:          {len(gate_expectation_problems)}")

    ok = (
        not compile_violations
        and not match_divergences
        and not gate_expectation_problems
    )
    print()
    print("VERDICT:", "PASS - gate + pinned semantics are identical across runtimes"
          if ok else "FAIL - see divergences above")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Run the pinned `duration` regex through all SEVEN target regex engines
against corpus.json and report:

  (a) Compile-acceptance: does the single pinned regex compile in every engine?
      (It is generator-authored and RE2-safe, so the Rust `regex` gate accepting
      it should imply every runtime accepts it.)

  (b) Cross-engine match agreement: for every corpus value, do all seven engines
      return the SAME match verdict? (This is the P1 bar.)

  (c) Correctness: does that shared verdict equal the corpus `expect` (the
      RFC 3339 Appendix A ABNF answer)?

Engines: Rust (`regex` crate = the gate + the generator's own engine), Go, JS
(node), Python, Java, Ruby, .NET. Java/Ruby/.NET apply the same per-target `$`
-> end-of-input-anchor normalization the generator would emit (see each runner).

Prints a summary and exits nonzero on any divergence or any wrong verdict.

Run: python3 compare.py
(assumes go/node/python3/java/ruby/dotnet/cargo on PATH.)
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
    cases = {c["id"]: c for c in corpus["cases"]}

    print("Building rust runner (cargo build --release)...", file=sys.stderr)
    run(["cargo", "build", "--release", "--quiet"], cwd=HERE / "rust_runner")
    rust_bin = HERE / "rust_runner" / "target" / "release" / "duration_runner"

    results = {
        "rust": parse_lines(run([str(rust_bin), str(CORPUS)])),
        "go": parse_lines(run(["go", "run", str(HERE / "runner.go"), str(CORPUS)])),
        "js": parse_lines(run(["node", str(HERE / "runner.mjs"), str(CORPUS)])),
        "python": parse_lines(run(["python3", str(HERE / "runner.py"), str(CORPUS)])),
        "java": parse_lines(run(["java", str(HERE / "Runner.java"), str(CORPUS)])),
        "ruby": parse_lines(run(["ruby", str(HERE / "runner.rb"), str(CORPUS)])),
        "dotnet": parse_lines(run(
            ["dotnet", "run", "--project", str(HERE / "dotnet_runner" / "DurationRunner"),
             "-c", "Release", "--", str(CORPUS)])),
    }

    # --- (a) compile-acceptance ---
    compile_failures = [eng for eng in ENGINES
                        if not all(results[eng][cid]["compiled"] for cid in cases)]

    # --- (b) cross-engine match agreement + (c) correctness ---
    divergences = []      # (cid, value, {eng: matched})
    wrong_verdicts = []   # (cid, value, shared_verdict, expect)
    for cid, c in cases.items():
        vals = {eng: results[eng][cid]["matched"] for eng in ENGINES}
        if len(set(v for v in vals.values() if v is not None)) > 1 or None in vals.values():
            divergences.append((cid, c["value"], vals))
            continue
        shared = next(iter(vals.values()))
        if shared != c["expect"]:
            wrong_verdicts.append((cid, c["value"], shared, c["expect"]))

    total = len(cases)
    agreeing = total - len(divergences)

    print("=" * 72)
    print("DURATION FORMAT CONFORMANCE REPORT")
    print("=" * 72)
    print(f"pinned regex: {corpus['pinned_regex']}")
    print(f"total values: {total}   engines: {ENGINES}")
    print()

    print("--- (a) compile-acceptance: pinned regex must compile in every engine ---")
    if not compile_failures:
        print("  OK: the pinned regex compiled in all seven engines.")
    else:
        for eng in compile_failures:
            print(f"  FAIL: pinned regex did not compile in {eng}")
    print()

    print("--- (b) cross-engine match agreement (the P1 bar) ---")
    if not divergences:
        print("  OK: all seven engines returned the same verdict for every value.")
    else:
        for cid, value, vals in divergences:
            print(f"  DIVERGENCE {cid}: value={value!r}")
            for eng in ENGINES:
                print(f"      {eng:7} matched = {vals[eng]}")
    print()

    print("--- (c) correctness: shared verdict must equal the ABNF `expect` ---")
    if not wrong_verdicts:
        print("  OK: the shared verdict equals `expect` for every value.")
    else:
        for cid, value, shared, expect in wrong_verdicts:
            print(f"  WRONG {cid}: value={value!r} shared={shared} expected={expect}")
    print()

    print("--- summary ---")
    print(f"  values agreeing across engines: {agreeing}/{total}")
    print(f"  compile failures:               {len(compile_failures)}")
    print(f"  match divergences:              {len(divergences)}")
    print(f"  wrong verdicts:                 {len(wrong_verdicts)}")

    ok = not compile_failures and not divergences and not wrong_verdicts
    print()
    print("VERDICT:", "PASS - one pinned regex, identical & correct across all 7 targets"
          if ok else "FAIL - see above")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Run all SEVEN engines against corpus.json, align by case id, and report:

  (a) Agreement with the PINNED verdict: for every case, does each engine's
      `valid` equal the corpus `valid`? The pinned check is a single anchored
      RE2-safe regex + a total-length guard (1..=253 code points), implemented
      identically in every target.
  (b) Cross-engine agreement: do all seven engines produce the same `valid`?

Engines: rust (the generator's own regex crate = gate proof), go, js, python,
java, ruby, dotnet. Exits nonzero on any disagreement.

Run: python3 compare.py
(assumes cargo/go/node/python3/java/ruby/dotnet on PATH.)
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
        sys.stderr.write(f"command failed: {cmd}\n{proc.stdout}\n{proc.stderr}\n")
        sys.exit(1)
    return proc.stdout


def parse_lines(text):
    out = {}
    for line in text.splitlines():
        line = line.strip()
        if line:
            rec = json.loads(line)
            out[rec["id"]] = rec
    return out


def main():
    corpus = json.loads(CORPUS.read_text(encoding="utf-8"))
    cases = {c["id"]: c for c in corpus["cases"]}

    print("Building rust runner (cargo build --release)...", file=sys.stderr)
    run(["cargo", "build", "--release", "--quiet"], cwd=HERE / "rust_runner")
    rust_bin = HERE / "rust_runner" / "target" / "release" / "rust_runner"

    print("Building dotnet runner...", file=sys.stderr)
    run(["dotnet", "build", "-c", "Release", "--nologo", "-v", "q"],
        cwd=HERE / "dotnet_runner" / "HostnameRunner")

    results = {
        "rust": parse_lines(run([str(rust_bin), str(CORPUS)])),
        "go": parse_lines(run(["go", "run", str(HERE / "runner.go"), str(CORPUS)])),
        "js": parse_lines(run(["node", str(HERE / "runner.mjs"), str(CORPUS)])),
        "python": parse_lines(run(["python3", str(HERE / "runner.py"), str(CORPUS)])),
        "java": parse_lines(run(["java", str(HERE / "Runner.java"), str(CORPUS)])),
        "ruby": parse_lines(run(["ruby", str(HERE / "runner.rb"), str(CORPUS)])),
        "dotnet": parse_lines(run(
            ["dotnet", "run", "-c", "Release", "--no-build", "--project",
             str(HERE / "dotnet_runner" / "HostnameRunner"), "--", str(CORPUS)])),
    }

    total = len(cases)

    # (a) each engine vs the pinned verdict
    pinned_mismatches = []  # (id, engine, expected, got)
    for cid, c in cases.items():
        want = c["valid"]
        for eng in ENGINES:
            got = results[eng][cid]["valid"]
            if got != want:
                pinned_mismatches.append((cid, eng, want, got))

    # (b) cross-engine agreement
    cross_divergences = []  # (id, {eng: valid})
    for cid in cases:
        vals = {eng: results[eng][cid]["valid"] for eng in ENGINES}
        if len(set(vals.values())) > 1:
            cross_divergences.append((cid, vals))

    print("=" * 72)
    print("HOSTNAME FORMAT CONFORMANCE REPORT")
    print("=" * 72)
    print(f"total cases: {total}")
    print(f"engines:     {', '.join(ENGINES)}")
    print()

    print("--- (a) each engine vs the PINNED verdict ---")
    if not pinned_mismatches:
        print("  OK: every engine matched the pinned `valid` for every case.")
    else:
        for cid, eng, want, got in pinned_mismatches:
            print(f"  MISMATCH {cid}: {eng} got valid={got}, pinned={want}  "
                  f"instance={cases[cid]['instance']!r}")
    print()

    print("--- (b) cross-engine agreement (all seven agree) ---")
    if not cross_divergences:
        print("  OK: all seven engines agreed on every case.")
    else:
        for cid, vals in cross_divergences:
            print(f"  DIVERGENCE {cid}: instance={cases[cid]['instance']!r}")
            for eng in ENGINES:
                print(f"      {eng:7} valid = {vals[eng]}")
    print()

    print("--- summary ---")
    print(f"  cases fully agreeing with pinned + across engines: "
          f"{total - len({m[0] for m in pinned_mismatches} | {d[0] for d in cross_divergences})}/{total}")
    print(f"  pinned mismatches:   {len(pinned_mismatches)}")
    print(f"  cross-divergences:   {len(cross_divergences)}")

    ok = not pinned_mismatches and not cross_divergences
    print()
    print("VERDICT:", "PASS - pinned hostname check is identical across all seven targets"
          if ok else "FAIL - see divergences above")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

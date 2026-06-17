#!/usr/bin/env python3
"""Compare the .NET (C#) runner's pinned `format` check against the corpus and
against the four current targets (Go, JS, Python, Java), to establish whether
.NET is a future-conformant target for the asserted-v1 `format` subset.

The pinned check is generator-owned (pinned regex + calendar arithmetic), so .NET
should agree value-for-value. The only .NET-specific pinning is anchoring: .NET's
`$` matches before a trailing `\\n`, so the pinned pattern uses `\\A`/`\\z`
(start/strict-end of input) -- exactly what Program.cs encodes. Explicit char
classes ([0-9] etc.) sidestep the Unicode-vs-ASCII `\\d` distinction.

Run: python3 compare_dotnet.py
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PC = HERE.parent            # format_conformance/
CORPUS = PC / "corpus.json"
REF_ENGINES = ["go", "js", "python", "java"]


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

    ref = {
        "go": parse_lines(run(["go", "run", str(PC / "runner.go"), str(CORPUS)])),
        "js": parse_lines(run(["node", str(PC / "runner.mjs"), str(CORPUS)])),
        "python": parse_lines(run(["python3", str(PC / "runner.py"), str(CORPUS)])),
        "java": parse_lines(run(["java", str(PC / "Runner.java"), str(CORPUS)])),
    }
    dotnet = parse_lines(run(
        ["dotnet", "run", "--project", str(HERE / "DotnetRunner"), "-c", "Release", "--", str(CORPUS)]
    ))

    corpus_disagree = []
    ref_disagree = []
    native_notes = []
    for pid, p in pairs.items():
        exp = p["expect_valid"]
        dv = dotnet[pid]["valid"]
        if dv != exp:
            corpus_disagree.append((pid, exp, dv))
        ref_vals = {e: ref[e][pid]["valid"] for e in REF_ENGINES}
        ref_val = next(iter(set(ref_vals.values()))) if len(set(ref_vals.values())) == 1 else None
        if ref_val is None or dv != ref_val:
            ref_disagree.append((pid, ref_vals, dv))
        nat = dotnet[pid].get("native")
        if nat is not None and nat != dv:
            native_notes.append((pid, p["format"], p["value"], dv, nat))

    print("=" * 72)
    print(".NET (C#) FORMAT CONFORMANCE vs corpus + 4-engine reference")
    print("=" * 72)
    print(f"total pairs: {len(pairs)}")
    print()

    print("--- corpus agreement (.NET pinned check vs expect_valid) ---")
    if not corpus_disagree:
        print("  OK: .NET agreed with the corpus on every pair.")
    else:
        for pid, exp, dv in corpus_disagree:
            p = pairs[pid]
            print(f"  DISAGREE {pid} [{p['format']}]: expect={exp} dotnet={dv} value={p['value']!r}")
    print()

    print("--- cross-engine agreement (.NET vs go/js/python/java) ---")
    if not ref_disagree:
        print("  OK: .NET agreed with all four current targets on every pair.")
    else:
        for pid, refv, dv in ref_disagree:
            p = pairs[pid]
            print(f"  DIVERGENCE {pid} [{p['format']}] value={p['value']!r}: ref={refv} dotnet={dv}")
    print()

    print("--- (informational) .NET native parser vs pinned check ---")
    if not native_notes:
        print("  (none)")
    else:
        print("  Documentation only -- the pinned check is the verdict:")
        for pid, fmt, val, pinned, nat in native_notes:
            print(f"    {pid} [{fmt}]: pinned={pinned} native={nat} value={val!r}")
    print()

    ok = not corpus_disagree and not ref_disagree
    print("VERDICT:", "PASS - .NET is future-conformant with the owned check"
          if ok else "FAIL - see divergences above")
    sys.exit(0 if ok else 2)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Run all 7 NATIVE URI parsers against native_inputs.json, align by id, and
report per-input the boolean "valid absolute URI" verdict of each engine, and
count how many inputs the 7 native parsers UNANIMOUSLY agree on.

The point: native parsers diverge wildly, so none is usable as a P1 oracle.

Run: python3 compare_native.py
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
INPUTS = HERE.parent / "native_inputs.json"

ENGINES = [
    ("go",     ["go", "run", str(HERE / "native.go"), str(INPUTS)]),
    ("js",     ["node", str(HERE / "native.mjs"), str(INPUTS)]),
    ("python", ["python3", str(HERE / "native.py"), str(INPUTS)]),
    ("java",   ["java", str(HERE / "NativeProbe.java"), str(INPUTS)]),
    ("ruby",   ["ruby", str(HERE / "native.rb"), str(INPUTS)]),
    ("dotnet", ["dotnet", "run", "--project", str(HERE / "DotnetNative"), "-c", "Release", "--", str(INPUTS)]),
]


def run(cmd):
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8")
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
    corpus = json.loads(INPUTS.read_text(encoding="utf-8"))
    inputs = corpus["inputs"]
    ids = [i["id"] for i in inputs]

    results = {}
    for name, cmd in ENGINES:
        print(f"running {name}...", file=sys.stderr)
        results[name] = parse_lines(run(cmd))

    names = [n for n, _ in ENGINES]

    print("=" * 100)
    print("NATIVE URI-PARSER DIVERGENCE (valid absolute URI? per engine)")
    print("=" * 100)
    header = f"{'id':28} " + " ".join(f"{n[:6]:>6}" for n in names)
    print(header)
    print("-" * len(header))

    unanimous = 0
    divergent_ids = []
    for i in inputs:
        pid = i["id"]
        verdicts = {n: results[n][pid]["valid"] for n in names}
        vals = set(verdicts.values())
        row = f"{pid:28} " + " ".join(f"{('T' if verdicts[n] else 'F'):>6}" for n in names)
        mark = "" if len(vals) == 1 else "  <-- DIVERGE"
        if len(vals) == 1:
            unanimous += 1
        else:
            divergent_ids.append(pid)
        print(row + mark)

    print("-" * len(header))
    print(f"total inputs:          {len(ids)}")
    print(f"unanimous (all 7 agree): {unanimous}")
    print(f"divergent:             {len(divergent_ids)}")
    print(f"divergent ids: {divergent_ids}")


if __name__ == "__main__":
    main()

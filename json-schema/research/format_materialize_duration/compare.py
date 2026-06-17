#!/usr/bin/env python3
"""Cross-language byte-equality harness for duration materialization.

Runs every language's emitter, which parses each corpus `wire` into the
language's materialized construct and re-serializes it via the CANONICAL form:
  - `full` group    -> design B custom component struct (all 6 model langs + Ruby/.NET)
  - `timeonly` group -> design C native fixed-duration type where one exists
                        (Go time.Duration, Java java.time.Duration, Python
                        timedelta, .NET TimeSpan); JS/Ruby have none so they
                        use the same total-based canonical by hand.

Pass condition: for every corpus id, ALL languages that materialize emit the
byte-identical string (P1). Run: python3 compare.py
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).parent

EMITTERS = {
    "go":     ["go", "run", "."],                       # cwd emit_go
    "python": [sys.executable, "emit.py"],               # cwd .
    "js":     ["node", "emit.mjs"],                       # cwd .
    "ruby":   ["ruby", "emit.rb"],                        # cwd .
    "java":   ["java", "Emit.java"],                      # cwd emit_java
    "dotnet": ["dotnet", "run", "--project", "emit_cs/EmitDur"],  # cwd .
}
CWD = {
    "go": HERE / "emit_go",
    "java": HERE / "emit_java",
}


def run(lang):
    cwd = CWD.get(lang, HERE)
    r = subprocess.run(EMITTERS[lang], cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        print(f"[{lang}] FAILED:\n{r.stderr}", file=sys.stderr)
        return None
    # dotnet may print build noise; take the last non-empty line that is JSON
    for line in reversed(r.stdout.strip().splitlines()):
        line = line.strip()
        if line.startswith("{"):
            return json.loads(line)
    print(f"[{lang}] no JSON line in output:\n{r.stdout}", file=sys.stderr)
    return None


def main():
    corpus = json.loads((HERE / "corpus.json").read_text())
    results = {}
    for lang in EMITTERS:
        out = run(lang)
        if out is not None:
            results[lang] = out
    langs = list(results.keys())
    print(f"Emitters that ran: {langs}\n")

    ok = True
    for group in ("full", "timeonly"):
        print(f"=== group: {group} ===")
        for row in corpus[group]:
            gid, wire = row["id"], row["wire"]
            vals = {l: results[l][group][gid] for l in langs if group in results[l] and gid in results[l][group]}
            distinct = set(vals.values())
            agree = len(distinct) == 1
            ok = ok and agree
            mark = "OK " if agree else "MISMATCH"
            shown = next(iter(distinct)) if agree else vals
            print(f"  [{mark}] {gid:20} {wire:30} -> {shown}")
        print()

    print("BYTE-EQUAL ACROSS ALL MATERIALIZING LANGUAGES:", ok)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()

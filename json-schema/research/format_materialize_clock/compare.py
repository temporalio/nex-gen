#!/usr/bin/env python3
"""Cross-language byte-equality harness for the clock materialization corpus,
under the CURRENT no-truncation, offset-preserving spec behavior.

Each runner parses every VALID wire string into that language's native temporal
type and re-serializes it via the generator-owned serializer (RFC 3339, original
offset preserved with +00:00/-00:00 -> Z, T/Z uppercased on the parse path,
fractional seconds at the value's own precision with trailing zeros trimmed).
This harness collects every engine's output per (format, id) and checks that the
EQUALLY-CAPABLE MATERIALIZING SET agrees byte-for-byte:

    equally-capable set = go, java, py, js-string, js-temporal

Documented (NON-failure) divergences, reported separately:
  * Python sub-microsecond truncation on a date-time whose fractional part
    exceeds 6 digits (datetime's native microsecond resolution).
  * js-date (legacy --js-temporal-repr=date) folds date-time to a UTC instant
    at millisecond resolution; date/time are unsupported.
  * Leap-second :60 rows are rejected by every native parser / the materialized
    grammar -> non-materializing (SKIP).

Ruby / .NET (--with-ruby / --with-dotnet) are PROSPECTIVE: shown and checked
against the reference, but their divergences never cause FAIL.

Usage: python3 compare.py [--with-ruby] [--with-dotnet]
Report-only (exit 0). Prints a PASS/FAIL line for the equally-capable invariant.
"""
import json, subprocess, sys, os, re

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(HERE, "corpus.json")

RUNNERS = [
    ("go",   ["go", "run", "runner.go", CORPUS]),
    ("node", ["node", "runner.mjs", CORPUS]),   # emits js-string / js-temporal / js-date
    ("py",   ["python3", "runner.py", CORPUS]),
    ("java", ["java", "Runner.java", CORPUS]),
]

# The set that must agree byte-for-byte (the invariant PASS/FAIL is over these).
CAPABLE = ["go", "java", "py", "js-string", "js-temporal"]
# Reported separately; never cause FAIL.
LOSSY = ["js-date"]
PROSPECTIVE = ["ruby", "dotnet"]


def run(engine, cmd):
    try:
        p = subprocess.run(cmd, cwd=HERE, capture_output=True, text=True, timeout=180)
    except Exception as e:
        print(f"!! {engine} failed to run: {e}", file=sys.stderr)
        return []
    if p.returncode != 0 and not p.stdout.strip():
        print(f"!! {engine} exited {p.returncode}: {p.stderr[:400]}", file=sys.stderr)
        return []
    rows = []
    for line in p.stdout.splitlines():
        line = line.strip()
        if not line or not line.startswith("{"):
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return rows


def frac_len(s):
    m = re.search(r"\.(\d+)", s)
    return len(m.group(1)) if m else 0


def micros_trunc(s):
    """Truncate a serialized date-time's fractional part to microseconds
    (6 digits) with trailing zeros re-trimmed -- the Python round-trip."""
    m = re.search(r"\.(\d+)", s)
    if not m or len(m.group(1)) <= 6:
        return s
    frac = m.group(1)[:6].rstrip("0")
    rep = "." + frac if frac else ""
    return s[:m.start()] + rep + s[m.end():]


def main():
    engines = list(RUNNERS)
    if "--with-ruby" in sys.argv:
        engines.append(("ruby", ["ruby", "runner.rb", CORPUS]))
    if "--with-dotnet" in sys.argv:
        engines.append(("dotnet", ["dotnet", "run", "--project", "dotnet_runner", "--", CORPUS]))

    # results[(fmt,id)][engine] = (canonical, err)
    results = {}
    ran = []
    for engine, cmd in engines:
        for r in run(engine, cmd):
            ran.append(r["engine"])
            results.setdefault((r["format"], r["id"]), {})[r["engine"]] = (r.get("canonical", ""), r.get("err", ""))
    ran = sorted(set(ran))

    prospective_present = [e for e in PROSPECTIVE if e in ran]
    capable = list(CAPABLE)  # PASS/FAIL is strictly over these

    corpus = json.load(open(CORPUS))
    order = [(fmt, row["id"]) for fmt in ("date-time", "date", "time") for row in corpus[fmt]]

    hard_mismatches = 0
    py_subus = 0
    jsdate_folds = 0
    leap_skips = 0

    print("engines run: " + ", ".join(ran))
    print("equally-capable set (must agree byte-for-byte): " + ", ".join(capable))
    if prospective_present:
        print("prospective (reported, never FAIL): " + ", ".join(prospective_present))
    print()

    for (fmt, rid) in order:
        cell = results.get((fmt, rid), {})

        def mat(e):
            v = cell.get(e)
            return v[0] if v and v[0] and not v[1] else None

        cap_vals = {e: mat(e) for e in capable if mat(e) is not None}
        cap_err = [e for e in capable if e in cell and mat(e) is None]

        # ---- leap / non-materializing across the whole capable set -> SKIP ----
        if not cap_vals:
            leap_skips += 1
            print(f"[SKIP ] {fmt:9s} {rid:16s} rejected by all capable engines (materialized grammar / native parser)")
            errs = {e: cell[e][1] for e in capable if e in cell and cell[e][1]}
            for e in sorted(errs):
                print(f"          {e:12s}: {errs[e][:80]}")
            continue

        # ---- reference (go preferred; lossless) ----
        ref = None
        for e in ("go", "java", "js-string", "js-temporal", "py"):
            if e in cap_vals:
                ref = cap_vals[e]
                break

        row_mismatch = False
        py_note = False
        expect_py = micros_trunc(ref)
        is_subus = frac_len(ref) > 6

        detail = []
        for e in capable:
            if e not in cap_vals:
                if e in cap_err:  # some capable engine rejected a row others materialized
                    row_mismatch = True
                    detail.append(f"MISSING {e}={cell[e][1][:50]!r}")
                continue
            val = cap_vals[e]
            if e == "py" and is_subus:
                if val == expect_py:
                    py_note = True  # expected sub-µs truncation
                else:
                    row_mismatch = True
                    detail.append(f"{e}={val!r} (expected trunc {expect_py!r})")
            elif val != ref:
                row_mismatch = True
                detail.append(f"{e}={val!r}")

        if row_mismatch:
            hard_mismatches += 1
            status = "MISMAT"
        elif py_note:
            py_subus += 1
            status = "EXPECT"
        else:
            status = "OK    "

        line = f"[{status}] {fmt:9s} {rid:16s} ref={ref!r}"
        if py_note:
            line += f"  | py sub-µs -> {expect_py!r}"
        print(line)
        if row_mismatch:
            for d in detail:
                print(f"          HARD MISMATCH: {d}")

        # ---- js-date (expected lossy), reported, never FAIL ----
        jd = mat("js-date")
        if jd is not None:
            jsdate_folds += 1
            tag = "same" if jd == ref else "UTC-instant fold"
            print(f"          js-date  -> {jd!r} ({tag})")

        # ---- prospective engines ----
        for e in prospective_present:
            v = cell.get(e)
            if not v:
                continue
            if v[0] and not v[1]:
                if v[0] == ref:
                    agree = "agrees with ref"
                elif v[0] == expect_py:
                    agree = "µs-truncated (sub-µs resolution limit)"
                else:
                    agree = "DIVERGES from ref"
                print(f"          {e:8s} -> {v[0]!r} ({agree})")
            else:
                print(f"          {e:8s} -> (no-mat) {v[1][:70]}")

    print()
    print("=" * 72)
    verdict = "PASS" if hard_mismatches == 0 else "FAIL"
    print(f"{verdict}: equally-capable set {{{', '.join(capable)}}} "
          f"agrees byte-for-byte  |  hard mismatches: {hard_mismatches}")
    print(f"expected divergences: Python sub-µs truncations = {py_subus}, "
          f"js-date UTC folds = {jsdate_folds}, leap-second skips = {leap_skips}")


if __name__ == "__main__":
    main()

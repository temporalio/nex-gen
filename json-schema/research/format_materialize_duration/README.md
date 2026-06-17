# format_materialize_duration

Empirical research: can the `duration` format be **materialized** as an idiomatic
in-memory language construct (instead of the current `string`), with consistent
cross-language re-serialization (P1)? Validation is unchanged — this only concerns
the emitted field **type** and the parse/serialize paths.

See **[NOTES.md](NOTES.md)** for the full findings, the feasibility matrices, the
design-option evaluation (A/B/C/D), and the recommendation. This README is just
the map + run instructions.

## TL;DR

- **No stdlib fixed-duration type in any target holds the full accepted grammar**
  (Y/M/W are calendar-variable; fixed ns/tick types can't store them).
- **Design B** (a generated component struct) round-trips the **full** grammar
  byte-identically across all six languages. **Design C** (native type) needs a
  **grammar narrowing** to `PTnHnMnS` and only reaches a native type in 4 of 6.
- **Recommendation:** keep `string` (A) by default; if materialization is
  demanded, ship **design B**, not C. Reject D (total-normalization loses Y/M).

## Layout

| Path | What |
|---|---|
| `corpus.json` | Wire strings (all valid per the pinned regex), grouped `full` / `timeonly` / `timeonly_noncanonical`. Documents the canonical serialization. |
| `go_full/`, `java_full/`, `py_full/`, `ts_full/`, `rb_full/`, `cs_full/` | Per-language walkthrough probes: Q1 (can a stdlib type hold the full grammar?), Q2 (design B struct round-trip), Q3 (design C native round-trip incl. non-canonical inputs). |
| `emit_go/`, `emit.py`, `emit.mjs`, `emit.rb`, `emit_java/`, `emit_cs/` | Machine-readable emitters: read `corpus.json`, materialize + canonical-serialize, print `{group:{id:output}}` JSON. |
| `compare.py` | Cross-language harness: runs every emitter and asserts byte-equal output per corpus id (the P1 proof). |

## Run

```
# per-language walkthroughs
cd go_full   && go run .        && cd ..
cd java_full && java Full.java  && cd ..
cd py_full   && python3 full.py && cd ..
cd ts_full   && node full.mjs   && cd ..
cd rb_full   && ruby full.rb    && cd ..
cd cs_full/DurRunner && dotnet run && cd ../..

# the proof: byte-equal re-serialization across all six languages
python3 compare.py     # -> "BYTE-EQUAL ACROSS ALL MATERIALIZING LANGUAGES: True"
```

Toolchains as-run: go 1.26, node v25, python3 3.13, java 21, ruby 2.6,
dotnet 8, rustc 1.88.

## Relationship to sibling research

- `../format_duration/` — the validation corpus (68 pairs, 7 engines agree) and
  `native_parsers_probe/` (native ISO-8601 *parsers* diverge). Reused, not redone.
- `../format_typed_repr/` — typed-representation feasibility for `uuid` / clock
  formats; did NOT cover `duration`. This directory fills that gap.
- `../format_materialize_clock/` — parallel materialize-or-string work for
  date / time / date-time; same harness shape. See NOTES "Consistency" section.

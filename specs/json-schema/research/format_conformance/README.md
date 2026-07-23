# `format` cross-language conformance

Empirical study answering the `format` spec's **open question 2** ("build the
format conformance corpus"): can the generator-**owned** `format` check — a
pinned portable regex plus a shared calendar predicate — be implemented
**identically** across every target, so that a value one language accepts is
accepted (and rejected) by all? This is the proof of **P1** (validation identical
across all languages) for the asserted-v1 `format` subset.

Modeled on `../pattern_conformance/`. Covers the six asserted-v1 formats:
`uuid`, `ipv4`, `ipv6`, `date`, `time`, `date-time`.

## What is under test — the pinned OWNED check

Each format lowers to a **generator-owned** check, never a native typed parser
(native date/UUID/IP parsers are the single most divergent corner of the JSON
Schema ecosystem — that is *why* the spec makes `format` assertion optional).
The owned check is:

1. A **pinned, fully-anchored, RE2-safe regex** (no lookaround, no
   backreferences), compiled once at module/package init. The regexes:

   | Format | Pinned regex (start/end anchors per target — see below) |
   |---|---|
   | `uuid` | `[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}` |
   | `ipv4` | dotted quad of `(25[0-5]\|2[0-4][0-9]\|1[0-9][0-9]\|[1-9][0-9]\|[0-9])` (no leading zeros) |
   | `ipv6` | RFC 4291 (full, `::`-compressed, IPv4-tail) |
   | `date` | `([0-9]{4})-([0-9]{2})-([0-9]{2})` |
   | `time` | `([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?([Zz]\|[+-][0-9]{2}:[0-9]{2})?` |
   | `date-time` | `([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?([Zz]\|[+-][0-9]{2}:[0-9]{2})` |

2. For the **temporal** formats, a **shared integer-arithmetic calendar
   predicate** (no date library): month `01–12`, day within the month's
   Gregorian length (leap-year Feb 29), plus the pinned RFC 3339 edge decisions
   (below). The regex enforces syntax; the predicate enforces the semantics a
   regex cannot.

### Pinned RFC 3339 edge decisions (from the spec)

- Leap second `:60` in the seconds field is **accepted syntactically**.
- `date-time` offset is **required** (`Z`/`z` or `±HH:MM`); `-00:00` is accepted.
- Fractional seconds at **any precision** (`.` + one or more digits).
- `T` / `Z` separators are **case-insensitive**.
- Calendar validity enforced for `date` and the date half of `date-time`.
- `time` **may be local** (offset optional) — RFC 3339 `partial-time`. See NOTES.

### The one portable pinning wrinkle — the end anchor

A raw `$` is **not** portable: in Python and Java (default flags) and .NET, `$`
matches *before a trailing `\n`*, so `"…uuid…\n"` would slip through, while Go/JS
`$` is strict end-of-input. The generator therefore emits a **strict
end-of-input anchor per target** — `\Z` (Python), `\z` (Java/Ruby/.NET/Rust),
`$` (Go/JS, already strict) — and a start anchor `\A` where `^` is a line anchor
(Ruby). With that single pinning every runtime agrees. (Identical in spirit to
the `$`-normalization `pattern_conformance` found.)

## Files

- `corpus.json` — 124 `{id, format, value, expect_valid}` pairs.
- `runner.go`, `runner.mjs`, `runner.py`, `Runner.java`, `runner.rb` — one per
  runtime. Each implements the pinned check and emits JSON Lines
  `{"id","engine","valid","native"}`. `valid` is the pinned-check verdict; the
  `native` column records what that language's **native typed parser** would say
  (documentation only — it never decides the verdict).
- `rust_runner/` — the Rust load-time **gate**: proves the pinned patterns
  compile in the pure-Rust `regex` crate, and also runs the full pinned check so
  its verdict is compared value-for-value. (`cargo clean` keeps the tree light.)
- `dotnet_runner/` — prospective .NET (C#) target + `compare_dotnet.py`.
- `compare.py` — runs Rust + Go/JS/Python/Java, checks (a) each engine vs the
  corpus `expect_valid` and (b) cross-runtime agreement. Exits nonzero on any
  disagreement.
- `compare_ruby.py` — Ruby vs corpus + the four current targets.
- `NOTES.md` — findings, the native-parser divergences, and proposed spec edits.

## Run

```sh
cd json-schema/research/format_conformance
python3 compare.py                       # rust gate + go/js/python/java
python3 compare_ruby.py                  # + ruby
python3 dotnet_runner/compare_dotnet.py  # + .NET
```

Each runner is also standalone, e.g. `go run runner.go corpus.json`,
`node runner.mjs corpus.json`, `python3 runner.py corpus.json`,
`java Runner.java corpus.json`, `ruby runner.rb corpus.json`, and (after
`cargo build --release`) `rust_runner/target/release/rust_runner corpus.json`.

## Findings (summary — full detail in NOTES.md)

**PASS across all seven runtimes.** 124 pairs; every runtime's pinned check
agrees with the corpus `expect_valid` **and** with every other runtime, on all
124 pairs. The Rust gate accepts (compiles) every pinned pattern. The owned
check delivers the P1 guarantee for the asserted-v1 `format` subset — including
prospective Ruby and .NET.

The **native typed parsers diverge widely** from the pinned check (110 recorded
divergences across Go/JS/Python/Java, plus more for Ruby/.NET) — e.g. JS `Date`
accepts `2021-02-30`, a missing offset, and `2021-01-15T12:30Z`; Go `time.Parse`
rejects lowercase `t`/`z` and leap seconds; .NET `IPAddress.TryParse` accepts
`01.2.3.4`, `1.2.3`, `0x1.2.3.4`, and a zone id. This is exactly the
cross-implementation chaos the owned check exists to avoid, and the empirical
justification for **not** delegating to native parsers.

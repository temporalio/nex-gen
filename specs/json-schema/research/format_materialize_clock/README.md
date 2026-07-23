# Materializing `date` / `time` / `date-time` as language constructs

Empirical corpus for `features/format` **Materialization (type mapping)**: can
the three **temporal** formats be **materialized** — emitted as an idiomatic
in-memory typed construct in the generated model (Go `time.Time`, Java
`OffsetDateTime`/`OffsetTime`/`LocalTime`/`LocalDate`, Python
`datetime`/`date`/`time`, TS `string` / `Temporal.*` / legacy `Date`) instead of
a bare `string` — while keeping the **wire bytes byte-identical across every
equally-capable materializing language** (PRINCIPLES **P1**)?

This corpus proves the **CURRENT** spec behavior, which is **no truncation**:

> Preserve the original UTC offset and the full sub-second precision to the
> extent each native type can hold them. Loss happens **only** at a type's
> genuine capacity limit, never as an artificial common-denominator floor.

(It replaces an earlier probe that proved a now-obsolete UTC-normalized,
millisecond-floored, offset-dropping canonical form — see the git history / the
NOTES "What changed" section.)

## Generator-owned serialized form

Every runner parses the validated wire string into its native construct and
**re-serializes** via a **generator-owned serializer** (NOT the language's
native `toString`/`Format`, which disagree on fractional-zero trimming — see
NOTES §2):

- **RFC 3339**, the **original offset preserved**, with `+00:00` / `-00:00`
  normalized to `Z`.
- `T` / `Z` **uppercased on the parse path** (the pinned grammar accepts
  lowercase; Go/Python/Ruby native parsers reject it).
- fractional seconds at the **value's own precision** with **trailing zeros
  trimmed** (`.250`→`.25`, `.500`→`.5`, `.120`→`.12`); **no fractional part when
  zero**.
- **`time` keeps its offset** when present (RFC 3339 makes it optional; an
  offset-less value stays offset-less).

## Per-target round-trip fidelity (what the corpus proves)

| Target | Type | date-time round-trip |
|---|---|---|
| Go | `time.Time` | offset + **nanosecond** preserved — lossless |
| Java | `OffsetDateTime` | offset + **nanosecond** preserved — lossless |
| Python | `datetime` (aware) | offset preserved; **sub-µs truncated** (native microsecond resolution) |
| TS `js-string` (default) | `string` | generator-serialized string — lossless |
| TS `js-temporal` | `Temporal.ZonedDateTime` | offset + **nanosecond** preserved — lossless |
| TS `js-date` (legacy) | `Date` | UTC **instant** at millisecond — offset folded to UTC, sub-ms lost (**LOSSY, expected**) |
| Ruby\* | `DateTime` | offset + nanosecond preserved (Rational) — lossless |
| .NET\* | `DateTimeOffset` | offset preserved; **100-ns tick** resolution (rounds a 9-digit input to 7) |

`date` (`YYYY-MM-DD`) and `time` (offset preserved) round-trip **losslessly**
everywhere their type exists. `\*` = prospective.

**The equally-capable set** — the languages that must agree **byte-for-byte** —
is **`go`, `java`, `py`, `js-string`, `js-temporal`**. The harness's PASS/FAIL
is over exactly this set. Documented, non-failure divergences reported
separately: Python sub-µs truncation, the `js-date` UTC fold, and the
leap-second skip.

## The three JS representations (`--js-temporal-repr`)

The single `runner.mjs` models all three modes as separate harness engines:

- **`js-string`** (default) — every temporal is the generator-serialized
  `string`; must match Go/Java/Python. Built via `Temporal.ZonedDateTime` for
  full precision, then serialized with the generator rules.
- **`js-temporal`** — `Temporal.ZonedDateTime` (date-time), `Temporal.PlainDate`
  (date); **`time` stays a `string`** (Temporal has no offset-bearing time-only
  type). Lossless.
- **`js-date`** — legacy `Date`, **date-time only**; `date`/`time` unsupported.
  UTC-instant fold at millisecond — the one lossy TS mode (reported separately,
  never a failure).

Temporal is **not** a Node global in this build (Node v25; the `--harmony-temporal`
flag does not expose it either), so the runner uses the **`@js-temporal/polyfill`**
installed locally (`npm install` in this dir).

## Files

- `corpus.json` — validated wire strings spanning the axes: offset ±, `+00:00`,
  `-00:00`, lowercase `t`/`z`, fractional 1/3/6/9 digits, trailing-zero trim,
  nanosecond with offset, midnight, leap `:60`; `date` bounds; `time`
  local/offset/frac/`Z`/`±00:00`.
- `runner.go`, `runner.mjs`, `runner.py`, `Runner.java` — the 4 model targets
  (`runner.mjs` emits the three `js-*` engines).
- `runner.rb`, `dotnet_runner/` — prospective Ruby / .NET.
- `compare.py` — runs the runners, collects each emitted serialized string, and
  asserts the equally-capable set agrees byte-for-byte; reports the documented
  divergences separately. Report-only (exit 0) with a PASS/FAIL summary line.

## Run

```sh
cd json-schema/research/format_materialize_clock
npm install                              # once, for the js-temporal polyfill
python3 compare.py                       # go / node(js-string,js-temporal,js-date) / python / java
python3 compare.py --with-ruby --with-dotnet
```

Each runner is standalone: `go run runner.go corpus.json`,
`node runner.mjs corpus.json`, `python3 runner.py corpus.json`,
`java Runner.java corpus.json`, `ruby runner.rb corpus.json`,
`dotnet run --project dotnet_runner -- corpus.json`.

Toolchains as-run: go 1.26, node v25.2 (+ `@js-temporal/polyfill`), python 3.13,
java 21, ruby 2.6, dotnet 8.

## Result (summary — full detail + the harness transcript in NOTES.md)

- **`date-time`, `date`, and `time` materialize with ZERO byte mismatches**
  across the equally-capable set (`go`, `java`, `py`, `js-string`, `js-temporal`)
  for every non-leap row — offset **preserved** (`+02:00` stays `+02:00`),
  `+00:00`/`-00:00`→`Z`, lowercase `t`/`z` uppercased, trailing fractional zeros
  trimmed, and full nanosecond precision on the `.123456789` rows in
  Go/Java/js-string/js-temporal.
- **Python truncates sub-microsecond** date-time (`.123456789`→`.123456`) — the
  one Python-side loss, exactly as the spec tabulates (2 rows).
- **`js-date` folds** every date-time to a UTC instant at millisecond (offset
  gone, 3 digits) — the expected legacy loss (16 rows).
- **Leap `:60`** is rejected by every native parser / the materialized grammar →
  non-materializing (SKIP, 2 rows).

### Spec discrepancies found (see NOTES §5)

- **`time` offset is preserved and lossless in TS-string / TS-temporal** — the
  spec text is correct, and the OLD notes claiming "`time` can't materialize in
  TS" are obsolete (it is a `string` in both modes, carrying the offset).
- **JS Temporal and Ruby SILENTLY CLAMP leap `:60`→`:59`** (not "reject", even
  with `{overflow:'reject'}`). The spec's "every native parser rejects `:60`"
  is inaccurate for these two; safety comes from the **validator** rejecting
  `:60` before the parse, which the runners model with an explicit guard.

[spec]: ../../features/format.md
[conf]: ../format_conformance/README.md

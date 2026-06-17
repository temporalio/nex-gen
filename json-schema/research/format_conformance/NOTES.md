# `format` conformance — findings

Toolchains: go, node v25, python3 3.13, java 21, ruby 2.6, dotnet 8,
cargo/rustc 1.88.

## Result

**All seven runtimes PASS.** 124 corpus pairs, six formats:

| format | pairs |
|---|---|
| uuid | 18 |
| ipv4 | 17 |
| ipv6 | 20 |
| date | 25 |
| time | 22 |
| date-time | 22 |

- Every runtime's **pinned check** agrees with the corpus `expect_valid` on all
  124 pairs (`compare.py`, `compare_ruby.py`, `compare_dotnet.py` all PASS).
- All seven runtimes agree **with each other** on all 124 pairs.
- The **Rust gate** compiles every pinned pattern (they are RE2-safe by
  construction: no lookaround, no backreferences).

This resolves `format` spec open question #2 and confirms **P1** for the
asserted-v1 subset — the owned check (pinned regex + integer-arithmetic calendar
predicate) is implementable identically in Go, TypeScript/JS, Python, Java, Rust,
Ruby, and .NET.

## The pinned check, encoded identically everywhere

1. **Regex** — same pattern source in every runner, anchored `\A…\z`-equivalent
   (see "End-anchor pinning" below). Explicit character classes (`[0-9]`,
   `[0-9a-fA-F]`) are used throughout, so the Unicode-vs-ASCII `\d` divergence
   that bit `pattern_conformance` **cannot arise here** — we never write `\d`.
2. **Calendar predicate** — the same three helpers in every language:
   `isLeap(y)`, `daysInMonth(y,m)`, and range checks `validCalendarDate`,
   `validTimeFields` (with `ss <= 60` for the accepted leap second),
   `validOffset` (offset hour ≤ 23, minute ≤ 59). Pure integer arithmetic; no
   date library.

## End-anchor pinning (the one portability adjustment)

A bare `$` is **not** portable as "end of string":

| Engine | `$` semantics | strict-end anchor used |
|---|---|---|
| Go (RE2) | end-of-text only | `$` (already strict; we use `\z`) |
| JS (`u`) | end-of-input only (no `m`) | `$` (already strict; we use `$`) |
| Python `re` | end **or before a trailing `\n`** | `\Z` |
| Java (default) | end **or before a final terminator** | `\z` |
| .NET | end **or before a trailing `\n`** | `\z` |
| Ruby (Onigmo) | line anchor (multiline always) | `\A…\z` |
| Rust `regex` | end-of-text only | `\z` |

Empirically confirmed: `re.match(r'^abc$', 'abc\n')` **matches** in Python but
`re.match(r'^abc\Z', 'abc\n')` does not. Without this pinning, the
`*-trailing-newline` / `*-trailing-space` corpus pairs would diverge (Python/
Java/.NET would wrongly accept a trailing newline). Each runner encodes the
strict-end anchor its engine needs; the generator would emit the per-target
anchor the same way. This mirrors the `$`-normalization finding in
`pattern_conformance`.

Ruby additionally needs `\A` for the start anchor (its `^` is a line anchor).

## Why NOT native parsers — the divergence evidence

The runners record a **secondary `native` column**: what each language's native
typed parser (`time.Parse`, `Date`, `datetime.fromisoformat`,
`OffsetDateTime.parse`, `Time.iso8601`, `IPAddress.TryParse`, …) says. It never
decides the verdict — it is pure documentation of *why the owned check exists*.
Selected divergences (pinned verdict is the correct one):

**JavaScript `Date` is famously lax** (accepts values the pinned check rejects):
- `2021-02-30T12:30:45Z` — calendar-invalid → `Date` accepts.
- `2021-02-29T00:00:00Z` — non-leap Feb 29 → `Date` accepts.
- `2021-01-15T12:30:45` — missing offset → `Date` accepts.
- `2021-01-15T12:30Z` — no seconds → `Date` accepts.
- `2021-01-15T24:00:00Z` — hour 24 → `Date` accepts.
- `2021/01/15`, `999-01-01`, `20210-01-01`, `2021-1-15`, `2021-04-31` (`date`) →
  `Date.parse` accepts all.

**Go `time.Parse(RFC3339)` is stricter than the pinned decisions** (rejects
values the pinned check accepts):
- lowercase `t`/`z` (`2021-01-15t12:30:45z`) → Go rejects; pinned accepts.
- leap second `23:59:60Z` / `2021-02-28T23:59:60Z` → Go rejects; pinned accepts.
- But `time.Parse` also *accepts* out-of-range offsets like `+24:00` / `+01:60`
  that the pinned check rejects.

**Python `datetime.fromisoformat`** accepts `12:30Z` (no seconds),
`12:30:45+0100` (no colon in offset), `12:30:45.Z` (empty fractional),
`2021-01-15 12:30:45Z` (space separator), and a bare `2021-01-15` as a
date-time — all rejected by the pinned check.

**Java `OffsetDateTime.parse`** accepts `12:30Z` and `12:30:45.Z` but rejects
leap seconds and very-high-precision fractions (`.123456789012`), the opposite
of the pinned decisions.

**.NET `IPAddress.TryParse` is very lax:** accepts `01.2.3.4` (leading zero),
`1.2.3` (3-part → treated as a 32-bit form), `0x1.2.3.4` (hex), and
`fe80::1%eth0` (zone id) — all rejected by the pinned ipv4/ipv6 regex.
`DateTimeOffset.TryParse` also accepts a trailing space and a missing offset.

**No stdlib UUID parser** in Go/JS/Python/Ruby/.NET's basic column, so `uuid`
shows `native=False` throughout — another reason UUID must be an owned regex.

The takeaway: **every** native parser disagrees with the pinned decisions in at
least one direction. Delegating to any of them would break P1. The owned check is
the only way to get value-for-value agreement.

## Coverage notes (per format)

- **uuid**: canonical lower/upper/mixed hex (all accepted), nil and max UUIDs,
  wrong length (±1), non-hex char, missing/extra/no dashes, wrong group lengths,
  brace-wrapped and `urn:uuid:` prefixed (both rejected), leading/trailing space,
  trailing newline (rejected — validates the strict-end anchor), empty.
- **ipv4**: `0.0.0.0`, `255.255.255.255`, octet 255 vs 256 vs 999, leading zeros
  (`01.2.3.4`, `192.168.000.1` — rejected per spec), too few/many octets,
  leading/trailing dot, empty octet, non-numeric, port suffix, hex.
- **ipv6**: full, `::` compression, `::`/`::1`, leading/trailing compression,
  IPv4-tail (`::ffff:1.2.3.4`, `::192.168.0.1`), uppercase, illegal double `::`,
  9 groups, too few uncompressed, invalid hex, over-long group, **zone id**
  (`%eth0`, rejected), IPv4-tail with bad octet, trailing colon, plain IPv4.
- **date**: leap-year matrix (2020 ok / 2021 bad / 1900 century-non-leap bad /
  2000 century-leap ok), month 00/13, day 00/32, every 30-day-month over-run
  (Apr/Jun/Sep/Nov 31 bad; 30 ok), Feb 30, short/long year, slashes, unpadded
  month, embedded time, trailing space.
- **time**: hour 24 (bad), minute 60 (bad), **second 60 leap (accepted)**,
  second 61 (bad), fractional 1/3/9 digits, empty fractional (bad), `+`/`-`/
  `-00:00` offsets, fractional-with-offset, lowercase `z`, **local time with no
  offset (accepted)**, offset hour 24 / minute 60 (bad), offset without colon
  (bad), missing seconds (bad), empty.
- **date-time**: `Z` / `+HH:MM` / `-00:00`, lowercase `t`/`z`/both, leap second,
  12-digit fractional, leap-year Feb 29, **missing offset (rejected)**,
  calendar-invalid (Feb 30 / month 13 / non-leap Feb 29), hour 24, space
  separator (bad), missing seconds (bad), offset without colon (bad), date-only
  (bad), trailing space, empty.

## Spec observations (proposed edits — spec NOT edited here, per instructions)

These are places the corpus revealed the spec is under- or mis-specified. All are
about wording; none required changing the pinned check.

1. **`time` local (offset-optional) is implied but never stated.** The spec's
   "RFC 3339 edge decisions" pin the **`date-time`** offset as required, but say
   nothing about **`time`**. RFC 3339 `partial-time` (a `full-time` without the
   `time-offset`) is offset-optional, and the natural reading of the pinned
   `time` regex (offset group `?`) accepts a local `12:30:45`. The corpus encodes
   `time-local-no-offset → valid`. **Proposed:** add a bullet to the temporal
   edge decisions: *"`time` MAY omit the offset (RFC 3339 `partial-time`); only
   `date-time` requires one."* Without it, an implementer could reasonably pin
   `time` to require an offset and silently diverge.

2. **Leading-zero ipv4 octets: spec says "no leading zeros" but does not pin the
   octet alternation.** The corpus rejects `01.2.3.4` and `192.168.000.1`.
   Worth noting the exact pinned octet alternation
   `(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])` in the Validator-mapping
   table (the spec currently only prose-describes ipv4), since native parsers
   (.NET) accept leading zeros and an implementer copying a looser regex would
   diverge.

3. **The strict end-anchor is a hidden portability requirement.** The spec's
   pinned-pattern table writes `$` (in the uuid row) as the end anchor, but a raw
   `$` is not portable (Python/Java/.NET accept a trailing `\n`). The `pattern`
   spec already establishes the `$`→`\Z`/`\z` normalization; **the `format` spec
   should cross-reference it** (or restate that the anchored pinned patterns use
   the per-target strict-end anchor), so the uuid row isn't read literally as a
   bare `$`. This corpus is the evidence.

4. **Offset range is unspecified.** The spec pins the offset *syntax*
   (`±HH:MM`) but not the numeric range. The corpus rejects `+24:00` and `+01:60`
   via the calendar predicate (`validOffset`). RFC 3339 constrains the offset
   hour/minute to `00–23`/`00–59`. **Proposed:** state that the calendar
   predicate also range-checks the offset fields, matching the date/time fields.
   (Minor — but `time.Parse` accepts `+24:00`, so it is a real divergence point.)

# Clock materialization — findings (no-truncation, offset-preserving)

Backs `features/format` **Materialization (type mapping)**. Question, per format,
per target: **can we emit a native typed construct AND have every equally-capable
materializing language re-serialize to the SAME wire bytes (P1) — while preserving
the original offset and full sub-second precision to each type's genuine limit
(NO truncation floor)?** Toolchains as-run: go 1.26, node v25.2 (+
`@js-temporal/polyfill`), python 3.13, java 21, ruby 2.6, dotnet 8.

> **Bottom line.** `date-time`, `date`, and `time` all materialize with
> **byte-identical** output across the equally-capable set (`go`, `java`, `py`,
> `js-string`, `js-temporal`) — **offset preserved**, full precision kept to
> each type's resolution, trailing fractional zeros trimmed. The only losses are
> at genuine type limits and land exactly where the spec says: **Python**
> truncates sub-microsecond date-time; legacy **`js-date`** folds date-time to a
> UTC millisecond instant; **leap `:60`** is non-materializing everywhere. Zero
> hard mismatches.

## What changed from the earlier probe

The previous corpus encoded an **obsolete** canonical form: UTC-normalized,
floored to milliseconds, always exactly 3 fractional digits with a literal `Z`,
`time` offset **dropped**, and TS materialized only via `Date`. The spec has
since moved to **no truncation**: preserve the original offset (`date-time` **and**
`time`), keep sub-second precision at each native resolution (Go/Java/Ruby
nanosecond, Python microsecond, .NET 100-ns), trim trailing fractional zeros, and
model all three `--js-temporal-repr` modes. This corpus was rewritten to that.

## 1. Serialized form (generator-owned)

For `date-time` / `time`, all materializing targets emit:

- **RFC 3339**, the value's **original offset preserved**, with `+00:00` /
  `-00:00` → `Z`.
- `T` / `Z` **uppercased on the parse path** before the native parse.
- fractional seconds at the value's own precision, **trailing zeros trimmed**,
  omitted entirely when zero.
- `time`: offset **preserved when present** (RFC 3339 optional offset); an
  offset-less value stays offset-less.

`date` → `YYYY-MM-DD` (lossless).

## 2. Why the serializer must be GENERATOR-OWNED, not native `toString`

The native serializers **disagree** on fractional-zero trimming, so none can be
the oracle — the runners build the string manually (offset + trimmed fraction):

| Input fraction | Go `RFC3339Nano` | Java `OffsetDateTime.toString` | Python `.isoformat` | Temporal `.toString` |
|---|---|---|---|---|
| `.5` | `.5` | **`.500`** | **`.500000`** | `.5` |
| `.250` | `.25` | **`.250`** | **`.250000`** | `.25` |
| `.123456789` | `.123456789` | `.123456789` | `.123456` (µs) | `.123456789` |

Go's `RFC3339Nano` happens to match the generator rules exactly (used directly
for Go date-time). Java emits fixed 3/6/9-digit groups; Python's `isoformat`
emits fixed 0/6 digits and `+00:00` (not `Z`). Both are re-serialized manually
in the runners. `Temporal.ZonedDateTime.toString({timeZoneName:'never'})` with
the default `fractionalSecondDigits:'auto'` matches, needing only `+00:00`→`Z`.

## 3. Per-target round-trip (proven)

| Target | Type | date-time | date | time |
|---|---|---|---|---|
| Go | `time.Time` (phantom for date/time) | offset + **ns** — lossless | lossless | offset preserved, **ns**, lossless |
| Java | `OffsetDateTime` / `LocalDate` / `OffsetTime`\|`LocalTime` | offset + **ns** — lossless | lossless | offset preserved (`OffsetTime`) or offset-less (`LocalTime`) — lossless |
| Python | `datetime`/`date`/`time` (aware/naive) | offset preserved, **µs** (sub-µs truncated) | lossless | offset preserved, µs, lossless |
| TS `js-string` | `string` | serialized string — lossless | lossless | lossless (string) |
| TS `js-temporal` | `ZonedDateTime` / `PlainDate` / **`string`** for time | offset + **ns** — lossless | lossless | lossless (**string**; Temporal has no offset-bearing time type) |
| TS `js-date` | `Date` (date-time only) | **UTC instant, ms** — LOSSY | unsupported | unsupported |
| Ruby\* | `DateTime` / `Date` / — | offset + **ns** (Rational) — lossless | lossless | **unsupported** (no time-of-day type) |
| .NET\* | `DateTimeOffset` / `DateOnly` / `TimeOnly` | offset preserved, **100-ns** (rounds 9→7) | lossless | offset-less only (`TimeOnly` can't hold an offset) |

`\*` prospective.

## 4. Leap second `:60` — rejected (materialized narrowing)

The materialized grammar rejects `:60` at **validation**, before any parse, so
`dt-leap`/`t-leap` are non-materializing (SKIP) in every language — not a
mismatch. Go, Java, Python, and .NET native parsers **also** reject `:60`
independently. The JS and Ruby runners add an explicit guard (see §5).

## 5. SPEC DISCREPANCIES — for correcting `features/format`

1. **`time` offset preservation is real and lossless in TS.** Under both
   `js-string` and `js-temporal`, `time` is a `string` that **carries its
   offset** (`12:30:45+02:00` round-trips verbatim; `+00:00`/`-00:00`→`Z`;
   `.250`→`.25`). The spec's current text is right; any lingering claim that
   `time` "cannot materialize in TS / merges distinct offsets" is obsolete. TS
   never uses `Date` for `time`. **No change needed to the spec's time row — but
   the OLD NOTES were wrong and are now corrected.**

2. **JS Temporal does NOT reject leap `:60` — it SILENTLY CLAMPS `:60`→`:59`.**
   `Temporal.ZonedDateTime.from('…23:59:60Z[+00:00]')` returns `…23:59:59`
   **even with `{overflow:'reject'}`** (leap-second handling in Temporal string
   parsing is always constrain). **Ruby `DateTime.rfc3339` clamps identically.**
   The spec (`features/format`, Ecosystem-variance + the narrowing rationale)
   says *"every native parser rejects it and Ruby silently clamps `:60`→`:59`."*
   That is **inaccurate**: **both Temporal (JS) and Ruby clamp**; only Go, Java,
   Python, and .NET reject. The materialization-safety argument still holds —
   safety comes from the **validator's `:60`-rejecting grammar running before the
   parse**, uniformly — but the spec sentence should read something like *"Go /
   Java / Python / .NET native parsers reject `:60`; JS Temporal and Ruby
   silently clamp `:60`→`:59` (corruption) — which is why the materialized
   grammar rejects `:60` at validation, before any parse, in every language."*
   The runners model this by rejecting `:60` explicitly in JS and Ruby.

3. **`.NET DateTimeOffset` resolution is 100-ns and ROUNDS, not truncates.** A
   9-digit input `.123456789` becomes `.1234568` in .NET (rounds the 8th/9th
   digits into the 7th), a third resolution distinct from Go/Java's 9 and
   Python's 6. .NET is prospective, so this is not a hard failure, but if .NET
   ever becomes a model target its `date-time` sub-100-ns behavior is a
   documented loss (and it *rounds*, unlike Python which *truncates*). The spec
   per-target table should note .NET at **100-ns, rounding** if/when added.

4. **.NET `time` with an offset is unsupported** (`TimeOnly` has no offset), and
   **Ruby has no time-of-day type at all**. Consistent with open-question #3;
   both stay non-materializing for offset-bearing / all `time` respectively. The
   equally-capable set deliberately excludes them for `time`.

No other discrepancies: offset preservation, `+00:00`/`-00:00`→`Z`, lowercase
`t`/`z` uppercasing, trailing-zero trimming, and nanosecond retention all behave
byte-identically across Go / Java / js-string / js-temporal, with Python's µs
truncation the only in-set loss (and it is kept OUT of the must-agree set for the
2 sub-µs rows, verified against the explicit µs-truncated expectation).

## 6. Harness transcript (`python3 compare.py`)

```
engines run: go, java, js-date, js-string, js-temporal, py
equally-capable set (must agree byte-for-byte): go, java, py, js-string, js-temporal

[OK    ] date-time dt-z-nofrac      ref='2021-06-15T12:30:45Z'
          js-date  -> '2021-06-15T12:30:45.000Z' (UTC-instant fold)
[OK    ] date-time dt-offset-pos    ref='2021-06-15T12:30:45+02:00'
          js-date  -> '2021-06-15T10:30:45.000Z' (UTC-instant fold)
[OK    ] date-time dt-offset-neg    ref='2021-06-15T12:30:45-05:00'
          js-date  -> '2021-06-15T17:30:45.000Z' (UTC-instant fold)
[OK    ] date-time dt-offset-plus0000 ref='2021-06-15T12:30:45Z'
          js-date  -> '2021-06-15T12:30:45.000Z' (UTC-instant fold)
[OK    ] date-time dt-offset-neg0000 ref='2021-06-15T12:30:45Z'
          js-date  -> '2021-06-15T12:30:45.000Z' (UTC-instant fold)
[OK    ] date-time dt-frac-3        ref='2021-06-15T12:30:45.123Z'
          js-date  -> '2021-06-15T12:30:45.123Z' (same)
[OK    ] date-time dt-frac-6        ref='2021-06-15T12:30:45.123456Z'
          js-date  -> '2021-06-15T12:30:45.123Z' (UTC-instant fold)
[EXPECT] date-time dt-frac-9        ref='2021-06-15T12:30:45.123456789Z'  | py sub-µs -> '2021-06-15T12:30:45.123456Z'
          js-date  -> '2021-06-15T12:30:45.123Z' (UTC-instant fold)
[EXPECT] date-time dt-frac-9-offset ref='2021-06-15T12:30:45.123456789+02:00'  | py sub-µs -> '2021-06-15T12:30:45.123456+02:00'
          js-date  -> '2021-06-15T10:30:45.123Z' (UTC-instant fold)
[OK    ] date-time dt-frac-6-offset ref='2021-06-15T12:30:45.123456-05:00'
          js-date  -> '2021-06-15T17:30:45.123Z' (UTC-instant fold)
[OK    ] date-time dt-frac-1        ref='2021-06-15T12:30:45.5Z'
          js-date  -> '2021-06-15T12:30:45.500Z' (UTC-instant fold)
[OK    ] date-time dt-frac-trailzero ref='2021-06-15T12:30:45.12Z'
          js-date  -> '2021-06-15T12:30:45.120Z' (UTC-instant fold)
[OK    ] date-time dt-frac-offset   ref='2021-06-15T12:30:45.25-03:00'
          js-date  -> '2021-06-15T15:30:45.250Z' (UTC-instant fold)
[OK    ] date-time dt-lowercase     ref='2021-06-15T12:30:45Z'
          js-date  -> '2021-06-15T12:30:45.000Z' (UTC-instant fold)
[OK    ] date-time dt-lower-frac-off ref='2021-06-15T12:30:45.5-03:00'
          js-date  -> '2021-06-15T15:30:45.500Z' (UTC-instant fold)
[OK    ] date-time dt-midnight      ref='2021-06-15T00:00:00Z'
          js-date  -> '2021-06-15T00:00:00.000Z' (UTC-instant fold)
[SKIP ] date-time dt-leap          rejected by all capable engines (materialized grammar / native parser)
          go          : parsing time "2021-12-31T23:59:60Z": second out of range
          java        : DateTimeParseException: Text '2021-12-31T23:59:60Z' could not be parsed: Invalid
          js-string   : leap second :60 rejected by materialized grammar
          js-temporal : leap second :60 rejected by materialized grammar
          py          : ValueError: second must be in 0..59
[OK    ] date      d-basic          ref='2021-06-15'
[OK    ] date      d-leap-feb29     ref='2020-02-29'
[OK    ] date      d-year-min       ref='0001-01-01'
[OK    ] date      d-year-max       ref='9999-12-31'
[OK    ] time      t-local          ref='12:30:45'
[OK    ] time      t-z              ref='12:30:45Z'
[OK    ] time      t-offset-pos     ref='12:30:45+02:00'
[OK    ] time      t-offset-neg     ref='12:30:45-05:00'
[OK    ] time      t-plus0000       ref='12:30:45Z'
[OK    ] time      t-neg0000        ref='12:30:45Z'
[OK    ] time      t-frac           ref='12:30:45.25'
[OK    ] time      t-frac-z         ref='12:30:45.25Z'
[OK    ] time      t-offset-frac    ref='12:30:45.5+02:00'
[OK    ] time      t-lower-z        ref='12:30:45Z'
[SKIP ] time      t-leap           rejected by all capable engines (materialized grammar / native parser)
          go          : parsing time "23:59:60Z": second out of range
          java        : DateTimeParseException: Text '23:59:60Z' could not be parsed, unparsed text foun
          js-string   : leap second :60 rejected by materialized grammar
          js-temporal : leap second :60 rejected by materialized grammar
          py          : ValueError: second must be in 0..59

========================================================================
PASS: equally-capable set {go, java, py, js-string, js-temporal} agrees byte-for-byte  |  hard mismatches: 0
expected divergences: Python sub-µs truncations = 2, js-date UTC folds = 16, leap-second skips = 2
```

With `--with-ruby --with-dotnet` the verdict is unchanged (PASS, 0 hard
mismatches): Ruby agrees on all `date-time`/`date` (nanosecond preserved), is
unsupported for `time`; .NET agrees except the two nanosecond rows (rounds to
100-ns: `.1234568`) and offset-bearing `time` (`TimeOnly` can't hold an offset).

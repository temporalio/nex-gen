# Why native ISO-8601 duration parsers are unsuitable (empirical)

The `duration` format must validate **identically** across all seven targets
(P1). Every language's stdlib duration facility either does not exist, parses a
*different grammar*, or diverges from RFC 3339 Appendix A (ISO 8601). None can
be the source of truth. Verdicts below are from live probes on the installed
toolchains (see the commands in the probes/report); "ABNF" = the strict RFC 3339
answer that the pinned regex produces.

Probe set and the strict-ABNF answer:

| value | ABNF | note |
|---|---|---|
| `P1Y` | valid | year |
| `P1M` | valid | month |
| `PT1.5S` | **invalid** | RFC 3339 ABNF forbids fractions |
| `-P1Y` | **invalid** | no sign |
| `P-1Y` | **invalid** | no signed component |
| `P` / `PT` | **invalid** | >=1 component required |
| `P1Y4D` | **invalid** | year-skip-month-day (strict nesting) |
| `P1W` | valid | week form |
| `P1YT1H` | valid | date + time |
| `P1D` | valid | day |

## Java — split across two types, neither is RFC 3339

`java.time.Duration.parse` and `java.time.Period.parse` cover **disjoint**
subsets and both diverge:

- `Duration.parse`: **rejects Y and M entirely** (`P1Y`, `P1M`, `P1Y4D`,
  `P1YT1H`, `P1W` all REJECT), **accepts fractions** (`PT1.5S` OK), and
  **normalizes** `P1D`->`PT24H`.
- `Period.parse`: **accepts negatives** (`-P1Y`, `P-1Y` both OK), **accepts
  `P1Y4D`** (which strict ABNF rejects), **silently expands** `P1W`->`P7D`, and
  **rejects the combined date+time** `P1YT1H`.

There is no single Java stdlib call that accepts the RFC 3339 duration set; a
generator using either would diverge from the other targets and from the spec.

## .NET — `System.Xml.XmlConvert.ToTimeSpan`

- **Accepts fractions** (`PT1.5S` OK), **accepts a leading sign** (`-P1Y` OK),
  **accepts `P1Y4D`** (strict ABNF rejects), and **rejects the week form**
  (`P1W` REJECT — a valid RFC 3339 duration!).
- It also **collapses** calendar units to fixed spans (`P1Y`->365 days,
  `P1M`->30 days), which is a value transformation, not a validation.

Diverges from both Java parsers and from the ABNF.

## Go — no ISO-8601 duration parser at all

`time.ParseDuration` parses the **Go-specific** `"1h30m"` grammar, not `P...`.
It **rejects every** RFC 3339 duration (`P1Y`, `PT1H`, `P1D`, ...) and
**accepts** non-ISO strings like `1h30m`. Unusable.

## Python — none in stdlib

`datetime` has **no** ISO-8601 duration parser. `timedelta.fromisoformat`
parses an `HH:MM:SS`-style timedelta, not a `P...` duration. (Third-party
`isodate`/`pydantic` exist but are dependencies -> P4, and have their own
grammars.)

## Ruby — none in stdlib

`Date`/`Time` parse timestamps, not durations; `Date._iso8601` returns an empty
hash `{}` for every `P...` value. No stdlib duration parser.

## Conclusion

The only way to get identical accept/reject across all seven targets is a
**generator-owned pinned regex** (RE2-safe, ASCII digits), which is exactly
what `../compare.py` verifies: 68/68 values agree across Rust/Go/JS/Python/
Java/Ruby/.NET *and* match the strict ABNF. The native parsers are the P1
hazard the owned check exists to avoid — the same posture the `format` spec
already takes for the temporal formats.

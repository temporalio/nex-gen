# Format typed-representation probes — findings

Backs `features/format`. Question: for each of the six asserted formats
(`time`, `date`, `date-time`, `uuid`, `ipv4`, `ipv6`), in each of the 7
languages, can we construct an **idiomatic typed in-memory value** from the
validated string using **STANDARD LIBRARY ONLY** (P4)? And if so, does the
native parser (a) match the spec's pinned RFC 3339 / regex grammar (P1) and
(b) round-trip back to the identical wire string?

Everything here is re-runnable:

```
go run .                                   # main.go
node typed.mjs                             # JS/Node built-ins
python3 typed.py                           # stdlib datetime/uuid/ipaddress
java Typed.java                            # JDK, single-file mode
rustc typed.rs -o /tmp/t && /tmp/t         # Rust std only
ruby typed.rb                              # stdlib date/time/ipaddr
dotnet run --project typed_cs.csproj       # .NET BCL (net8)
```

Toolchains as-run: go, node v25, python3 3.13, java 21, ruby 2.6,
dotnet 8, rustc 1.88.

> **Bottom line up front.** The spec's [[format]] Type-mapping already says
> "None — the emitted field type is [[type]]'s `string`." These probes
> **confirm that decision is correct** and quantify *why*: there is **no
> format for which all 7 languages can build a typed value from stdlib**
> (uuid and every temporal format break in Rust; uuid also breaks in Go and
> Ruby), and even where a stdlib type exists it frequently **diverges from
> the pinned grammar** (accepts what we reject, or vice-versa) or
> **normalizes on the way back out** (loses the original wire bytes). A
> typed model field would therefore either force a dependency (violating
> P4) or silently break P1/round-trip. Keep `string`.

---

## Matrix A — stdlib typed representation available? (Y / N + type)

"Y" = a first-class typed value can be constructed from the canonical
string using only the standard library, no added package.

| Format \ Lang | Go | Java | Python | TypeScript/JS | Rust (std) | Ruby | .NET |
|---|---|---|---|---|---|---|---|
| `date-time` | Y `time.Time` | Y `OffsetDateTime` | Y `datetime.datetime` | ~ `Date` (instant) | **N** (no type) | Y `DateTime`/`Time` | Y `DateTimeOffset` |
| `date` | ~ `time.Time`¹ | Y `LocalDate` | Y `datetime.date` | **N**² | **N** | Y `Date` | Y `DateOnly` |
| `time` | ~ `time.Time`¹ | Y `OffsetTime`/`LocalTime`³ | Y `datetime.time` | **N**⁴ | **N** | **N**⁵ | ~ `TimeOnly`³ |
| `uuid` | **N**⁶ | Y `java.util.UUID` | Y `uuid.UUID` | **N**⁷ | **N**⁶ | **N**⁷ | Y `System.Guid` |
| `ipv4` | Y `netip.Addr` | Y `InetAddress`⁸ | Y `IPv4Address` | **N**⁹ | Y `Ipv4Addr` | Y `IPAddr` | Y `IPAddress` |
| `ipv6` | Y `netip.Addr` | Y `InetAddress`⁸ | Y `IPv6Address` | **N**⁹ | Y `Ipv6Addr` | Y `IPAddr` | Y `IPAddress` |

Legend: **Y** = clean stdlib type; **~** = a type exists but is a poor/lossy
fit (footnote); **N** = no stdlib type OR no stdlib parser (would force a
dependency).

1. Go has **no** date-only or time-only type. `date`/`time` reuse
   `time.Time`, which then carries a bogus zero time-of-day (`00:00:00 UTC`
   for a `date`) or a bogus zero date (`0000-01-01` for a `time`) — an
   honest fit only for `date-time`.
2. `new Date("2020-02-29")` *does* return a `Date`, but it is a full UTC
   **instant at midnight**, not a date — carries a spurious time & TZ.
3. RFC 3339 `time` **may carry an offset** (`12:00:00+01:00`). Java's
   `OffsetTime` holds it; `LocalTime` drops it. .NET `TimeOnly` **cannot
   represent an offset at all** — offset info is lost, so it's a `~`.
4. `new Date("12:00:00")` → `Invalid Date` (JS has no time-only concept).
5. Ruby has no time-of-day-only stdlib type. `Time.parse("12:00:00")`
   fabricates *today's* date (`2026-07-16T12:00:00`), so it's not a `time`.
6. **Go and Rust std ship no UUID type and no UUID parser.** Go needs
   `github.com/google/uuid`; Rust needs the `uuid` crate. (Both *generate*
   nothing in std either.)
7. **JS and Ruby have no UUID *type*.** `crypto.randomUUID()` /
   `SecureRandom.uuid` only **generate** a plain `string`; there is no
   parse-into-typed-value or validate API.
8. Java `InetAddress` is a `~` in practice — see the DNS + lax-parse
   hazards in Matrix B; it is not a safe pure syntactic parser.
9. JS/Node has `net.isIP()` (a **validator**, returns 4/6/0) but yields
   **no typed address object**, and `isIP` is a *Node* built-in, absent
   from the ECMAScript/browser stdlib entirely.

**Rust is the hard wall:** std can build **only** the two IP types. Every
temporal format and `uuid` require a crate. (Rust is the generator's own
gate engine, not an emitted target — but this confirms std can't back a
typed emit even if it were.)

---

## Matrix B — grammar-divergence & round-trip hazards (the P1 killers)

For each cell that *has* a stdlib parser, does it (col 1) accept/reject the
**same** strings as our pinned grammar, and (col 2) round-trip the exact
wire bytes back out? `✗` = hazard, `≈` = normalizes but lossless-ish, `✓` =
clean. Evidence is the probe output.

### Grammar divergence vs the pinned RFC 3339 / regex rules

| Format \ Lang | Go | Java | Python | JS `Date` | Rust std | Ruby | .NET |
|---|---|---|---|---|---|---|---|
| `date-time` | ✗ rejects `:60` leap sec; ✗ rejects lowercase `t`/`z` | ✗ rejects `:60`; ✓ lowercase OK | ✗ rejects `:60`; ✗ rejects lowercase; ✗ **accepts missing offset** | ✗✗ wildly lax (see below) | n/a | ✗✗ **clamps `:60`→`:59`** silently | ✗ rejects `:60`; ✗ **accepts missing offset** (as local) |
| `date` | ✓ calendar OK | ✓ calendar OK | ✓ | ✗ `2021-13-01` Invalid but `2021-02-29`→**rolls to Mar 1** | n/a | ✓ (`Date.iso8601`) | ✓ (`ParseExact`) |
| `time` | ✗ rejects `:60`; needs offset in layout | ✗ rejects `:60` | ✗ rejects `:60` | n/a (no type) | n/a | n/a | ✗ rejects `:60`; ✗ offset unrepresentable |
| `uuid` | n/a | ✗✗ **very lax** (`1-2-3-4-5` OK, pads groups) | ✗ **too lax** (accepts no-dash, braces, `urn:`) | n/a | n/a | n/a | ✗ **too lax** (`Parse` accepts no-dash/braces); `ParseExact "D"` is strict ✓ |
| `ipv4` | ✓ matches pinned (rejects leading-zero) | ✗✗ **`getByName` may DNS**; `01.2.3.4`→`1.2.3.4`, `1.2.3`→`1.2.0.3` | ✓ matches pinned (rejects leading-zero) | n/a | ✓ matches pinned | ✓ rejects leading-zero & short | ✗ `01.2.3.4` OK, `1.2.3`→`1.2.0.3` |
| `ipv6` | ✓ (accepts zone `%`) | ✗ DNS risk | ✓ (accepts zone `%`) | n/a | ✗ **rejects zone id `%eth0`** | ✓ | ✓ (accepts zone `%`) |

**`:60` leap second is the single biggest temporal divergence.** The spec
**pins leap-second `:60` as ACCEPTED** (§ "RFC 3339 edge decisions"). Every
native temporal parser disagrees, three different ways:
- Go / Java / Python / .NET: **reject** `2021-02-28T23:59:60Z` outright.
- **Ruby `DateTime.rfc3339` silently clamps** `:60`→`:59` — the returned
  value is `...23:59:59`, a *different instant*, no error. Data corruption.
- **Ruby `Time.iso8601`** *rolls it forward*: `23:59:60Z` →
  next-day `00:00:00Z`. Also silent corruption.

**`date-time` missing offset.** Spec pins offset **REQUIRED**. Python
(`fromisoformat`) and .NET (`DateTimeOffset.Parse`) both **accept** a bare
local `date-time` (Python → naive `tzinfo=None`; .NET → local machine
offset). Go/Java/Ruby reject it (agree with pinned). So the native parsers
split on a value the spec says must fail.

**JS `Date` is the worst offender (as the spec's Validator-mapping note
already warns).** It accepts a missing offset and reinterprets as *local*
time (`2006-01-02T15:04:05` → `...T23:04:05Z` here, machine-TZ dependent);
it **rolls over** an out-of-range calendar date (`2021-02-30` →
`2021-03-02`, `2021-02-29` → `2021-03-01`) instead of rejecting; and it
truncates sub-ms precision. Non-portable and lossy on multiple axes.

**Java `InetAddress.getByName` is disqualifying for two reasons:** (1) it is
a *host resolver*, not a literal parser — for a non-literal it can perform a
**blocking DNS lookup** (a network call from a validator!); (2) it parses
malformed IPv4 leniently — `01.2.3.4`→`1.2.3.4` (accepts leading zero the
pinned regex rejects) and `1.2.3`→`1.2.0.3` (BSD `inet_aton` 3-part form).
.NET `IPAddress.Parse` shares the lenient-IPv4 bugs (no DNS though).

**Rust `Ipv6Addr` rejects the zone-id form** (`fe80::1%eth0`) that Go /
Python / Ruby / .NET accept — another accept/reject split (though the
pinned ipv6 regex is RFC 4291 and also excludes zone ids, so Rust actually
agrees with *our* regex here; Go/Python/etc. are the lax ones).

**UUID parsers are uniformly too lax** where they exist: Python accepts
no-dash, brace-wrapped, and `urn:uuid:` forms; Java pads short groups
(`1-2-3-4-5` → a valid UUID!); .NET `Guid.Parse` accepts no-dash & braces.
Only .NET `Guid.ParseExact(s, "D")` matches the pinned hyphenated-only
regex. The pinned regex rejects all the extra forms, so every lax native
parser is an accept-side P1 hazard.

### Round-trip / normalization hazards (does the typed value re-emit the identical wire string?)

| Format \ Lang | Go | Java | Python | JS `Date` | Rust std | Ruby | .NET |
|---|---|---|---|---|---|---|---|
| `date-time` | ≈ `+00:00`→`Z` | ≈ `+00:00`→`Z` | ≈ keeps offset but `Z`→`+00:00`, **truncates ns→µs** | ✗✗ →ms UTC always | n/a | ✗ `Z`→`+00:00`, **pads to 9 frac digits** | ≈ →`+00:00`, pads to 7 frac digits |
| `date` | ≈ (drops the phantom time) | ✓ | ✓ | ✗ | n/a | ✓ | ✓ |
| `time` | ≈ | ≈ (`12:00:00`→`12:00`) | ✓ | n/a | n/a | n/a | ✗ offset lost |
| `uuid` | n/a | ✗ **lowercases** hex | ✗ **lowercases** hex | n/a | n/a | n/a | ✗ **lowercases** hex |
| `ipv4` | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | ✓ (after lenient re-canon) |
| `ipv6` | ✗ **compresses** `2001:0db8:…:0001`→`2001:db8::1` | ✗ expands `::`→full | ✗ **compresses**/lowercases | n/a | ✗ compresses | ✗ compresses | ✗ compresses |

Observed normalizations (all break byte-identical round-trip — a user handed
a typed value and re-serializing would emit **different bytes** than came in):
- **UUID hex → lowercase** everywhere (`F81D...`→`f81d...`). The pinned
  regex accepts both cases; a typed field would silently lowercase.
- **IPv6 recompression/lowercasing** everywhere
  (`2001:0db8:0000:...:0001`→`2001:db8::1`; Java instead *expands*). The
  canonical form differs from most on-wire forms.
- **`date-time` offset normalization**: `+00:00`↔`Z` swap (Go/Java →`Z`;
  Python/Ruby/.NET →`+00:00`), and **fractional-second precision changes**
  (Python truncates 9→6 digits — *actual data loss* for nanosecond
  timestamps; Ruby pads to 9; .NET to 7). The spec pins "any precision, not
  normalized", so all of these violate the pinned rule.

---

## Per-language one-liners

- **Go** — IPs clean via `net/netip.Addr` (matches pinned ipv4 incl.
  leading-zero reject; ipv6 compresses on re-emit). **No UUID type at all.**
  `time.Time` works only for `date-time` (rejects leap-second & lowercase),
  and is a phantom-field fit for `date`/`time`.
- **Java** — best temporal coverage (`OffsetDateTime`/`LocalDate`/
  `OffsetTime`, all calendar-correct, lowercase-`t`/`z` OK) but **rejects
  leap-second** and lowercases UUID. `InetAddress` is unusable: **DNS risk**
  + lenient IPv4.
- **Python** — richest clean stdlib set (`datetime`/`date`/`time`/`UUID`/
  `IPv4Address`/`IPv6Address`). Still: rejects leap-second, accepts
  missing-offset, UUID too lax + lowercases, ns→µs truncation, ipv6
  recompresses.
- **TypeScript/JS** — only `Date`, and it is uniquely dangerous (lax +
  local-time reinterpret + calendar roll-over + ms truncation). **No** time,
  date, uuid, or IP *type*. `net.isIP` is Node-only and returns no object.
- **Rust (std)** — **only** `Ipv4Addr`/`Ipv6Addr`. No temporal type, no
  UUID — both need crates. (Rust is the gate engine, not an emit target.)
- **Ruby** — `DateTime`/`Date`/`IPAddr` present; **no time-only type, no
  UUID type**. `DateTime.rfc3339` **silently clamps** leap-second (worst
  correctness bug found).
- **.NET** — `DateTimeOffset`/`DateOnly`/`Guid`/`IPAddress` present;
  `TimeOnly` can't hold an offset. `Guid.Parse` too lax (use `ParseExact`);
  `IPAddress.Parse` has the lenient-IPv4 bugs; accepts missing offset.

---

## Recommendation — per format

The guiding constraints: **P4** (no added dependency) and **P1** (identical
accept/reject *and* byte round-trip across all targets). A typed field is
admissible only if *every* target can build it from stdlib **and** agree
value-for-value in both directions. Verdict per format:

| Format | Emit typed? | Blocking reason (evidence) |
|---|---|---|
| `date-time` | **No — keep `string`** | Rust has no std type (dep). And *every* native parser diverges from the pinned grammar: leap-second rejected (Go/Java/Py/.NET) or **silently clamped** (Ruby); missing-offset accepted (Py/.NET); JS `Date` reinterprets/rolls over. Plus offset & fractional-second normalization on re-emit. P1 dead on arrival. |
| `date` | **No — keep `string`** | Rust & JS have no date type (dep / lossy `Date`). Go's fit is a phantom-time `time.Time`. Native calendar handling agrees elsewhere, but two targets can't play. |
| `time` | **No — keep `string`** | **No stdlib time-only type in JS, Rust, or Ruby.** .NET `TimeOnly` drops the RFC 3339 offset. Go fakes it with `time.Time`. Non-starter. |
| `uuid` | **No — keep `string`** | **No stdlib UUID type in Go, JS, Rust, or Ruby** (4/7 force a dep). Where a type exists it lowercases hex (round-trip break) and mostly over-accepts (Java pads, Py/.NET accept extra forms). |
| `ipv4` | **No — keep `string`** (but the *closest* candidate) | Go/Python/Rust/Ruby match the pinned grammar and round-trip cleanly; **but JS has no IP type** and **Java/.NET parse leniently (+ Java DNS risk)**. So still not universal, and the value-add over a validated `string` is marginal. |
| `ipv6` | **No — keep `string`** | Same as ipv4 plus **universal recompression/lowercasing** on re-emit (byte round-trip lost everywhere except a raw string), and Rust rejects the zone-id form others accept. |

**Overall: keep every format as `string` in the emitted model, exactly as
[[format]]'s Type-mapping already states ("None").** These probes are the
evidence for that line. The format *name* stays in the doc comment; the
check stays a shared-`Validate` predicate over the `string` (the pinned
regex + calendar helper), which is the *only* way to get identical
accept/reject — no native parser does, and several corrupt data silently
(Ruby leap-second clamp, JS calendar roll-over, Python ns truncation).

**Cells that would force a dependency if we ever emitted typed** (the P4
violations, for the record): **uuid** in Go, JS, Rust, Ruby; **every
temporal format** in Rust; **date/time/uuid/ip** all in JS. There is **no
single format** where all 7 targets can go typed on stdlib alone — so a
typed-repr feature could at best be *per-language opt-in*, which would break
the "identical model shape" half of P1 and is not worth it for values we
already validate as strings.

**If a typed accessor is ever wanted** (a future, opt-in ergonomic layer,
*not* the wire model), the only safe shape is a **derived getter** that
parses the already-validated string on demand and is **never** the
serialized field — e.g. Python `.as_uuid()`/`.as_datetime()`, Go
`ParsedIP() (netip.Addr, error)` — gated per-language to where stdlib
suffices (so: not Rust temporal/uuid, not Go/JS/Ruby uuid, not JS at all).
Even then the parser must be **our** pinned check, not the native one, to
preserve P1 — meaning the typed value is a *convenience over* the validated
string, and re-serialization must always go from the stored `string`, never
from the typed value (whose re-emit normalizes). That keeps the wire bytes
authoritative and sidesteps every round-trip hazard in Matrix B.

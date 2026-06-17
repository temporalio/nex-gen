# `hostname` format cross-language conformance

Empirical study answering **`format` open question 1** (widen the asserted
subset) for the **`hostname`** format (JSON Schema 2020-12 §7.3.3, RFC 1123 /
RFC 952 ASCII host names). Question: can the generator OWN a single pinned,
RE2-safe check that every target implements to **identical** accept/reject
verdicts, with no added runtime dependency — the same posture [[pattern]] and
the already-asserted `uuid`/`ipv4`/`ipv6` use?

Answer (this study): **yes.** A single anchored RE2-safe regex plus a cheap
total-length guard agrees value-for-value across all **seven** targets (Go,
TypeScript/JS, Python, Java, Rust, Ruby, .NET) on a 41-case corpus.
`idn-hostname` (Unicode / Punycode / IDNA) stays deferred — it is a separate
format and is explicitly out of scope here.

## The pinned check under test

Two parts, both in the shared `Validate` layer:

1. **Regex** (RE2-safe, compiles under the Rust `regex` crate = the load gate),
   fully anchored, ASCII, compiled once:

   ```
   ^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*$
   ```

   Each label: `[A-Za-z0-9]` head, up to 61 `[A-Za-z0-9-]`, `[A-Za-z0-9]`
   tail — i.e. **1–63 chars, LDH, no leading/trailing hyphen**; the `?` lets a
   1-char label through. `(?:\. …)*` chains labels; at least one label
   required. Case-insensitive by construction (both cases in the class).

2. **Total-length guard**, OUTSIDE the regex, in every target:
   `1 <= code_point_count(s) <= 253`. RE2 has **no lookahead**, so the ajv-style
   `(?=.{1,253}$)` whole-input assertion cannot be expressed; a `len()` check is
   the portable equivalent and is trivial arithmetic in every language.

### End-anchor normalization (load-bearing — reused from [[pattern]])

A naive `$` is **not** portable: Python and Java `$` also match *before a
single trailing `\n`*, so `"host\n"` would be **accepted** by Python/Java and
**rejected** by Go/JS — a real P1 split (verified). The runners apply the exact
[[pattern]] `$`-normalization per target:

| Target | Start | End anchor | Note |
|---|---|---|---|
| Go / JS(`u`) / Rust | `^` | `$` | already end-of-input only |
| Python | `\A` | `\Z` | Python's strict end anchor (`$` is lenient) |
| Java | `\A` | `\z` | Java's strict end anchor (`\Z` is lenient) |
| Ruby | `\A` | `\z` | `^`/`$` are always line anchors in Ruby |
| .NET | `\A` | `\z` | .NET `\z` = strict; `\Z` is lenient (reverse of Java letter) |

No `\d`/`\w`/`\s`/`\b` appear (the class is spelled explicitly), so no
ASCII-class flag (`re.ASCII` / `ECMAScript` / Ruby `(?a)`) is *required*;
`re.ASCII` is kept in the Python runner for consistency with the pinned recipe.
Length is counted in **code points** everywhere (JS `[...s].length`, Java
`codePointCount`, .NET `EnumerateRunes().Count()`) so astral input can't split
the count — though hostnames are ASCII, so it only matters for the non-ASCII
reject rows, which the regex rejects anyway.

## Pinned edge decisions (the P1 line — we OWN these)

| Decision | Verdict | Rationale |
|---|---|---|
| **Trailing dot** `example.` | **reject** | Matches JSON-Schema-Test-Suite draft2020-12 §1 (`trailing dot` → invalid). Simpler, stricter, canonical. (ajv *accepts* it — a divergence we deliberately don't follow.) |
| **All-numeric label / TLD** `999`, `123.456`, `192.168.0.1` | **accept** | RFC 1123's "a valid host name can never have the form #.#.#.# … the highest-level label will be alphabetic" is an interpretive note, **not** a syntactic MUST, and the "at least one label is alphabetic" rule is not cleanly RE2-expressible in general. Matches ajv. Documented residual below. |
| **`xn--` A-labels** `xn--9n2bp8q…`, `xn--X` | **accept as ordinary LDH labels** | Punycode decode + IDNA validation (the test-suite's §2 A-label cases) is **`idn-hostname`**, deferred. At the ASCII layer `xn--X` is just a valid LDH label. Matches ajv (which also does no Punycode decode). |
| **Case** | insensitive | Both cases in the class. |
| **Max label** | 63 | `{0,61}` + head + tail. |
| **Max total** | 253 | Presentation-form domain limit (RFC 1035 wire 255 octets → 253 chars presentation); the length guard. |

These are the same "we OWN the check so every target agrees" stance the `format`
spec already takes for the temporal calendar predicate.

## Files (mirrors `../pattern_conformance/`)

- `corpus.json` — 41 `{id, instance, valid, why}` cases; `valid` is the pinned
  verdict. Also carries `regex` and `max_total_len` for reference.
- `rust_runner/` — the generator's own `regex` crate; doubles as the **load
  gate** (the pinned regex must compile — it does) and evaluates the full check.
  `cargo clean` keeps the tree to source (`Cargo.toml`, `Cargo.lock`, `src/`).
- `runner.go`, `runner.mjs`, `runner.py`, `Runner.java`, `runner.rb`,
  `dotnet_runner/HostnameRunner/` — one per target engine. Each emits JSON Lines
  `{"id","engine","valid","regex","len_ok"}` (the extra `regex`/`len_ok` fields
  show the guard's contribution).
- `compare.py` — builds/runs all seven, aligns by id, checks (a) each engine vs
  the pinned `valid` and (b) all-seven cross-agreement. Exits nonzero on any
  disagreement.

## Run

```sh
cd json-schema/research/format_hostname
python3 compare.py
```

Standalone: `go run runner.go corpus.json`, `node runner.mjs corpus.json`,
`python3 runner.py corpus.json`, `java Runner.java corpus.json`,
`ruby runner.rb corpus.json`,
`dotnet run --project dotnet_runner/HostnameRunner -- corpus.json`, and (after
`cargo build --release`) `rust_runner/target/release/rust_runner corpus.json`.

## Findings

**PASS — 41/41 cases, identical across all seven targets.** Every engine's
`valid` equals the pinned verdict, and all seven agree with each other. The
pinned regex compiles under the Rust `regex` crate (RE2-safe, no
lookahead/backtracking), so it is portable to every runtime engine by
construction — the same gate property [[pattern]] relies on.

Two things the corpus proves are load-bearing:

1. **`$` → `\Z`/`\z` normalization.** Verified directly: a raw `$` makes Python
   and Java *accept* `"host\n"` while Go/JS reject it. The `newline-tail` case
   (pinned `false`) is upheld by all seven **only** with the per-target strict
   end anchor. This is not new machinery — it is exactly [[pattern]]'s existing
   `$` rewrite, reused.
2. **Length guard lives outside the regex.** `total-len-253` (accept) vs
   `total-len-254` (reject) — every label ≤ 63 in both, so only the guard
   distinguishes them. All seven agree, confirming the `len()` check is a
   faithful, portable substitute for the RE2-impossible total-length lookahead.

## Why we OWN the check (no native validator)

Native hostname validators are absent or divergent — the exact P1 hazard the
`format` spec cites for not delegating:

- **Python / Go / Ruby / TS/JS / Rust:** no stdlib RFC-1123 *hostname* validator
  at all (Python `ipaddress` is IP-only; Java `InetAddress` does a *DNS lookup*,
  a network call, not syntax; `java.net.IDN` is IDN transcoding, not validation).
- **.NET has one — and it disagrees.** `Uri.CheckHostName` (probed): accepts
  `host_name` (underscore) as `Dns`, accepts trailing-dot `example.`, and
  classifies `999` as `IPv4` (not a DNS name) — **three** divergences from the
  pinned grammar in one small probe.
- **ajv (`ajv-formats`), the most-used JS validator:** uses a lookahead-based
  regex (`(?=.{1,253}\.?$)…`), *accepts* trailing dot, does **no** Punycode
  decode (so `xn--X` passes), and does not enforce all-numeric-TLD — i.e. even
  the popular reference is the plain LDH grammar, not the strict test-suite
  grammar, and still differs from us on the trailing dot.

Delegating to any of these would make one target accept what another rejects.
Owning a single pinned regex + guard is the only way to guarantee P1.

## Residual risks

1. **All-numeric TLD accepted** (`999`, `123.456`). RFC 1123's dotted-decimal
   note is not enforced (not RE2-expressible in general; ajv also skips it). An
   all-numeric host that is *also* a valid IPv4 is still a legal LDH string.
   Low impact — if ever needed, a cheap non-regex predicate ("at least one
   label contains a letter") could be added to the shared `Validate` and
   verified against this corpus, exactly like the temporal calendar predicate.
2. **Trailing dot rejected** — a deliberate pin matching the test suite; note it
   *diverges from ajv*. If a source ecosystem needs the FQDN-root trailing dot,
   it is a one-token regex change (`(?:\.)?$`) re-verified against the corpus.
3. **A-labels not IDNA-validated.** `xn--`-prefixed garbage passes as an LDH
   label. This is `idn-hostname`'s job and stays deferred; documented so it is a
   conscious boundary, not an oversight.
4. **Ruby toolchain** here is 2.6 (system). Onigmo's `\A`/`\z` and explicit
   ASCII class behave identically on newer Rubies; the explicit class avoids the
   `\b`/`\w` Onigmo quirks [[pattern]] hit, so no `(?a)` flag is needed.

## Recommendation

**Support `hostname`** via the pinned RE2-safe regex above **plus the 1..=253
code-point length guard**, both in the shared `Validate` (identical both
directions, P12), lowering through the existing [[pattern]] gate + `$`-anchor
normalization — no new machinery, no new dependency (P4). Verified identical
across all seven targets (41/41). Keep `idn-hostname` deferred. Adopt the pinned
edge decisions (reject trailing dot; accept all-numeric and `xn--` LDH labels)
as the owned P1 line, and keep this corpus as the regression guard.

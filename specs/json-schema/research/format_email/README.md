# `email` format cross-language conformance

Empirical study answering `format`'s **open question 1** ("widen the asserted
subset" — is `email` supportable?) and **open question 2** ("build the format
conformance corpus"). It mirrors `research/pattern_conformance/`: a shared
`(instance, expect_valid)` corpus run through the same seven engines
(Rust gate + Go, JS, Python, Java, Ruby, .NET) under the pinned per-target
`pattern`-gate recipe.

## The question

JSON Schema 2020-12 §7.3.2 defines `email` by reference to the RFC 5321 mailbox
(`Local-part "@" ( Domain / address-literal )`). RFC 5321/5322 admit quoted
local parts, comments, IP-literal domains (`[192.0.2.1]`), and (via RFC 6531)
internationalized addresses — a large, ambiguous language that no two native
validators implement the same way (see `native_validators_probe.md`). Can we
instead pin a **single, well-defined, RE2-safe subset** and prove every target
implements it to identical verdicts, with no new dependency?

## The pinned check under test

A fully-anchored regex capturing an **unquoted ASCII dot-atom** local part, a
single `@`, and a **hostname-style domain** (letter/digit/hyphen labels,
dot-separated, ≥ 2 labels, each label 1–63 chars, no leading/trailing hyphen):

```
^[a-zA-Z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-zA-Z0-9!#$%&'*+/=?^_`{|}~-]+)*@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+$
```

It uses **only explicit ASCII character classes** — no `\d` `\w` `\s` `\b`, and
**no bare `.`** — so it sidesteps almost every `pattern`-gate divergence axis
(ASCII-class, astral-`.`, `\s` scope). The **only** gate transform that applies
is the `$` end-anchor normalization (`$`→`\Z` Python, `$`→`\z` Java/.NET,
`^`→`\A`/`$`→`\z` Ruby); each runner applies its target's rewrite.

**Deliberately excluded** (rejected): quoted local parts, comments, IP-literal
domains, single-label domains (`a@b`, `user@localhost`), trailing dot, any
whitespace / control char, and all Unicode / IDN (that is the separate
`idn-email` format). `atext` per RFC 5321 §4.1.2 is included in the local part.

## Files

- `corpus.json` — 56 `(instance, expect_valid)` pairs (12 valid, 44 invalid),
  including tricky forms: quoted/comment locals, IP-literals, IDN/Unicode,
  leading/trailing dots and hyphens, double dots, whitespace, control chars,
  64-char over-long label, bare-IPv4 domain.
- `gate_runner/` — Rust `regex`-crate runner = the **load-time gate** *and* a
  runtime engine. `cargo clean` keeps the tree light.
- `runner.go`, `runner.mjs`, `runner.py`, `Runner.java`, `runner.rb`,
  `dotnet_runner/EmailRunner/` — one per target engine, each applying its pinned
  anchor/flag recipe. Emit JSON Lines `{id, engine, compiled, matched}`.
- `compare.py` — builds/runs all seven, checks (a) compile-acceptance,
  (b) seven-way verdict agreement, (c) agreement matches `expect_valid`.
  Exits nonzero on any divergence.
- `redos_probe.py` — adversarial long-input probe (see finding below).
- `native_validators_probe.md` — why `.NET MailAddress` / Pydantic `EmailStr` /
  Java `@Email` / Go `net/mail` are unsuitable as the source of truth.

## Run

```sh
cd json-schema/research/format_email
python3 compare.py     # all seven engines
python3 redos_probe.py # adversarial-input behavior
```

## Findings

**1. The pinned regex compiles in all seven engines and produces IDENTICAL
verdicts on all 56 instances, matching intent (56/56).** `compare.py` reports
zero compile failures, zero verdict divergences, zero intent mismatches. So the
*semantic* portability question is a clean PASS — a single owned regex gives
identical accept/reject in Rust, Go, JS, Python, Java, Ruby, and .NET, with no
dependency beyond each stdlib regex engine (P4), reusing the `pattern` gate
wholesale.

**2. One implementation hazard: Java `StackOverflowError` on pathological
input.** The regex is *not* ReDoS-vulnerable — every backtracking engine scales
linearly to 100 000 chars (each quantified group has a distinct leading token,
so there is no ambiguous backtracking). BUT `java.util.regex` matches nested
quantifier loops **recursively**, and a long dot-atom run (`a.a.a…`, ~3 000–8 000
atoms, i.e. ~6–16 kB, nondeterministic) overflows the JVM stack — a crash, not a
clean "invalid" verdict, so a P1 hazard on adversarial input. Go and Rust (RE2
family) are linear-time and never recurse on input; JS, Python, Ruby, .NET all
stay linear with no crash.

**Mitigation (verified):** a **mandatory length pre-check** at the RFC 5321
address cap (254 chars; 320 also safe) runs *before* the regex and keeps every
input orders of magnitude below Java's stack threshold (worst-case 254-char
input matches in µs with no overflow). This is the same "gate makes the emitted
check unconditional" posture `pattern` uses, and it is independently sensible
(RFC 5321 caps addresses anyway). With the guard, all seven engines agree at
every reachable length.

## Verdict

`email` **is supportable** as an asserted format via a **generator-owned pinned
regex + a mandatory length guard**, with identical verdicts across all seven
targets and no new dependency — provided the check is documented as a
**well-defined ASCII subset** of RFC 5321 (no quoted locals, comments,
IP-literals, or IDN; `idn-email` stays deferred). See the top-level report for
the recommended spec change.

# `uri` format — cross-language conformance study

Empirical study answering **format spec open-question 1** for the URI family:
can `format: "uri"` (JSON Schema 2020-12 §7.3.5 = RFC 3986 URI, **scheme
required**) be supported via a **generator-owned pinned check** at acceptable
fidelity, agreeing IDENTICALLY across all 7 targets (Go, TypeScript/JS, Python,
Java, Ruby, .NET, plus the Rust load-time gate) — as **P1** demands — or should
it stay deferred?

Two independent experiments:

## 1. Native URI parsers are unusable for P1 (`native_probes/`)

`native_inputs.json` — 57 tricky candidate URIs. Each language's *own* native
URI parser (the thing a naive `format:uri` implementation reaches for) is asked
"is this a valid absolute URI?":

- Go `net/url.Parse` + `IsAbs()`
- JS WHATWG `new URL()`
- Python `urllib.parse.urlsplit` + truthy `scheme`
- Java `java.net.URI.isAbsolute()` (and `java.net.URL` for contrast)
- Ruby `URI.parse` + `absolute?`
- .NET `Uri.TryCreate(UriKind.Absolute)`

Run: `python3 native_probes/compare_native.py`

**Result: 27 of 57 inputs get divergent verdicts across the 7 parsers.** They
disagree on percent-encoding correctness (`%2`, `%`, `%zz`), illegal path chars
(`|`, `[]`, `^`, `"`), control chars, non-ASCII (IRI) input, IPvFuture,
unbracketed IPv6, empty ports, `http:` with empty path, trailing/leading space,
double-`@` userinfo, and more. Python's `urllib` almost never fails (accepts raw
spaces, bad pct, non-ASCII); WHATWG `URL` silently NORMALIZES (rewrites `\`→`/`,
percent-encodes spaces, punycodes hosts, drops control chars) so it never
reports the ORIGINAL string invalid; .NET normalizes and even reinterprets
`/a/b/c` as `file:///a/b/c`; Java `URI` is the strictest; Ruby rejects non-ASCII
and most bad chars but accepts IPvFuture. No native parser is a viable P1 oracle.

## 2. A pinned RE2-safe check IS identical across all 7 engines (this dir)

`pinned_regex.txt` — human-readable derivation. `pinned_body.json` — the actual
anchor-less regex body (an RFC-3986-faithful, ASCII-only, no-lookaround /
no-backreference / no-inline-flag pattern; in the RE2 / regular family, so it
compiles under the Rust `regex` gate). Each runner wraps the body in its own
portable full-input anchor:

- Go/JS: `^…$` (end-of-text, no `m` flag)
- Python: `\A…\Z` + `re.ASCII`
- Java: `\A…\z` (default flags)
- Ruby: `\A…\z`
- .NET: `\A…\z` + `RegexOptions.ECMAScript`
- Rust gate: `^…$` (compile = RE2-safety proof; also matches)

`corpus.json` — 72 `(value, expect)` pairs. Run: `python3 compare.py`

**Result: all 7 engines COMPILE the pinned regex and AGREE on all 72 values,
matching the intended verdict exactly (72/72, 0 divergences, 0 compile
failures).** Backtracking engines (Python/Java/JS/.NET/Ruby) run in linear time
— the alternations are unambiguous (pct-encoded starts with `%`, disjoint from
the char classes), so there is no ReDoS (verified to 50k-char adversarial
near-miss inputs, sub-millisecond).

Fresh transcript (`python3 compare.py`):

```
total pairs: 72
--- (a) compile-acceptance: all 7 engines compile the pinned regex ---
  OK: rust/go/js/python/java/ruby/dotnet all compiled the pinned pattern.
--- (b) match-agreement: all 7 engines agree per value ---
  OK: all 7 engines agreed on every corpus value.
--- (c) expectation check: agreed verdict == corpus `expect` ---
  OK: agreed verdict matched `expect` for every value.
--- summary ---
  compile failures:     0
  match divergences:    0
  fully agreeing pairs: 72/72
VERDICT: PASS - pinned check is identical across all 7 engines
```

`pinned_body_uriref.json` — the `uri-reference` variant (scheme optional +
relative-ref with `segment-nz-nc`). Also compiles in all 7 engines; a clean
derivation from the same building blocks.

### IPv6 IP-literal host is validated semantically

The bracketed IPv6 IP-literal host is checked with the **full RFC 4291 IPv6
grammar** (full, `::`-compressed, and IPv4-tail forms), spliced in VERBATIM from
the pinned `ipv6` format check in `research/format_conformance/` so the URI check
and the standalone `ipv6` check agree byte-for-byte on what a valid IPv6 address
is. Consequences: `http://[zzzz]` (bad hex), `http://[1::2::3]` (double `::`),
and `http://[1:2:3:4:5:6:7:8:9]` (nine groups) are **rejected**, while
`http://[2001:db8::1]`, `http://[::1]`, `http://[fe80::1]`, and IPv4-tail
`http://[::ffff:192.0.2.1]` pass. The combined pattern stays RE2-safe and
compiles under the Rust `regex` gate and all six other engines. The IPvFuture
alternative (`\[v…\]`) remains permissive/structural (RFC 3986 leaves its payload
version-defined). Everything else tested is faithful: scheme rules, pct-encoding
(`%HEXDIG HEXDIG` only), reg-name/sub-delims/pchar char classes, port `*DIGIT`,
the `//authority` vs path-absolute vs path-rootless hier-part split, ASCII-only
(non-ASCII = IRI, rejected), and `999.999.999.999` correctly accepted as a
reg-name (RFC 3986 permits it).

## Files

- `native_inputs.json`, `native_probes/native.{go,mjs,py,rb}`,
  `native_probes/NativeProbe.java`, `native_probes/DotnetNative/`,
  `native_probes/compare_native.py` — experiment 1.
- `pinned_regex.txt`, `pinned_body.json`, `pinned_body_uriref.json`,
  `corpus.json`, `runner.{go,mjs,py,rb}`, `Runner.java`, `rust_runner/`,
  `dotnet_runner/`, `compare.py` — experiment 2.

Run `cargo clean` in `rust_runner/` and delete `bin/`/`obj/` under the dotnet
dirs to keep the tree light (only sources are tracked).

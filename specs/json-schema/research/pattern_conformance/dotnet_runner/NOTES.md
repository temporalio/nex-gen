# .NET (C#) `pattern` conformance — empirical findings

Prospective 5th target for the JSON-Schema `pattern` generator. Tested with
`dotnet` 8.0.128, `System.Text.RegularExpressions`.

## Layout

- `DotnetRunner/` — the conformance runner. Reads `../corpus.json`, applies the
  best pinned-semantics config, emits JSON Lines
  `{"id","engine":"dotnet","compiled","matched","normalized"}`.
- `probe/` — a throwaway program that establishes each axis empirically
  (anchoring, `$`/`\n`, `\d\w\s` scope, astral `.`, lookaround). Re-runnable.
- `compare_dotnet.py` — runs the Go/JS/Python reference runners + the .NET
  runner, applies the final gate exclusions, and reports divergences.

Build artifacts (`bin/`, `obj/`) are git-ignored; `dotnet clean` +
removing those dirs keeps the tree to source only.

## Run

```sh
cd json-schema/research/pattern_conformance/dotnet_runner
dotnet run --project DotnetRunner -- ../corpus.json     # raw runner
python3 compare_dotnet.py                                # vs Go/JS reference
dotnet run --project probe                               # per-axis probe
```

## Best pinned-semantics config for .NET

```csharp
// unanchored search + ASCII \d\w\s + $ pre-rewritten to \z
Regex.IsMatch(instance, patternWithDollarRewrittenToBackslashZ,
              RegexOptions.ECMAScript);
```

- `Regex.IsMatch(input, pat)` is **unanchored** (search, not full match).
- `RegexOptions.ECMAScript` restricts `\d \w \s \D \W \S` to **ASCII**.
- The gate must rewrite a trailing `$` anchor to **`\z`** (end-of-input only).
  In .NET, `\z` = end-of-input; `\Z` = before an optional final `\n`
  (opposite convention from Java, where `\z` is end-only and `\Z` allows the
  trailing `\n`; happens to be the same letter Java uses for end-only).

## Per-axis empirical results (quoted from `probe`)

Anchoring — unanchored:
```
IsMatch("the cat sat", "cat") = true
IsMatch("category", "cat")    = true
```

`$` + trailing `\n` — raw `$` matches before final `\n` (like Py/Java); `\z`
is end-of-input only (matches Go/JS pinning); `\Z` does NOT (allows final `\n`):
```
$ : IsMatch("cat\n","^cat$")  = true    # divergent (Go/JS = false)
\z: IsMatch("cat\n","^cat\z") = false   # correct end-of-input construct
\Z: IsMatch("cat\n","^cat\Z") = true    # WRONG for our pinning
```

`\d \w \s` scope — default Unicode, ECMAScript = ASCII:
```
\d default vs Arabic U+0663    = true    # Unicode
\d ECMAScript vs Arabic U+0663 = false   # ASCII  (want false)
\d ECMAScript vs ASCII 5       = true
\w default vs eacute U+00E9    = true
\w ECMAScript vs eacute U+00E9 = false   # want false
\w ECMAScript vs cyrillic/cjk  = false
\D ECMAScript vs Arabic U+0663 = true    # want true
\W ECMAScript vs eacute        = true    # want true
```

Astral `.` — **DIVERGENCE**: .NET `.` matches ONE UTF-16 unit; a surrogate
pair (e.g. U+1F600) is 2 units, so `^a.b$` fails. This is the JS-without-`u`
behaviour, and .NET has **no `u`/code-point flag** (`RegexOptions.Singleline`
does not help):
```
. : ^a.b$ on "a😀b"        = false   # reference (Go/JS/Py/Java) = true
. Singleline: ^a.b$        = false
```
Only workaround is a per-target pattern rewrite of every `.` to a
surrogate-aware group, which works for both astral and BMP:
```
. -> (?:[\uD800-\uDBFF][\uDC00-\uDFFF]|.)
   ^a(?:...)b$ on "a😀b"   = true
   ^a(?:...)b$ on "a中b"   = true   # BMP still one unit
```

Lookaround / backref — all compile (backtracking engine), so they are only
excluded because the load-time gate already rejects them:
```
foo(?=bar) ~ foobar = true
foo(?!bar) ~ foobaz = true
(?<=x)y   ~ xy      = true
(a)\1     ~ aa      = true
```

## Comparison result (`compare_dotnet.py`)

72 gate-accepted pairs (83 total minus lookaround/backref/inline-flag/`\s`-`\S`).
**70/72 agree** with the Go/JS reference. The only two divergences are the
astral-`.` pairs:
```
DIVERGENCE dot-astral-emoji: a.b ~ "a😀b"  ref=True  dotnet=False
DIVERGENCE dotstar-astral:   ^a.$ ~ "a😀"  ref=True  dotnet=False
```

## Verdict

.NET is future-conformant with the current recipe **plus one additional
per-target normalization**:

1. `$` → `\z` (end-of-input) — same shape as the existing Python (`\Z`) / Java
   (`\z`) normalization; already handled by the current gate design.
2. `RegexOptions.ECMAScript` for ASCII `\d\w\s` — no pattern rewrite needed.
3. **NEW requirement:** astral `.`. Either (a) rewrite every unescaped `.` in
   the pattern to `(?:[\uD800-\uDBFF][\uDC00-\uDFFF]|.)` for the .NET target
   (heavier than any current target needs, since Go/JS-`u`/Py/Java are already
   code-point), or (b) gate-reject `.` when astral instances are possible
   (not generally decidable) — so option (a) is the practical path. With the
   `.`-rewrite, .NET would reach 72/72.

Everything else (anchoring, ASCII classes via ECMAScript, `$`→`\z`, and the
already-gated lookaround/backref/inline-flag/`\s`) conforms with no surprises.

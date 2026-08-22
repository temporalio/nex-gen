# python-emitter — execution report

Files touched (owned): `src/generator/json_schema/python.rs`,
`src/generator/python.rs`, `tests/generate_python.rs`. Nothing else.

`cargo build --all-features` clean; the three files are `rustfmt --check` clean.
`cargo test --all-features --test generate_python` → **29 passed, 2 failed**, and
both failures are the golden-snapshot tests listed under *Snapshot shifts*.
`cargo test --all-features --lib` → 478 passed.

---

## Fixed

### `12#1` — P0, docstring escaper missed a trailing `"`
`src/generator/python.rs:7013` (`python_docstring_literal_text`). The two existing
passes are kept; a final `"` that is not already escaped now gets one, decided by
counting the trailing backslashes before it (so a body ending in `"""`, which the
second pass already rewrote to `\"\"\"`, is left alone, and a body ending in a
literal `\` + `"` — which escapes to `\\"` — is still handled).

Verified: generated an unformatted package from a schema whose model, attribute,
service and operation descriptions all end in `"`; before the fix
`python3.11 -m py_compile` reported `SyntaxError: unterminated string literal` on
`models.py:60` and `services.py:11`. After, all four modules compile and the
docstrings read back byte-exact. Also probed eight adversarial endings
(`"`, `""`, `"""`, `\`, `\"`, `"\`, `""""`, an interior `"`) — all parse and all
preserve their text exactly (checked via `ast` on the emitted source).

### `05#7` — P1, nested same-quote f-string is a `SyntaxError` below 3.12
`src/generator/json_schema/python.rs:4722`. The raw-array cast interpolated into
`render_py_array_checks`'s `f"…"` reasons is now spelled
`typing.cast('list[typing.Any]', …)`.

Verified against real interpreters, on **unformatted** output: before,
`python3.11 -m py_compile` → `SyntaxError: f-string: unmatched '('`; after, both
`python3.10` and `python3.11` compile. Confirmed `ruff format` (samples' config,
`target-version = "py310"`) leaves the single quotes inside the f-string and
re-normalises the occurrences outside it to `"`, so the *committed* samples do not
move for this change.

Also confirmed that the existing `assert_python_310_syntax_compatible` helper
**cannot** catch this: `ast.parse(src, feature_version=(3, 10))` on a 3.13 host
accepts a PEP 701 nested f-string. Added `assert_python_floor_compiles`, which
byte-compiles the generated package with `uv run --python 3.10 --no-project`
(~0.1 s) and does reject it, and documented the gap on the old helper.

### `14#5` — P0, deprecated operation made a definitions-only package unimportable
`src/generator/python.rs:4171` — `uses_typing` now also fires on
`operation.deprecated`. Verified: generated with `generate_native_api: false`;
before, `import out.services` raised
`NameError: name 'typing' is not defined` (reproduced by deleting the import from
the fixed output); after, the package imports and `DepService` resolves.

### `09#4` — P0, Python temporal regexes skipped the `\Z` rewrite
`src/generator/json_schema/python.rs:965` (`render_temporal_helpers`) now routes
all four `TemporalKind::*.pattern()` through
`pattern::rewrite_end_anchor(.., r"\Z")`, like every other Python pattern site.

Verified end-to-end. Against the *checked-in* sample
(`samples/python/temporal/_definitions.py`): `_parse_duration("PT1H\n")` raised
`KeyError('\n')` and `_parse_date_time("2021-06-15T12:30:45Z\n")` raised
`ValueError: Invalid isoformat string` — both escaping the aggregation (P11).
After regeneration all four helpers return `None` with exactly one
`Violation` (`must be a valid duration, got "PT1H\n"`, etc.).

### `07#3` — P0, `number` bounds were compared as exact integers
`src/generator/json_schema/python.rs:101` — new `py_bound_compare_literal`
(rounds a `number` bound to binary64 and emits it with Rust's shortest
round-tripping `f64` spelling, which always carries a `.` or an exponent) plus a
`float(...)`-narrowed subject in `render_py_numeric_checks`. `integer` is
untouched and stays exact. **The reason string keeps the authored spelling**
(`must be >= 5`, not `5.0`), so the four targets still print identical text and
`samples/python/tests/test_showcase.py` needs no change.

Verified at runtime: with `maximum: 9007199254740992` on a `type: number` field,
wire `9007199254740993` is now **accepted** (as in Go/TS/Java) and
`9007199254740995` is rejected. Emitted shape:
`if float(n_value_raw) > 9007199254740992.0:`.

### `13#3` — P0, no serialize-side ±(2^53−1) integer cap
`src/generator/json_schema/python.rs` — `py_field_needs_serialize_check` now
returns `true` for every `integer` (as it already did for `number`), and the
serialize arm of `render_py_field_checks` emits
`if abs(<value>) > 9007199254740991:` with Go's exact reason,
`"exceeds ±(2^53-1) integer cap"`. It covers declared properties, typed-map
members, array elements (including nested — `grid[0][1]`) and union branches.

To keep it off the parse side (where `_parse_spec_integer` and the union integer
token guard have already asserted the cap, so a second check would be dead code),
`render_py_field_checks` gained a `side: PyCheckSide` parameter. Verified:
`R(i=9007199254740993).to_transfer_type()` now raises `ValidationError` with that
single violation; previously it emitted an integer its own parser rejects.

### `06#1` — P0, matcher type guard derived from the first `const`/`enum` literal
`src/generator/json_schema/python.rs:4946`. The matcher kind is now the declared
`type`, else the element type (unwrapping a nullable element per **D2**). The
`const`/`enum`-first-literal fallback is gone, and the now-dead
`scalar_kind_for_value` was removed.

Verified: `items: {type: number}` with `contains: {enum: [2, 1.5]}` and with the
members reversed both accept wire `[1.5]` — the order-dependence is gone and
Python agrees with Go/TS/Java. The emitted guard is the Number one
(`-1.797…e308 <= element <= 1.797…e308`), no `float(element).is_integer()`.

### `04#6` / `11#13` — P2, uninformative `propertyNames` + `enum` reason
`src/generator/json_schema/python.rs` now emits
`invalid property name "gamma": must be one of ["alpha", "beta"], got "gamma"`,
matching TypeScript (`typescript.rs:760`) and Java. `tests/generate_python.rs:330`
updated to the new text (it is the only assertion on the old wording).

### `12#5` — P1, union `TypeAlias` dropped `title` and `deprecated`
`src/generator/json_schema/python.rs:688`. The docstring is now composed through
`compose_python_doc(title, description)`, and a deprecated union emits
`U: typing.TypeAlias = typing.Annotated[str | int,
typing_extensions.deprecated("This type is deprecated.", category=None)]`.
`typing_extensions` is picked up automatically by the body-scanning import writer.
Verified by importing the generated module.

### `14#9` — P2, empty `models.py` for a service-only module
`src/generator/python.rs:738` / `:3841`. `render_models_module` returns
`Option<String>` and yields `None` when its body is empty; the caller then writes
no file. Verified with a two-file closure: `svc/models.py` is gone, `svc/__init__.py`
is unchanged (it never imported from it) and the package still imports.

### `04#1` — P0, `propertyNames` guard/body mismatch (empty `for` → `IndentationError`)
Fixed defensively at `src/generator/json_schema/python.rs:1752`: the guard now
tests `format::check_for(format).is_some()` and "the `enum` holds at least one
string member", matching the body term-for-term.

**Note:** by the time I probed it the shape was already unreachable — the loader
agent has landed **D6** *and* a vacuity reject, so both
`propertyNames: {type: string, format: date-time}` and
`propertyNames: {type: string}` now fail at load
("`propertyNames` asserts nothing…" / "`format: date-time` is not supported…").
The emitter guard is therefore second-line defence. My test asserts the two load
rejections rather than an emission, so it will fail loudly if the loader is ever
loosened without the emitter following.

---

## Not fixed

### `13#5` — P1, a wire `int` is stored in a `type: number` field
**Deliberately not fixed — it is a cross-language decision, not a Python bug.**

The obvious fix (`target = float(raw)` on the `number` parse path) changes
Python's **wire output**, and the four targets already disagree there:

| wire in | Go out | TS out | Java out | Python out (today) | Python out (if coerced) |
|---|---|---|---|---|---|
| `3`   | `3` | `3` | `3.0` | `3` | `3.0` |
| `3.0` | `3` | `3` | `3.0` | `3.0` | `3.0` |
| `9007199254740993` | `9007199254740992` | `9007199254740992` | `9.007199254740992E15` | `9007199254740993` | `9007199254740992` |
| `2**60` | `1152921504606846976` | `1152921504606846976` | — | same | `1.152921504606847e+18` |

Measured (`json.dumps(..., separators=(",",":"))` on the generated converter's
output). So coercing fixes row 3 and **breaks** rows 1 and 4, and
`samples/python/tests/json_converter_helper.py:149-152` states in so many words
that the numeric form is part of the asserted wire contract
(`test_wire_fixtures.py` compares bytes).

What this actually needs is a shared rule for *writing* a `number` (Go/TS write
the shortest integral spelling; Java always writes `.0`; Python follows whatever
type it holds) — i.e. a Wave 0.1 conformance-manifest decision, then one change per
emitter. **Recommendation: add a D-series entry.** Note that `07#3` above already
removes the *validation* half of this divergence; what remains is only the value
Python hands back and re-serialises.

### `06#2` — P0, fractional matcher bound over `integer` elements
**Confirmed correct in Python; left as-is, as instructed.** With
`items: {type: integer}` and `contains: {type: number, minimum: 1.5}` Python emits
`element >= 1.5` (untruncated) in both directions. Go/TS/Java-typed truncate to
`>= 1`.

**Coordination note for the other emitters:** Python is untruncated only when the
matcher declares its own `type`. When the matcher is *typeless* the kind falls
back to the element type, `is_integer` becomes true, and `py_bound_literal`
truncates — so `contains: {minimum: 1.5}` over `items: {type: integer}` still
compares `>= 1` in Python. If Go/TS/Java converge on "never truncate a matcher
bound", Python needs the matching one-line change (drop `is_integer` from the four
`py_bound_literal` calls at `python.rs:5007-5019`, keeping it only for the
`%`-vs-`math.fmod` operator choice). I did not make it unilaterally because it
would put Python out of step with three emitters mid-rollout.

---

## Cross-file requests

None. Everything I needed was in my three files.

---

## Sample schema requests

None strictly required. Two would add real coverage if you want them in
`samples/schemas/showcase.nexusrpc.yaml` (all four languages benefit):

```yaml
      # A description that ends in a double quote — the shape that made Python's
      # generated module unparseable (12#1).
      quoted:
        description: 'Ends with a quote "'
        type: string
      # A `number` bound past 2^53, where exact-integer and binary64 comparison
      # disagree (07#3).
      wideBound:
        description: Optional number bounded at the largest exact binary64 integer.
        type: number
        maximum: 9007199254740992
```

---

## Snapshot shifts

`tests/generate_python.rs::python_json_example_generation_matches_checked_in_output`
and `::python_json_api_example_generation_matches_checked_in_output` will move.
Regeneration will change, under `samples/python/`:

- `showcase/models.py` — serialize-side `exceeds ±(2^53-1) integer cap` checks on
  every `integer` member (declared, map value, array element, union branch), and
  `Settings.to_transfer_type` gains a `violations` list + `raise` it did not have;
  `< 5` → `float(...) < 5.0` and `math.fmod(..., 5)` → `math.fmod(float(...), 5.0)`
  on the `ratio` member.
- `temporal/_definitions.py` (and any other package with temporal helpers) — the
  four `_TEMPORAL_*_RE` patterns end in `\Z` instead of `$`.
- Any package with array count bounds — the raw-array cast inside an f-string
  reason is single-quoted (this is already what `ruff` produced, so those lines
  should come out byte-identical; the *unformatted* generator output is what
  changed).

I regenerated `showcase` and `temporal` into a scratch tree and ran the full
`samples/python` suite against them: **75 passed**, including
`test_wire_fixtures.py` (byte-identical round-trip) and all 32 showcase tests. The
checked-in samples themselves were left untouched, as instructed.

---

## Tests added (all in `tests/generate_python.rs`)

| Test | Guards |
|---|---|
| `python_docstrings_ending_in_a_quote_parse_and_keep_their_text` | `12#1` — compiles on the 3.10 floor **and** the docstrings read back exactly |
| `python_json_definitions_only_deprecated_operation_imports_typing` | `14#5` — `generate_native_api: false`, asserts the import and *imports* the package |
| `python_json_array_bound_reasons_parse_on_the_declared_floor` | `05#7` — unformatted output, real 3.10 interpreter |
| `python_json_temporal_patterns_reject_a_trailing_newline` | `09#4` — `\Z` in all four patterns, and one aggregated `Violation` per parse helper |
| `python_json_number_bounds_compare_in_binary64_and_integers_are_capped` | `07#3` + `13#3` |
| `python_json_contains_matcher_kind_ignores_the_literal_set` | `06#1` — both enum orderings accept `[1.5]` |
| `python_json_property_names_without_a_key_predicate_are_refused` | `04#1` — the two load rejections that keep the shape off the emitter |
| `python_json_union_alias_keeps_its_title_and_deprecation` | `12#5` |
| `python_json_service_module_without_own_types_does_not_reemit_refs` (extended) | `14#9` — asserts `svc/models.py` is absent |

New helper `assert_python_floor_compiles` byte-compiles a package with a real
3.10 interpreter via `uv run --python 3.10 --no-project`. Verified it rejects the
pre-fix nested f-string (`SyntaxError: f-string: unmatched '('`) where
`assert_python_310_syntax_compatible` accepts it. The conformance agent's new
`tests/json_schema_probe_matrix.rs` does the same thing at matrix scale; the two
are complementary (mine pins the specific shape per finding).

---

## Incidental notes

- The build broke transiently three times during this run from concurrent
  in-flight edits in `java.rs` / `go.rs`; each cleared on retry. Nothing of mine.
- `contains` matcher **bounds** still use `py_bound_literal` (exact), not the new
  binary64 `py_bound_compare_literal`, so `contains: {type: number, minimum: N}`
  over a raw wire array carries the same exact-vs-binary64 hazard `07#3` fixed for
  declared fields. It was outside my brief and Wave 4.4 is being converged by
  several agents at once; flagging it as a follow-up.

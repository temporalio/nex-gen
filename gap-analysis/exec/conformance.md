# conformance — Wave 0

Three new test binaries turn the four reports' "nothing executes this" findings
into executed checks. All three are green; every divergence they found is either
fixed upstream already or pinned to a finding with a marker that fails the build
the day it is fixed.

    tests/toolchain/mod.rs                     shared harness (generate + drive 4 toolchains)
    tests/json_schema_conformance_manifest.rs  0.1 the manifest, executed
    tests/json_schema_probe_matrix.rs          0.2 does generated output compile / import
    tests/json_schema_corpus_runtime.rs        0.3 the pinned corpora, through 4 runtimes
    samples/conformance/schemas/*.yaml         9 new conformance schemas
    samples/conformance/runners/*              4 generic runners + 2 import smokes
    samples/conformance/json-schema.json       manifest v2: 4 cases -> 13, all executed

Runtime: 15 s + 8 s + 4 s. Java's classpath is read once from Gradle through an
init script (no `build.gradle` edit) and cached under `target/`.

## Fixed

### 0.1 The manifest is executable — `13#gap-9`
`tests/json_schema_conformance_manifest.rs:611` generates every case into all
four targets, drives every `accepted_wire_values` / `parse_failures` /
`serialize_failures` through the **generated code**, and asserts the verdicts
agree with the manifest *and with each other*. Violation **paths** are compared,
never reason text (P11).

Design choices worth knowing:

- **Scratch workspaces, never the samples.** Go copies `samples/go/go.mod`
  under a neutral module path (definitions-only output is stdlib-only; a schema
  with a service needs nexus-rpc, which resolves from the module cache);
  TypeScript symlinks `samples/typescript/node_modules`; Python uses
  `samples/python/.venv`; Java uses the Gradle-resolved classpath. Nothing is
  generated into committed sample output, so the harness is immune to the
  regeneration pass and to the other six agents.
- **Java runs once over a batched plan**, not per case. Same for Go and vitest.
- **A build break is a per-case verdict.** Go and Java link a package set, so a
  failed whole-set build is retried package by package; the offending case
  reports "generated code does not build" and the other twelve still run.
  Python and TypeScript isolate naturally (import inside a try, lazy `import()`).
- **`permitted_presence_nullability_collapse` is closed**, modelled on
  `samples/python/tests/test_wire_fixtures.py:130`. A member not listed must
  round-trip with its presence intact in every target; a member listed must
  actually collapse in every target it names, or the declaration is stale and
  fails (`check_collapse_declarations`, `:1006`).
- **Serialize-side rejections** need a native value no parser produces, so a
  case declares `from_wire` + a tiny mutation vocabulary (`set_integer`,
  `set_number` incl. `inf`/`nan`, `set_string`, `set_null`,
  `duplicate_element`) over an `a.b[0][1]` path. All four runners implement it
  by reflection — Go by `json` tag, Java by declared field, Python by
  snake_case, TypeScript directly.
- **xfail with teeth.** `expected_divergence.matches` classifies which findings
  a case is allowed to produce. An unmatched finding fails; a matcher that stops
  matching fails; a case that starts passing fails. A marker cannot outlive its
  bug.

### 0.1b Cases: 4 -> 13 — `01#gap-7 03#gap-3 05#gap-5 06#gap-3 07#gap-1 10#gap-3 11#gap-18 13#gap-1 13#gap-2 13#gap-4`
New schemas under `samples/conformance/schemas/`, each a single model named
after its file stem so the name is identical in all four targets:

| case | pins |
|---|---|
| `nullability-matrix` | optional x nullable x 3 presences over non-string inner types, constraint on every non-null branch (`13#gap-1`, `13#gap-2`) |
| `union-token-selection` | integer and boolean `const` tags, `{"kind":1.0}` == `{"kind":1}`, unknown/absent tag, string/integer/array/object branch selection (`01#gap-7`) |
| `numeric-bounds` | the **inclusive boundary accepted** (asserted nowhere before), `multipleOf` at 1e23/1e300, a `number` bound against an integral token above 2^53 (`07#gap-1`) |
| `integer-semantics` | `1 1.0 1e2 -0 +-(2^53-1)` in, `1.5 true "1" 2^53 2^53+1 4503599627370496.5` out, plus the serialize-side cap (`13#gap-4`, `13#3`) |
| `unique-items-equality` | `[1,1.0]`, `[5,5e0]`, `[-0.0,0.0]` parse **and** serialize (`05#gap-5`, `05#2`) |
| `closed-values` | off-set `enum`/`const`, including one nullability wrapper deep (`11#gap-18`, `11#1`) |
| `object-modeling` | closed rejection, open-key preservation, typed-map member checks, missing-required aggregation across siblings (`03#gap-3`) |
| `content-encoding-canonical-form` | canonical / URL-safe / padding / whitespace / trailing-bits / wrong-alphabet accept-reject line (`10#gap-3`) |
| `contains-matcher` | **one** matcher schema through all four predicates (`06#gap-3`) |

The four legacy cases now execute too, including the serialize failure that was
previously the prose string `"replace numberGrid[0][1] with a non-finite
number"`.

### 0.2 The probe matrix — `10#gap-11`, `05#7`
`tests/json_schema_probe_matrix.rs`: 16 adversarial schemas x 4 targets, over
**unformatted** output.

| target | build | evaluate |
|---|---|---|
| Go | `go vet` (type-checks, so it subsumes `go build`) | — |
| Java | `javac --release 8` (asserts the declared baseline) | — |
| Python | `py_compile` on **3.10 and 3.11 as well as the venv's 3.13** | module import |
| TypeScript | `tsc --noEmit` with the samples' shims | module import |

Both extra axes earn their keep: the 3.10 sweep is the only thing that would
catch `05#7` (a nested same-quote f-string is a `SyntaxError` below 3.12 and the
samples are `ruff format`ed before they are checked), and the import smoke is
the only thing that catches a pinned `pattern` that type-checks and then throws
`SyntaxError` from `new RegExp`. Verified the artefacts exist:
`__pycache__/*.cpython-310.pyc` alongside `-311` and `-313`, and both smoke
JSONs list all 14 buildable probes.

Seeded with the shapes that broke: closed empty object; nullable
integer/number/boolean/array/enum/const/constrained-string/date-time;
`contentEncoding` + `minLength`/`pattern`; `uniqueItems` over materialized
elements; arrays and typed maps of `time`/`duration`; `enum` + `default`;
`^\d{3}\-\d{4}$`; portable regexes; a doc string ending in `"`; array and object
count bounds; required + nullable arrays; members named
`validate`/`deserializer`/`serializer`/`violation`; every asserted `format`; a
deprecated operation in a definitions-only package.

Negative controls run by hand: a type error injected into a generated `.ts` is
reported by `tsc` and attributed to its probe; the same in a generated `.go` is
reported by `go vet`.

A per-target load rejection is recorded as that target's failure rather than
skipped, because a schema Go refuses and Java accepts is itself a P1
disagreement.

### 0.3 The corpora, through the runtimes — `08#gap-1`, `09#gap-1`, `09#gap-2`
`tests/json_schema_corpus_runtime.rs` compiles each corpus into a generated
model with one member per rule and pushes every row through all four:

- **`pattern_conformance` — 140 pairs, all four runtimes, all green.** The
  shared-helpers agent's `expect_match` had already landed, so the runner asserts
  the declared verdict, not just pairwise agreement. `expect_gate_reject` rows
  are skipped (the gate's business). Members the gate now refuses are dropped
  one at a time and reported, so a tightening gate does not fail the corpus.
- **`format_conformance` (124), `format_email` (56), `format_hostname` (41),
  `format_uri` (72)** against their declared verdicts.
- **`format_materialize_clock` (32)** round-tripped. The corpus declares no
  canonical form, so agreement between the four re-emitted wires *is* the
  contract — which is exactly the check `format.md` claimed to have.

## Divergences the harness found

Each is pinned; the pin fails when it is fixed.

**New — measured here first:**

1. **`new:go-numeric-accepts-quoted-token`** — **P0.** Go accepts the JSON
   *string* `"7"` for both `type: integer` and `type: number`. `encoding/json`
   decodes a quoted token into `json.Number`, and
   `parseIntegerField`/`parseNumberField` (`definitions.go:214`) never check the
   token's type. Java, Python and TypeScript all reject. This holds for every
   numeric member of every schema in the repository. Pinned by `numeric-bounds`
   and `integer-semantics`. → **go-emitter**
2. **`new:java-union-typed-map-branch`** — **P0, Java does not compile.** A
   typed-map branch (`{type: object, additionalProperties: {type: string}}`)
   inside a typeless `oneOf` emits
   `context.readTreeAsValue(node, <Branch>.class)` where `T` is already bound to
   the union interface: `incompatible types: inference variable T has
   incompatible bounds`. Pinned by `union-token-selection`. → **java-emitter**
3. **`new:java-rejects-12-digit-fraction`** — a `date-time` with twelve
   fractional digits (`format_conformance/dt-frac-high-precision`, declared
   valid) is accepted by Go, Python and TypeScript and rejected by Java.
   → **java-emitter**
4. **`new:ts-negative-zero`** — TypeScript alone cannot re-emit `-0`
   (`JSON.stringify(-0) === "0"`). Same class as D8: the TS converter does not
   own the `JSON.parse`/`stringify` boundary. Recommend documenting, as with D8.
5. **`new:reserved-member-load-disagreement`** — a member named `validate` is
   **load-rejected for Go** (P15 collision with the generated `Validate`) and
   **accepted for Java, Python and TypeScript**, all three of which then compile
   fine. `03#5` is fixed for Go; the fix left the four disagreeing at load time,
   which is the `02#3` shape. Needs a call: reject for all four, or none.
6. **`corpus:leap-second-rows`** — four corpus rows expect `:60` to be **valid**
   (`format_conformance/time-second-60-leap`, `dt-leap-second`,
   `format_materialize_clock/dt-leap`, `t-leap`) while both corpora's own notes
   describe a ":60-rejecting grammar" and all four runtimes reject. The data is
   wrong, not the runtimes. → **shared-helpers** (owns the corpora JSON)

**Already-known findings still reproducing:**

7. **`13#2`** — Java's field planner reads the nullability `oneOf` wrapper, so
   `minLength` on an array element's non-null branch is dropped: `slots[0]="x"`
   is rejected by Go/TypeScript/Python and accepted by Java. Pinned by
   `recursive-collections`. → **java-emitter**
8. **`13#4`** — `4503599627370496.5` is rejected by Go and Java (which see the
   decimal text) and accepted by Python and TypeScript (which see the rounded
   double). Note Java is now on the *correct* side, unlike the report. Pinned by
   `integer-semantics`.
9. **`09#8`** — Python truncates nanoseconds to microseconds, so
   `2021-06-15T12:30:45.123456789Z` re-emits as `...123456Z` where the other
   three preserve all nine digits. First time this is measured across languages.
   P1 exception (b) conditions it on a recoverable opt-out that does not exist.

## Not divergent (verified, previously unverified)

Everything else the harness asks now holds in all four: the full nullability
matrix incl. constrained non-null branches; integer/boolean discriminated
dispatch and `1.0 == 1`; inclusive numeric boundaries; `multipleOf` at 1e23 and
1e300; `uniqueItems` mathematical equality on parse *and* serialize incl.
`-0.0`/`0.0`; the serialize-side 2^53 cap; off-set closed values behind a
nullability wrapper; closed/open/typed-map object behaviour and required
aggregation; the whole `contentEncoding` canonical-form line; the `contains`
matcher over `number` and `integer` elements; and 140 pattern pairs + 293 format
rows. Several of these (`01#4`, `05#2`, `06#1..3`, `07#2`, `11#1`, `13#1`,
`13#3`, `08#1`, `03#5`, D6) appear to have been closed by the concurrent waves
while this was being built — the cases now hold them closed.

## Cross-file requests

- **shared-helpers** — `specs/json-schema/corpora/format_conformance/corpus.json`
  and `format_materialize_clock/corpus.json`: the four `:60` rows above expect
  `valid`/inclusion but every runtime rejects them, and the notes agree with the
  runtimes. Either flip the rows to invalid (and move the two clock rows out of
  the valid list) or change the grammar. Once changed, delete the matching
  entries from `OPEN_FORMAT_ROWS` in `tests/json_schema_corpus_runtime.rs`.
- **java-emitter** — a generated Java model whose temporal members appear inside
  a collection hands the element to Jackson's defaults: with a bare
  `ObjectMapper` this throws `Java 8 date/time type java.time.LocalDate not
  supported by default`, and with `WRITE_DATES_AS_TIMESTAMPS` on it writes
  `[2024,2,29]` instead of `"2024-02-29"`. It works today only because Temporal's
  `DefaultDataConverter` happens to register jsr310. A scalar `date` member is
  fine — the generated serializer writes it. The conformance runner replicates
  the Temporal configuration (`Runner.java:41`); consider emitting the element
  serializer instead of relying on the host mapper.

## Sample schema requests

None. Conformance schemas live under `samples/conformance/schemas/`, which I
own, so no `samples/schemas/*.yaml` change is needed for Wave 0.

## Snapshot shifts

None from me. I do not touch committed sample output; the four
`tests/generate_<lang>.rs` failures currently in the tree are the emitter agents'
in-flight work.

## Notes for whoever runs this next

- `NEXGEN_CONFORMANCE_KEEP=1` keeps the scratch workspace and prints its path.
- `NEXGEN_CONFORMANCE_PYTHON` overrides the interpreter.
- The harness prepends the newest `~/.nvm/versions/node/*/bin` to `PATH` so
  vitest gets a Node with `Temporal`; on CI's Node 26 that is a no-op.
- Adding a conformance **case** needs no runner change. Adding a **mutation
  kind** needs all four runners.

---

# Addendum — pin reconciliation

Five of the nine divergences the harness reported have been closed by other
agents since the first pass. Every stale pin is deleted; the three binaries are
green again. `cargo fmt --check` clean.

## Corpus data: `corpus:leap-second-rows` — resolved, four entries deleted

shared-helpers settled this against my reading, correctly. Their evidence is
better than mine: the two corpora contradicted **each other** —
`format_conformance` said ":60 ACCEPTED syntactically" while
`format_materialize_clock` called the same check the ":60-rejecting grammar" and
carried a prose `(SKIP)` admitting its own rows "are rejected by every native
parser". One documented the pre-materialization grammar, the other the shipped
one. The narrowing is structural (`[0-5][0-9]`), and the `:60`-accepting variant
lives behind the `string` opt-out that `09#8` found unimplemented — so the rows
described behaviour available in no configuration. My finding was right about
*which side was wrong* but I attributed it to a single stale row set rather than
to two corpora disagreeing.

Their data change is now in the tree: `time-second-60-leap` and `dt-leap-second`
are `expect_valid: false`, and the clock corpus's `dt-leap`/`t-leap` carry a
machine-readable `"expect_valid": false` in place of the prose marker.

My side, `tests/json_schema_corpus_runtime.rs:390`: the clock loop no longer
hardcodes `Expectation::Agree`. It reads `expect_valid`, treating an **absent**
field as valid — which is how they structured it, and the right default for a
corpus that is by definition a list of valid wires:

```rust
expected: match row.get("expect_valid").and_then(Value::as_bool) {
    Some(false) => Expectation::Rejected,
    _ => Expectation::Agree,
},
```

No prose parsing, no matching on `":60"`. `Expectation::Rejected` already
requires `parse_rejected` in all four and compares `outcome_summary()` rather
than the wire, which is exactly right for a row with no wire to agree on. All
four `OpenRow` entries deleted.

## `new:java-rejects-12-digit-fraction` — resolved, entry deleted

`(\.[0-9]+)?` → `(\.[0-9]{1,9})?` is landed in all three clock patterns
(`format.rs:125`, `:128`), and `format_conformance/dt-frac-high-precision` is now
`expect_valid: false`. Measured through the harness: all four targets reject
`2021-01-15T12:30:45.123456789012Z`, so the row produces no finding and the
entry is gone.

Agreed with the reasoning, and it is worth recording why: 12 digits exceed every
target's real capacity, so a uniform 9-digit cap moves an **accept-set**
divergence to agreement. Teaching Java to parse-and-truncate instead would have
converted it into a **round-trip** divergence — the strictly worse trade, and
exactly the shape `09#8` already is.

## Pins that went stale on their own

- **`new:go-numeric-accepts-quoted-token` — fixed.** All four now reject a
  quoted numeric token for `type: integer` and `type: number`.
  `numeric-bounds`'s `expected_divergence` deleted outright;
  `integer-semantics`'s narrowed from two findings to `13#4` alone, with the
  `parse_failures[2]` matcher removed. The driver caught both halves for me:
  "every target now agrees, so its expected_divergence is stale" for the first,
  "matches nothing any more" for the second.
- **`new:java-union-typed-map-branch` — fixed.** `union-token-selection` builds
  and passes in all four.
- **`13#2` — fixed.** `recursive-collections` passes; Java now reports
  `slots[0]` alongside `links[0]`.

Those last two cases' `expected_divergence` blocks were already removed from
`samples/conformance/json-schema.json` when I re-ran, so someone deleted them
along with the fix — which is the intended workflow, and the manifest is
otherwise intact (13 cases, 85 probes, collapse declarations unchanged). Noting
it only because that file is in my ownership set: if the convention is that the
fixing agent clears the pin, that works, and the driver enforces it either way.

## Still open after reconciliation

| pin | where | owner |
|---|---|---|
| `new:ts-negative-zero` | `mathematical-number-equality` | ts-emitter (D8-class; may be undeliverable — `JSON.stringify(-0)` is `"0"`) |
| `13#4` | `integer-semantics` | Python and TypeScript accept `4503599627370496.5`; Go and Java reject |
| `09#8` | `format_materialize_clock/dt-frac-9{,-offset}` | Python truncates ns → µs |
| `new:reserved-member-load-disagreement` | probe matrix, `reserved_methods / go` | needs a call: Go load-rejects a member named `validate`, the other three accept and compile |

Current totals: 13 manifest cases / 85 probes × 4 targets, 16 probe-matrix
schemas × 4 toolchains, 140 pattern pairs + 293 format rows + 32 clock rows × 4
runtimes. Four open pins, each failing the build the day it is fixed.

---

# Addendum 2 — per-target load scoping, and closing the last unexplained item

Accepted the call on `new:reserved-member-load-disagreement`, and encoded the
rule rather than deleting the row, since you are right that it recurs: every
Stage-3 reserved-word fix produces this exact shape.

## `new:reserved-member-load-disagreement` — closed as by-design

My harness was applying "a schema one target refuses to load and another accepts
is itself a P1 divergence" as a blanket rule. That conflates two different
guarantees. P1 constrains the accepted/rejected **wire** value set; P15 scopes
identifier validity to the **emitted target**, and
`features/properties.md:132-144` says so directly. Go rejecting `validate`
because the loader agent correctly added Go's fixed method set to the member
scope — while Java, Python and TypeScript accept and compile — is the specified
behaviour, not a defect.

`tests/json_schema_probe_matrix.rs` now has a first-class category for it,
distinct from `Broken`:

```rust
struct ScopedLoad {
    target: Target,
    diagnostic: &'static str,   // must keep matching
    rationale: &'static str,    // why this target, and only this target, is right
}
```

A declared target is excluded from the build stages (there is nothing to
compile) and reported under **"load verdicts scoped to the emitted target (by
design, not defects)"**. Crucially the declaration stays live in both
directions: the diagnostic must still match, and if the target *starts* loading
the schema the probe fails with "its scoped_load declaration is stale — delete
it". An undeclared per-target load rejection is still a finding, so the next
Stage-3 fix classifies itself the moment someone writes down the rationale
instead of escalating.

The category doc comment states the boundary explicitly, so the next reader does
not have to rediscover it: *load verdicts may differ per target; the accepted
and rejected wire value set may not — and that second one is the conformance
driver's business, not this file's.* Same sentence added to the README.

## `open` vs `structural`

You asked for `new:ts-negative-zero` to be recorded as structurally unfixable
rather than left on an open list. I agree it is unfixable, and rather than say so
only in prose I gave both harnesses a status field, because the same question
will be asked of every D8-class finding:

- `expected_divergence.status` in the manifest — `open` | `structural`
- `OpenRow.status` in the corpus runner — same two

Both keep `matches` live, so the assertion is unchanged; the difference is
reporting. `structural` prints under "documented target limitations (not
defects; kept live so a fix is noticed)". That way a permanent limitation stops
reading as an unclosed to-do without becoming an unwatched exemption.

Classified:

- **`new:ts-negative-zero` → structural.** `JSON.stringify(-0) === "0"` and the
  generated converter receives an already-parsed value, so TypeScript cannot
  emit negative zero in any configuration.
- **`13#4` → structural.** Same boundary. Go and Java see the decimal text and
  reject `4503599627370496.5`; Python and TypeScript see the rounded double and
  accept. TypeScript cannot recover the literal, so the agreed accept set is
  bounded by it — narrowing Python alone would move the divergence, not remove
  it. Marking it structural is a statement about the *cross-language* accept
  set, not a claim that Python is unfixable.
- **`09#8` → open.** Python's ns → µs truncation is the one remaining item with
  a possible fix: P1 exception (b) is conditioned on a recoverable `string`
  opt-out, and Wave 10 found that opt-out unimplemented. Left as `open` with the
  blocker named, since it closes when that lands.

## Final state

Three harnesses green, `cargo fmt --check` clean, `cargo build --all-features`
clean.

| classification | count | items |
|---|---|---|
| by design | 1 | `reserved_methods / go` — P15 per-target load scoping |
| structural | 2 | `new:ts-negative-zero`, `13#4` — the `JSON.parse` boundary (D8) |
| open | 1 | `09#8` — Python ns → µs, blocked on the Wave 10 `string` opt-out |
| unexplained | **0** | |

Coverage: 13 manifest cases / 85 probes × 4 targets, 16 probe-matrix schemas ×
4 toolchains (Python additionally on the 3.10 floor and 3.11), 140 pattern pairs
+ 293 format rows + 32 clock rows × 4 runtimes.

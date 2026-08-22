# Shared execution context — fix-plan rollout

Read this before starting. Then read your assigned sections of
`gap-analysis/FIX-PLAN.md` and the per-spec report(s) it cites.

## Repo
`/Users/bergundy/temporal/nexus-api-gen`, branch `codex-json-schema-conformance`.
Rust generator producing Go/TypeScript/Python/Java models + Nexus bindings from
JSON Schema 2020-12. Architecture: `architecture.md`, `AGENTS.md`.
Design mandates: `specs/json-schema/PRINCIPLES.md` (P1..P16, per-language §n).

## Environment (mandatory)
TypeScript/vitest needs nvm's Node — the system Node 26 lacks `Temporal`
despite the same version string. Every shell that may run TS tests:

    export PATH="$HOME/.nvm/versions/node/v26.5.0/bin:$PATH"

Verify with `node -e 'console.log(typeof Temporal)'` → must print `object`.

## Baseline
`cargo build --all-features` clean; `cargo test --all-features` fully green
with the PATH above. **If a test fails when you start, it is yours to
understand — do not assume it was already broken.**

## FILE OWNERSHIP — do not edit outside your set
Agents run concurrently. Editing another agent's file will be lost or will
conflict. If a fix requires a file you do not own, do NOT edit it: record it
in your report under "Cross-file requests" and I will route it.

| Agent | Owns |
|---|---|
| go-emitter      | `src/generator/json_schema/go.rs`, `src/generator/go.rs`, `src/planning/reachability.rs`, `tests/generate_go.rs` |
| java-emitter    | `src/generator/json_schema/java.rs`, `src/generator/java.rs`, `tests/generate_java.rs` |
| ts-emitter      | `src/generator/json_schema/typescript.rs`, `src/generator/typescript.rs`, `tests/generate_typescript.rs` |
| python-emitter  | `src/generator/json_schema/python.rs`, `src/generator/python.rs`, `tests/generate_python.rs` |
| loader          | `src/parser/json_schema.rs` |
| shared-helpers  | `src/json_schema/*.rs` (pattern, format, content_encoding, scalar, mod) |
| conformance     | `tests/json_schema_conformance_manifest.rs`, `samples/conformance/**`, NEW files under `tests/` |
| specs           | `specs/**/*.md` (except the corpora `.json`/`.body` files) |

Nobody owns: `samples/schemas/*.yaml`, any generated output under
`samples/{go,python,typescript,java}/`, `src/spec.rs`, `src/planning/*` (except
reachability.rs). See "Sample schemas" below.

## Sample schemas and regeneration — DO NOT TOUCH
- Do **not** edit `samples/schemas/*.yaml`. Several fixes want new fields there;
  that would collide across agents. Instead, list the exact YAML you want added
  under "Sample schema requests" in your report.
- Do **not** run `cargo build-json-examples` or otherwise regenerate
  `samples/{go,python,typescript,java}/`. I run one consolidated regeneration
  pass at the end. Checked-in samples are golden snapshots asserted by
  `tests/generate_*.rs`, so those assertions may fail until that pass — that is
  expected. Note in your report which snapshot tests you expect to shift.
- Write NEW probe schemas under `/tmp/` for your own verification.

## Cross-language contracts (decided; implement exactly these)
These were open questions; they are now settled. Do not re-litigate.

- **D1** base64: tighten the pinned regex to the canonical form (constrain the
  final character class) so decode→encode is the identity. Do not add a
  re-canonicalization step.
- **D2** `contains`/`uniqueItems` over a nullable element (`oneOf:[T,null]`):
  loosen the loader to accept; a `null` element never matches a scalar matcher,
  and two `null`s are a duplicate for uniqueness.
- **D5** `default` on a sum-type (non-nullability) `oneOf`: **load reject**.
- **D6** a materializing temporal `format` inside `propertyNames` or a
  `contains` matcher: **load reject** (a materialized value cannot assert a key).
- **D7** regex: gate-reject nested quantifiers / exponential-backtracking shapes.
- **D10** `uniqueItems` and `const`/`enum` over materialized values
  (temporal / `contentEncoding`): compare the **canonical wire string**, in both
  directions, in all four languages. Never the native value, never a reference.
- **D11** empty `fqn` on a service or operation: **load reject**.
- **DEFERRED — do not touch**: D3 (Java closed-value constant naming stays
  member-derived for now), D8 (TS untyped-extras >2^53 is a spec fix, not code),
  D9 (Java `get<Field>OrDefault` is kept; only its P15 registration changes).

## Definition of done
1. `cargo build --all-features` clean.
2. `cargo fmt` on the files you changed (repo gates a global
   `cargo fmt --check`; it is NOT clippy-clean, so ignore clippy baseline noise).
3. Tests you added pass; tests you did not intend to change still pass
   (except the golden-snapshot shifts you noted).
4. Every fix verified against a real probe: generate from a schema and
   compile/run the output (`go vet`/`go build`, `tsc`, `python -m py_compile`
   or import, `javac`). Reading the emitted source is not verification for a
   claimed compile break.

## Reporting
Write `gap-analysis/exec/<your-agent-name>.md`:
- **Fixed** — one line per finding id (e.g. `13#1`), what changed, `path:line`,
  how verified.
- **Not fixed** — finding id, why (blocked / needs another owner's file /
  disagreed with the finding, with evidence).
- **Cross-file requests** — file you do not own + the exact change wanted.
- **Sample schema requests** — exact YAML to add to `samples/schemas/*.yaml`.
- **Snapshot shifts** — which `tests/generate_*.rs` golden assertions will move.

Final chat response: ≤30 lines, findings fixed / deferred / blocked, one line each.

## Judgment
Several reports flag findings where the SPEC is wrong rather than the code
(Wave 11 of the plan). If you conclude a finding is wrong or the spec is the
thing to change, do not "fix" the code — say so in "Not fixed" with evidence.
A wrong fix is worse than a skipped one.

# nexgen contributor guidance

## Validation

- Before pushing, run the full validation suite from the repository root:

  ```sh
  ./scripts/validate.sh
  ```

- Focused checks are appropriate while iterating, but do not push without the
  full script passing. It covers Rust formatting and tests plus the checked-in
  Python, TypeScript, and Go sample projects.

## Generated samples

- Never edit generated sample output directly. Change the authored input,
  generator, or shared model as appropriate, then regenerate the affected
  outputs with the generator.
- JSON Schema sample outputs: `cargo build-json-examples [--lang <language>]`.
- Advanced WIT sample outputs: `cargo build-examples [--lang <language>]
  [<example>]`.
- Review regenerated diffs to ensure they are the intended consequence of the
  source change.

## Architecture and ownership

- Before changing JSON Schema behavior or architecture, read
  [`specs/json-schema/PRINCIPLES.md`](specs/json-schema/PRINCIPLES.md). Before
  changing WIT authoring or WIT-facing generated behavior, read
  [`GUIDE.md`](GUIDE.md).
- `src/parser/wit.rs` is the only place that should understand WIT-specific
  syntax and semantics. Keep WIT details out of the shared model and later
  pipeline stages.
- Everything after parsing—including `spec`, planning, validation, and the
  shared generator orchestration—must be input-format agnostic. Parsers lower
  their input into the shared API model; later stages must not branch on whether
  it originated as WIT or JSON Schema.
- Keep JSON and JSON Schema concerns within the relevant JSON Schema parser,
  planning, generator, and `src/json_schema/` components. Do not introduce JSON
  knowledge into generic or unrelated modules.
- Put target-language policy and rendering decisions in that language's files
  or modules (for example `src/generator/python.rs` and
  `src/generator/json_schema/python.rs`). Shared code may represent language
  neutral concepts, but must not encode one target's conventions.

## Documentation and public changes

- Update the architecture documentation when a pipeline boundary, ownership
  rule, or processing flow changes. JSON Schema architecture lives under
  `specs/json-schema/`, especially `pipeline.md` and `PRINCIPLES.md`.
- Update `GUIDE.md` when WIT authoring or generated WIT-facing behavior changes.
- Update `CHANGELOG.md` only for public-facing changes, such as CLI flags,
  generated APIs, or externally visible behavior. Use the `Unreleased` section.

## Compatibility

This repository is experimental. Do not infer compatibility guarantees for the
CLI, generated output, or internal APIs; document intentional public-facing
changes as above.

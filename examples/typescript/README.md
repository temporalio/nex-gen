# TypeScript Examples

Shared Node/TypeScript example suite for generated outputs.

- Authored WIT inputs live in `examples/inputs/*.wit`
- Checked-in generated outputs live in `examples/typescript/<example-id>/`
- Generated support fragments are emitted as `support.ts` next to the
  generated `index.ts`
- `nex-gen-runtime.ts` and `nex-gen-payload-converter.cjs` provide
  shared example runtime serialization helpers for WIT-direct generated values
- Vitest files live in `examples/typescript/tests/`
- `build_outputs.mjs` is a thin wrapper around
  `cargo build-examples --lang typescript`

Top-level rebuild command:

```bash
cargo build-examples --lang typescript
```

Current workflow:

```bash
cd examples/typescript
npm install
npm run build-outputs
npm run test
npm run typecheck
```

To rebuild one example only:

```bash
cd examples/typescript
node build_outputs.mjs workflow-service
```

Set `NEX_GEN_BIN=/path/to/nex-gen` to make `build_outputs.mjs` use an already-built binary instead of the cargo alias.

## Runtime

`nex-gen-runtime.ts` is a small hand-written runtime used by the generated TypeScript examples. It provides shared support for marking generated resource/model values and serializing WIT-direct values with the `json/nexus` payload encoding used in the example tests.

`nex-gen-payload-converter.cjs` exports the same payload converter in a CommonJS module so the Temporal TypeScript SDK can load it through `payloadConverterPath` when running real workflow/Nexus tests.

These runtime files exist because that serialization behavior is not yet built into the TypeScript SDK. They should eventually be removed once the SDK can natively serialize Nexus API generator values and resources.

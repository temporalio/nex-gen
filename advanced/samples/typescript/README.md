# TypeScript WIT Samples

Advanced TypeScript sample suite for outputs generated from authored WIT inputs,
plus the snapshot-only native-api (Nexus service/client) output of the
JSON-Schema samples.

- Authored WIT inputs live in [`advanced/samples/inputs/*.wit`](../inputs)
- Checked-in generated WIT outputs live in
  `advanced/samples/typescript/wit/<example>/`
- Generated support fragments are emitted as `support.ts` next to the generated
  `index.ts`
- Snapshot-only JSON-Schema native-api output lives in
  `advanced/samples/typescript/json_schema/api/<example>/`
- Vitest files live in `advanced/samples/typescript/tests/` (WIT round-trip and
  real-workflow/Nexus tests); proto wire fixtures live in
  [`advanced/samples/wire/proto`](../wire/proto)
- `build_outputs.mjs` is a thin wrapper around
  `cargo build-examples --lang typescript`

Top-level rebuild command:

```bash
cargo build-examples --lang typescript
```

Current workflow:

```bash
cd advanced/samples/typescript
npm install
npm run build-outputs
npm run typecheck
npm run test
```

To rebuild one example only:

```bash
cd advanced/samples/typescript
node build_outputs.mjs workflow-service
```

Set `NEX_GEN_BIN=/path/to/nex-gen` to make `build_outputs.mjs` use an
already-built binary instead of the cargo alias.

The definitions-mode JSON-Schema samples (plain data models) live under
[`samples/typescript`](../../samples/typescript).

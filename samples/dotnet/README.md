# .NET JSON-Schema samples

Generated C# for the JSON-Schema inputs in [`../schemas/`](../schemas/), in
**definitions** mode — plain data models plus the `NexusRpc` service interface,
without the native-api operation/client scaffolding.

- `chat/`, `kb/` — generated models, one directory per schema.
- `tests/` — round-trip checks that serialize the shared wire fixtures in
  [`../wire/json_schema/`](../wire/json_schema/) through `System.Text.Json` and
  assert JSON-equality.

> [!WARNING]
> **.NET is not yet a supported JSON-Schema target.** The `dotnet` generate
> target is gated behind the `advanced` Cargo feature and is absent from the
> [root README](../../README.md)'s language list. Generation, compilation and
> round-tripping all work, but the **constraint validator that every other
> target emits is not implemented for .NET**. See Known gaps below.

## Known gaps

The models here are structurally faithful — required/optional members,
nullability, open vs. closed objects, typed maps and `$ref` cycles all match the
contract. What is missing is *assertion*: constraint keywords are parsed,
planned, and then dropped without enforcement or diagnostic.

| Feature | Go / Java / Python / TS | .NET |
|---|---|---|
| Aggregated `ValidationError` over `Violation[]` | ✅ shared `definitions` runtime | ❌ per-class `JsonException`, first failure only |
| `minimum` / `maximum` / `multipleOf` | ✅ | ❌ dropped |
| `minLength` / `maxLength` / `pattern` | ✅ | ❌ dropped |
| `minItems` / `maxItems` / `uniqueItems` | ✅ | ❌ dropped |
| `enum` closed value sets | ✅ | ❌ emitted as bare `string` / `long` |
| `oneOf` discriminated unions | ✅ | ❌ emitted as an empty class — branches lost |
| `format` temporal materialization | ✅ native types | ❌ left as `string` |
| `contentEncoding: base64` | ✅ native bytes | ❌ left as `string` |
| `maxProperties` | ✅ | ✅ |

Concretely, `order` in [`../schemas/kb/content/block.json`](../schemas/kb/content/block.json)
declares `minimum: 0`. Go emits a bounds check; .NET emits a bare `long`. So
`{"blockId":"b","order":-5}` is rejected by every other target and accepted here.

The fixtures in `../wire/json_schema/` contain only valid payloads, so these
suites round-trip green despite the gaps — they test serialization, never
rejection.

## Regenerating

`cargo build-json-examples` does not accept `--lang dotnet` yet
(`build_json_examples` in `src/lib.rs` rejects it). Invoke the generate target
directly from the repo root:

```bash
cargo run --features advanced -- dotnet samples/schemas/chat.nexusrpc.yaml \
  --output samples/dotnet/chat
cargo run --features advanced -- dotnet samples/schemas/kb \
  --output samples/dotnet/kb
```

## Building and testing

```bash
dotnet build NexusApiGen.DotNetExamples.csproj   # compile the models
dotnet test tests/                                # round-trip the fixtures
```

For the native-api (service/client) form of these same schemas and the
WIT-based examples, see [`../../advanced/samples/dotnet/`](../../advanced/samples/dotnet/).

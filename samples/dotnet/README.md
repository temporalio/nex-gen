# .NET JSON-Schema samples

Generated C# for the JSON-Schema inputs in [`../schemas/`](../schemas/), in
**definitions** mode — plain data models plus the `NexusRpc` service interface,
without the native-api operation/client scaffolding.

- `chat/`, `kb/`, `showcase/` — generated models, one directory per schema.
- `tests/` — round-trip checks that serialize the shared wire fixtures in
  [`../wire/json_schema/`](../wire/json_schema/) through `System.Text.Json` and
  assert JSON-equality.

> [!WARNING]
> **.NET is not yet a supported JSON-Schema target.** The `dotnet` generate
> target is gated behind the `advanced` Cargo feature and is absent from the
> [root README](../../README.md)'s language list, and the constraint validator is
> only **partially** implemented. See Known gaps below.

## Validation

Generated models validate in both wire directions:

- **Deserialize** — `IJsonOnDeserialized.OnDeserialized` calls `Validate()`, so an
  inbound payload cannot enter the process in a shape the contract forbids.
- **Serialize** — `Validate()` is public, so a value built in code is checked
  before it goes on the wire.

Failures aggregate into one `ValidationException` carrying every `Violation
{ Path, Reason }`, never a partial first-failure. The message format matches Go's
`ValidationError.Error()` verbatim, so the same payload reads the same on every
target. `ValidationException` derives from `JsonException`, so a handler already
catching `System.Text.Json` failures keeps working.

## Known gaps

The models are structurally faithful — required/optional members, nullability,
open vs. closed objects, typed maps and `$ref` cycles all match the contract. What
is still incomplete is *assertion*: the keywords below are parsed, planned, and
then dropped. Generation reports each one as a `warning: dotnet: ...` naming the
affected members, so nothing is dropped silently.

| Feature | Go / Java / Python / TS | .NET |
|---|---|---|
| Aggregated `ValidationError` over `Violation[]` | ✅ shared `definitions` runtime | ✅ `Definitions.cs` |
| `minimum` / `maximum` / `exclusiveMinimum` / `exclusiveMaximum` / `multipleOf` | ✅ | ✅ |
| `minLength` / `maxLength` / `pattern` | ✅ | ✅ |
| `minItems` / `maxItems` / `uniqueItems` | ✅ | ✅ |
| `contains` / `minContains` / `maxContains` | ✅ | ⚠️ `const` branch only |
| 2^53-1 spec integer cap | ✅ | ✅ |
| `minProperties` / `maxProperties` / `dependentRequired` | ✅ | ✅ |
| `propertyNames` | ✅ | ⚠️ map-shaped objects only |
| `enum` closed value sets | ✅ | ❌ emitted as bare `string` / `long` |
| `oneOf` discriminated unions | ✅ | ❌ emitted as an empty class — branches lost |
| `format` temporal materialization | ✅ native types | ❌ left as `string` |
| `contentEncoding: base64` | ✅ native bytes | ❌ left as `string` |

One known diagnostic divergence within the covered set:

- `pattern` reasons quote the target's own rewritten expression, and Go and Java
  already differ here (Go `%q`-quotes and keeps `$`; Java concatenates and uses
  `\z`). .NET follows Java, since it shares the `\z` rewrite.

### .NET-specific lowering

Two places where the obvious C# spelling would be wrong, both covered by
`tests/StringConstraintChecks.cs`:

- **`pattern` end anchors are rewritten `$` → `\z`.** .NET's `Regex` treats `$` as
  "end of input, *or* immediately before a final newline", so `^[A-Z]{2,4}$` would
  match `"ABCD\n"` — a value the contract forbids. Python (`\Z`) and Java (`\z`)
  need the same rewrite; Go and JS do not.
- **`minLength`/`maxLength` count code points, not `string.Length`.** C#'s
  `string.Length` counts UTF-16 code units, so 12 astral characters would score 24
  and be rejected against `maxLength: 12`. `JsonRuntime.CodePointCount` matches
  Go's `utf8.RuneCountInString` and Java's `codePointCount`.

The wire fixtures in `../wire/json_schema/` hold only valid payloads, so they
exercise serialization but never rejection; `tests/ConstraintValidationChecks.cs`
covers the rejection side.

## Regenerating

`cargo build-json-examples` does not accept `--lang dotnet` yet
(`build_json_examples` in `src/lib.rs` rejects it). Invoke the generate target
directly from the repo root:

```bash
cargo run --features advanced -- dotnet samples/schemas/chat.nexusrpc.yaml \
  --output samples/dotnet/chat
cargo run --features advanced -- dotnet samples/schemas/kb \
  --output samples/dotnet/kb
cargo run --features advanced -- dotnet samples/schemas/showcase.nexusrpc.yaml \
  --output samples/dotnet/showcase
```

## Building and testing

```bash
dotnet build NexusApiGen.DotNetExamples.csproj   # compile the models
dotnet test tests/                                # round-trip the fixtures
```

For the native-api (service/client) form of these same schemas and the
WIT-based examples, see [`../../advanced/samples/dotnet/`](../../advanced/samples/dotnet/).

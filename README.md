# nex-gen

> [!WARNING]
> This repository is experimental. Generated APIs, input formats, and CLI behavior
> may change without compatibility guarantees.

Rust CLI for generating language-specific Nexus operation bindings from WIT.

The WIT definition is the source of truth for the public API. Protobuf descriptor sets are optional and are only needed when the WIT opts into proto-backed models or when using `add-rpc` to scaffold WIT from an existing proto RPC.

## Contents

- [Current Status](#current-status)
- [Examples](#examples)
- [WIT-Direct Generation](#wit-direct-generation)
- [WIT Directives](#wit-directives)
- [Runtimes](#runtimes)
- [Proto Backing](#proto-backing)
- [Validation](#validation)

## Current Status

| Feature | What it covers | Python | TypeScript | Go | .NET |
| --- | --- | :-: | :-: | :-: | :-: |
| Core API generation | WIT types become native data types; WIT functions and resources become wrappers that invoke Nexus operations from Temporal workflows | ✅ | ✅ | ✅ | ✅ |
| Service definitions | Generated service and operation descriptors, used to register operation handlers and to mock operations in tests | ✅ | ✅ | ❌ | ✅ |
| Proto backing | Generated models convert to and from Temporal's protobuf messages at the Nexus boundary (`@nexus.proto` and related directives) | ✅ | ✅ | ✅ | ✅ |
| Ergonomics directives | `@nexus` directives that polish the generated API: transforming raw responses into workflow handles, flattening nested parameters, accepting typed workflow/signal functions, doc comments | ✅ | ✅ | ✅ | ✅ |
| `json/nexus` runtime | Serialization shim that lets non-proto (WIT-direct) values round-trip through a Temporal server using a wire format shared across languages | ✅ | ✅ | ❌ | ❌ |

Because Go and .NET have no `json/nexus` runtime, only the proto-backed Go and
.NET examples share a wire format with the Python and TypeScript bindings;
WIT-direct Go and .NET models serialize with each SDK's default converter.

## Examples

Each example starts with authored WIT under `examples/inputs/`. Checked-in
generated output lives under `examples/python/<example_name>/` and
`examples/typescript/<example-name>/`; Go output lives under
`examples/go/<example-name>/` and .NET output lives under
`examples/dotnet/<example-name>/`. Language-specific tests live under each
language's `tests/` directory where present. See [`examples/README.md`](examples/README.md)
for links to each example's WIT, generated code, and tests.

- [`user-service`](examples/inputs/user-service.wit): a small WIT-direct API showing the basic shape of an operation returning a resource and a resource method that calls another operation.
- [`type-showcase`](examples/inputs/type-showcase.wit): a WIT-direct API focused on type coverage, including records, enums, flags, variants, results, maps, tuples, resources, resource methods, and no-result operations.
- [`start-workflow`](examples/inputs/start-workflow.wit): a proto-backed Temporal workflow-start API that returns a generated resource handle with follow-up operations such as cancel, restart, and get-result.
- [`workflow-service`](examples/inputs/workflow-service.wit): a proto-backed `SignalWithStartWorkflowExecution` example showing flattened APIs, function arguments, sourced fields, support converters, and output transforms.
- [`type-roundtrip`](examples/inputs/type-roundtrip.wit): a proto-backed type roundtrip example for focused native/proto conversion coverage, including retry policies, activity options, durations, task queues, and priority.

Rebuild the checked-in example outputs:

```bash
cargo build-examples
```

Rebuild one language or one example only:

```bash
cargo build-examples --lang python
cargo build-examples --lang dotnet
cargo build-examples user-service
cargo build-examples --lang typescript user-service
```

Run the same validations as the CI pipeline:

```bash
./scripts/validate.sh
```

Write the prepared WIT workspace the loader actually parses:

```bash
cargo run -- debug-wit-dir \
  --input examples/inputs/user-service.wit \
  --output /tmp/user-service-wit
```

## WIT-Direct Generation

Start with a WIT file that describes the API surface directly:

```wit
package temporal:user-service@1.0.0;

world system {
  export user-service;
}

/// @nexus.endpoint "__user_service"
interface user-service {
  resource user {
    constructor(user-id: string, email: string);

    update-email: func(email: string) -> user-result;
  }

  record get-user-request {
    user-id: string,
  }

  type user-result = own<user>;

  record update-email-request {
    user-id: string,
    email: string,
  }

  get-user: func(request: get-user-request) -> user-result;
  update-email: func(request: update-email-request) -> user-result;
}
```

Generate Python:

```bash
cargo run -- generate \
  --lang python \
  --input examples/inputs/user-service.wit \
  --output /tmp/user_service
```

Generate TypeScript:

```bash
cargo run -- generate \
  --lang typescript \
  --input examples/inputs/user-service.wit \
  --output /tmp/user-service
```

Generate Go:

```bash
cargo run -- generate \
  --lang go \
  --input examples/inputs/user-service.wit \
  --output /tmp/userservice
```

Generate .NET:

```bash
cargo run -- generate \
  --lang dotnet \
  --input examples/inputs/user-service.wit \
  --output /tmp/user-service-dotnet
```

Add `--format` to run a formatter after generation:

- Python: `ruff format`
- TypeScript: `prettier --write`
- Go: `gofmt -w`
- .NET: no formatter is run

The `user-service` example is intentionally small and WIT-native. The `type-showcase` example demonstrates broader WIT type coverage: records, enums, flags, variants, results, maps, tuples, resources, resource methods, and an operation with no return value without proto annotations.

## WIT Directives

The WIT file defines the public surface. `@nexus` directives carry the parts WIT does not express directly:

- service endpoint names
- service wire names
- support file paths
- language-native service namespaces/packages
- language-native override types
- flattened API-only field types
- sourced field expressions
- function conversion metadata
- output transforms
- explicit resource method operation bindings
- experimental service, operation, and record warnings
- `@nexus.delay-load-temporalio-workflow` on Python services that must not import `temporalio.workflow` until an operation executes

Resource methods bind to operations only when the method and operation have the same generated operation name. When they intentionally differ, mark the method with `@nexus.operation`, for example `/// @nexus.operation "cancel-workflow"` on `cancel: func(...)` to bind it to `cancel-workflow: func(...)`.

Input WIT files can set generated service namespaces/packages with
`@nexus.namespace`, such as `dotnet="Temporalio.Workflows"` or
`go="go.temporal.io/sdk/workflow"`. For Go, the import path's final segment is
used as the package name and the full path is used to remove self-imports.

Input WIT files can add support code with `@nexus.support`. Python support fragments are copied into the generated private `_support` package, TypeScript support fragments are emitted as `support.ts` next to the generated `index.ts`, and .NET support fragments are copied under `Support/`.

Support code can also be supplied outside WIT with repeatable `--support-file`
arguments on `generate`. Explicit support files apply to the selected
`--lang`, are appended after WIT-declared support, and use the same generated
layout as `@nexus.support` fragments. .NET support files infer their support
namespace from the C# `namespace` declaration in the file:

```bash
cargo run -- generate \
  --lang python \
  --input examples/inputs/user-service.wit \
  --support-file /path/to/custom_support.py \
  --output /tmp/user_service
```

## Runtimes

The examples include small language runtimes that are not generated from WIT:

- Python: `examples/python/nex_gen_runtime.py`
- TypeScript: `examples/typescript/nex-gen-runtime.ts`

These runtimes provide shared serialization helpers for WIT-direct values, including the `json/nexus` payload encoding used by the example tests to round-trip generated records and resources through real Temporal Nexus clients. The TypeScript examples also include `nex-gen-payload-converter.cjs` so the Temporal TypeScript SDK can load the same payload converter through its `payloadConverterPath` data-converter hook.

These files are intentionally example/runtime shims. They should eventually be removed once the corresponding Temporal SDKs provide native serialization support for Nexus API generator values and resources.

## Proto Backing

Proto backing is opt-in per WIT type. Use it when an operation should accept or return generated API models while converting to or from protobuf messages at the Nexus boundary.

Proto-backed WIT uses:

- `@nexus.proto` on a WIT type to identify the protobuf message or enum it represents
- `@nexus.proto-field` when the WIT field name differs from the proto field name
- `--descriptors` on `generate` so the generator can validate fields and derive proto conversion code

Example:

```wit
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
/// @nexus.service-name "temporal.api.workflowservice.v1.WorkflowService"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{signal-function, task-queue, workflow-function};

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record signal-with-start-workflow-request {
    /// @nexus.proto-field "workflow_type"
    workflow: workflow-function,
    workflow-id: string,
    task-queue: task-queue,
    /// @nexus.proto-field "signal_name"
    signal: signal-function,
    /// @nexus.source "workflow_namespace"
    namespace: option<string>,
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse"
  record signal-with-start-workflow-response {
    run-id: option<string>,
  }

  /// @nexus.output-transform
  ///   python-type="workflow.ExternalWorkflowHandle[typing.Any]"
  ///   python="workflow.get_external_workflow_handle(request.workflow_id, run_id=result.run_id)"
  ///   typescript-type="workflow.ExternalWorkflowHandle"
  ///   typescript="workflow.getExternalWorkflowHandle(request.workflowId, result.runId ?? undefined)"
  /// @nexus.operation name="SignalWithStartWorkflowExecution"
  signal-with-start-workflow: func(
    request: signal-with-start-workflow-request,
  ) -> signal-with-start-workflow-response;
}
```

Generate a proto-backed example:

```bash
cargo run -- generate \
  --lang python \
  --input examples/inputs/workflow-service.wit \
  --input examples/inputs/deps \
  --descriptors examples/descriptors/temporal_api.bin \
  --output /tmp/workflow_service
```

`--descriptors` can be passed more than once when a proto-backed API depends on multiple descriptor files. Duplicate files or duplicate symbols are rejected.

The examples include a reusable Temporal semantic/common type WIT input:

- `nexus:temporal-types/model@1.0.0`

Pass it as an additional `--input` when generating an API that uses
`nexus:temporal-types/model@1.0.0`. For `generate`, the first `--input` is the
API generation root and later inputs are linked into the parser workspace. For
`add-rpc`, pass any WIT inputs needed to resolve shared types; when extending an
existing WIT file, put that file first. A linked input can be a single WIT file,
a WIT package directory, or a directory containing WIT package directories, so
`examples/inputs/deps` links every package under it.

Generate WIT for a proto RPC from a descriptor set:

```bash
cargo run -- add-rpc \
  --descriptors examples/descriptors/temporal_api.bin \
  --rpc SignalWithStartExecution \
  --input examples/inputs/deps
```

Write the standalone WIT scaffold to a file instead of stdout:

```bash
cargo run -- add-rpc \
  --descriptors examples/descriptors/temporal_api.bin \
  --rpc temporal.api.workflowservice.v1.WorkflowService.SignalWithStartWorkflowExecution \
  --input examples/inputs/deps \
  --output /tmp/add-rpc.wit
```

Extend an existing WIT file with a new RPC:

```bash
cargo run -- add-rpc \
  --descriptors examples/descriptors/temporal_api.bin \
  --rpc SignalWorkflowExecution \
  --input examples/inputs/workflow-service.wit \
  --input examples/inputs/deps
```

Rewrite the existing WIT file in place by pointing `--output` at the same path:

```bash
cargo run -- add-rpc \
  --descriptors examples/descriptors/temporal_api.bin \
  --rpc SignalWorkflowExecution \
  --input examples/inputs/workflow-service.wit \
  --input examples/inputs/deps \
  --output examples/inputs/workflow-service.wit
```

## Validation

Validate the Python examples:

```bash
cargo build-examples --lang python
cd examples/python
uv run pytest
uv run basedpyright
```

Validate the TypeScript examples:

```bash
cargo build-examples --lang typescript
cd examples/typescript
npm install
npm run test
npm run typecheck
```

`cargo test` validates the checked-in example outputs as-is. Use `cargo build-examples` when you want to refresh them.

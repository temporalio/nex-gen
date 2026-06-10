# .NET Generation Follow-Ups

This tracks review items for removing example-shaped or Temporal-specific assumptions from
`src/dotnet/mod.rs`. Tackle these one at a time.

- [x] Emit fully qualified .NET `@nexus.source` helper calls and keep `WorkflowNamespace` outside `TemporalModelConverters`.
- [x] Replace remaining hardcoded temporal converter references with the well-known `NexGen.Support.ProtoExtensions` support path.
- [x] Move known proto conversion selection out of the .NET generator. The generator now calls `ProtoConverters.ToProto<TProto>(...)` for proto-backed replacement values instead of switching on specific Temporal proto full names.
- [x] Keep generated operations as Temporal workflow APIs, but make them direct static functions that create the workflow Nexus client internally instead of extension methods on `NexusWorkflowClient<T>`.
- [x] Make function-field expression helpers metadata-driven. .NET `@nexus.function` fields now use `dotnet-name-extractor="..."` support helpers for method-name extraction, so Temporal attribute handling lives outside the generator.
- [ ] Remove generator-level empty-string-to-null semantics for optional string resource bindings, or make that behavior explicitly configurable per field/source.
- [x] Replace remaining Temporal-specific protobuf C# namespace mapping with `@nexus.proto dotnet-type="..."` plus a generic PascalCase fallback for unannotated proto names.

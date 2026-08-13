// TEMPORARY: the published nexus-rpc@0.0.2 predates
// https://github.com/nexus-rpc/sdk-typescript/pull/40, which added `TypeInfo` /
// `TransferTypeConverter` and the operation `inputType` / `outputType` metadata the
// generated code emits. `operation()` already spreads its options through
// untouched, so this is a typings-only gap: delete this file and bump the
// `nexus-rpc` pin once the release carrying #40 is published.
//
// The top-level `export {}` makes this file a module, so each `declare module`
// below *augments* the real package instead of shadowing it. `OperationOptions`
// and `OperationDefinition` are augmented at their declaring module paths rather
// than at the `nexus-rpc` barrel that re-exports them.
export {};

declare module "nexus-rpc" {
  export interface TransferTypeConverter<T, D = unknown> {
    fromTransferType(value: D): T;
    toTransferType(value: T): D;
  }

  export interface TypeInfo<T = unknown, D = T> {
    transferTypeConverter?: TransferTypeConverter<T, D>;
  }
}

declare module "nexus-rpc/lib/service/helpers" {
  interface OperationOptions<_I, _O> {
    inputType?: import("nexus-rpc").TypeInfo<_I, unknown>;
    outputType?: import("nexus-rpc").TypeInfo<_O, unknown>;
  }
}

declare module "nexus-rpc/lib/service/service-definition" {
  interface OperationDefinition<I, O> {
    inputType?: import("nexus-rpc").TypeInfo<I, unknown>;
    outputType?: import("nexus-rpc").TypeInfo<O, unknown>;
  }
}

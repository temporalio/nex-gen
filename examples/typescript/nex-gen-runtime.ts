import {
  BinaryPayloadConverter,
  CompositePayloadConverter,
  JsonPayloadConverter,
  UndefinedPayloadConverter,
  str,
  u8,
} from "@temporalio/common";
import type { Payload, PayloadConverterWithEncoding } from "@temporalio/common";

export const NEXUS_ENCODING = "json/nexus";
const NEXUS_TYPE_METADATA_KEY = "nexusType";
const NEXUS_REGISTRY = Symbol.for("nex-gen.registry");
const NEXUS_TYPE_ID = Symbol.for("nex-gen.type-id");
const NEXUS_VALUE = Symbol.for("nex-gen.value");

/// The wire shape of a single value. Encoding and decoding are type-directed:
/// generated code registers a schema per nex-gen type, and the converter walks
/// the value and schema together. This is the TypeScript analog of the Python
/// runtime walking dataclass type hints, and is required for correctness --
/// a structural transform cannot distinguish map keys (data, preserved
/// verbatim) from record field names (renamed to kebab-case on the wire).
export type WireSchema =
  | { kind: "scalar" }
  | { kind: "bytes" }
  | { kind: "list"; element: WireSchema }
  | { kind: "map"; value: WireSchema }
  | { kind: "tuple"; elements: WireSchema[] }
  | { kind: "variant"; cases: Record<string, WireSchema | null> }
  | { kind: "ref"; typeId: string };

export interface NexusFieldSchema {
  /// Wire key for the field (kebab-case).
  wire: string;
  schema: WireSchema;
}

export interface NexusTypeSchema {
  /// Field schemas keyed by the native (camelCase) field name.
  fields: Record<string, NexusFieldSchema>;
  /// Constructs the native value from decoded fields. Used by resources to
  /// rebuild class instances; plain records omit it and decode to objects.
  factory?: (fields: Record<string, unknown>) => unknown;
}

interface NexusWrappedValue {
  [NEXUS_TYPE_ID]: string;
  [NEXUS_VALUE]: unknown;
}

function registry(): Map<string, NexusTypeSchema> {
  const globalObject = globalThis as typeof globalThis & {
    [NEXUS_REGISTRY]?: Map<string, NexusTypeSchema>;
  };
  globalObject[NEXUS_REGISTRY] ??= new Map();
  return globalObject[NEXUS_REGISTRY];
}

export function registerNexusType(typeId: string, schema: NexusTypeSchema): void {
  registry().set(typeId, schema);
}

export function markNexusResource(constructor: Function, typeId: string): void {
  Object.defineProperty(constructor, NEXUS_TYPE_ID, { value: typeId });
}

export function nexusValue<T>(typeId: string, value: T): T {
  return {
    [NEXUS_TYPE_ID]: typeId,
    [NEXUS_VALUE]: value,
  } as unknown as T;
}

function isObject(value: unknown): value is Record<PropertyKey, unknown> {
  return typeof value === "object" && value !== null;
}

function wrappedValue(value: unknown): NexusWrappedValue | undefined {
  if (!isObject(value)) {
    return undefined;
  }
  if (typeof value[NEXUS_TYPE_ID] === "string" && NEXUS_VALUE in value) {
    return value as unknown as NexusWrappedValue;
  }
  const constructor = value.constructor as
    | (Function & { [NEXUS_TYPE_ID]?: string })
    | undefined;
  const typeId = constructor?.[NEXUS_TYPE_ID];
  if (typeId != null) {
    return nexusValue(typeId, value) as unknown as NexusWrappedValue;
  }
  return undefined;
}

function schemaForType(typeId: string): NexusTypeSchema {
  const schema = registry().get(typeId);
  if (schema == null) {
    throw new Error(
      `unknown nex-gen type '${typeId}'; import the generated module before encoding or decoding`,
    );
  }
  return schema;
}

function encodeRecord(typeId: string, value: unknown): Record<string, unknown> {
  const entry = schemaForType(typeId);
  if (!isObject(value)) {
    throw new Error(`expected an object for nex-gen type '${typeId}'`);
  }
  const wire: Record<string, unknown> = {};
  for (const [name, field] of Object.entries(entry.fields)) {
    const item = value[name];
    if (item == null) {
      continue;
    }
    wire[field.wire] = encodeValue(item, field.schema);
  }
  return wire;
}

function encodeValue(value: unknown, schema: WireSchema): unknown {
  if (value == null) {
    return value;
  }
  switch (schema.kind) {
    case "scalar":
      return value;
    case "bytes":
      return Buffer.from(value as Uint8Array).toString("base64");
    case "list":
      return (value as unknown[]).map((item) => encodeValue(item, schema.element));
    case "map":
      // Map keys are data, not field names: preserved verbatim.
      return Object.fromEntries(
        Object.entries(value as Record<string, unknown>).map(([key, item]) => [
          key,
          encodeValue(item, schema.value),
        ]),
      );
    case "tuple":
      return (value as unknown[]).map((item, index) => {
        const element = schema.elements[index];
        if (element == null) {
          throw new Error(
            `tuple value has more elements than its schema (${schema.elements.length})`,
          );
        }
        return encodeValue(item, element);
      });
    case "variant": {
      const tagged = value as { tag: string; value?: unknown };
      const caseSchema = schema.cases[tagged.tag];
      if (caseSchema === undefined) {
        throw new Error(`unknown variant case '${tagged.tag}'`);
      }
      return caseSchema === null || tagged.value === undefined
        ? { tag: tagged.tag }
        : { tag: tagged.tag, value: encodeValue(tagged.value, caseSchema) };
    }
    case "ref":
      return encodeRecord(schema.typeId, value);
  }
}

function decodeRecord(typeId: string, wire: unknown): unknown {
  const entry = schemaForType(typeId);
  if (!isObject(wire)) {
    throw new Error(`expected an object for nex-gen type '${typeId}'`);
  }
  const fields: Record<string, unknown> = {};
  for (const [name, field] of Object.entries(entry.fields)) {
    const item = wire[field.wire];
    if (item == null) {
      continue;
    }
    fields[name] = decodeValue(item, field.schema);
  }
  return entry.factory != null ? entry.factory(fields) : fields;
}

function decodeValue(value: unknown, schema: WireSchema): unknown {
  if (value == null) {
    return value;
  }
  switch (schema.kind) {
    case "scalar":
      return value;
    case "bytes":
      return new Uint8Array(Buffer.from(value as string, "base64"));
    case "list":
      return (value as unknown[]).map((item) => decodeValue(item, schema.element));
    case "map":
      return Object.fromEntries(
        Object.entries(value as Record<string, unknown>).map(([key, item]) => [
          key,
          decodeValue(item, schema.value),
        ]),
      );
    case "tuple":
      return (value as unknown[]).map((item, index) => {
        const element = schema.elements[index];
        if (element == null) {
          throw new Error(
            `tuple value has more elements than its schema (${schema.elements.length})`,
          );
        }
        return decodeValue(item, element);
      });
    case "variant": {
      const tagged = value as { tag: string; value?: unknown };
      const caseSchema = schema.cases[tagged.tag];
      if (caseSchema === undefined) {
        throw new Error(`unknown variant case '${tagged.tag}'`);
      }
      return caseSchema === null || tagged.value === undefined
        ? { tag: tagged.tag }
        : { tag: tagged.tag, value: decodeValue(tagged.value, caseSchema) };
    }
    case "ref":
      return decodeRecord(schema.typeId, value);
  }
}

export class NexusPayloadConverter implements PayloadConverterWithEncoding {
  public readonly encodingType = NEXUS_ENCODING;

  public toPayload(value: unknown): Payload | undefined {
    const wrapped = wrappedValue(value);
    if (wrapped == null) {
      return undefined;
    }
    const typeId = wrapped[NEXUS_TYPE_ID];
    return {
      metadata: {
        encoding: u8(NEXUS_ENCODING),
        [NEXUS_TYPE_METADATA_KEY]: u8(typeId),
      },
      data: u8(JSON.stringify(encodeRecord(typeId, wrapped[NEXUS_VALUE]))),
    };
  }

  public fromPayload<T>(payload: Payload): T {
    const typeMetadata = payload.metadata?.[NEXUS_TYPE_METADATA_KEY];
    if (typeMetadata == null) {
      throw new Error("json/nexus payload is missing nexusType metadata");
    }
    if (payload.data == null) {
      throw new Error("json/nexus payload is missing data");
    }
    const typeId = str(typeMetadata);
    return decodeRecord(typeId, JSON.parse(str(payload.data))) as T;
  }
}

export const nexusPayloadConverter = new NexusPayloadConverter();

export const payloadConverter = new CompositePayloadConverter(
  new UndefinedPayloadConverter(),
  new BinaryPayloadConverter(),
  nexusPayloadConverter,
  new JsonPayloadConverter(),
);

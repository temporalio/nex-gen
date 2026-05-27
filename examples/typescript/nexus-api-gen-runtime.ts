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
const NEXUS_REGISTRY = Symbol.for("nexus-api-gen.registry");
const NEXUS_TYPE_ID = Symbol.for("nexus-api-gen.type-id");
const NEXUS_VALUE = Symbol.for("nexus-api-gen.value");

type NexusFactory = (value: Record<string, unknown>) => unknown;

interface NexusWrappedValue {
  [NEXUS_TYPE_ID]: string;
  [NEXUS_VALUE]: unknown;
}

function registry(): Map<string, NexusFactory> {
  const globalObject = globalThis as typeof globalThis & {
    [NEXUS_REGISTRY]?: Map<string, NexusFactory>;
  };
  globalObject[NEXUS_REGISTRY] ??= new Map();
  return globalObject[NEXUS_REGISTRY];
}

export function registerNexusResource(typeId: string, factory: NexusFactory): void {
  registry().set(typeId, factory);
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

function toWireName(name: string): string {
  return name.replace(/[A-Z]/g, (match) => `-${match.toLowerCase()}`);
}

function fromWireName(name: string): string {
  return name.replace(/-([a-z])/g, (_match, character: string) =>
    character.toUpperCase(),
  );
}

function isTagged(value: object): value is {
  tag: string;
  value?: unknown;
} {
  const tagged = value as { tag?: unknown };
  return (
    typeof tagged.tag === "string" &&
    Object.keys(value).every((key) => key === "tag" || key === "value")
  );
}

function toWire(value: unknown): unknown {
  if (value == null || typeof value !== "object") {
    return value;
  }
  if (value instanceof Uint8Array) {
    return Buffer.from(value).toString("base64");
  }
  if (Array.isArray(value)) {
    return value.map(toWire);
  }
  if (isTagged(value)) {
    return "value" in value
      ? { tag: value.tag, value: toWire(value.value) }
      : { tag: value.tag };
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [toWireName(key), toWire(item)]),
  );
}

function fromWire(value: unknown): unknown {
  if (value == null || typeof value !== "object") {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map(fromWire);
  }
  if (isTagged(value)) {
    return "value" in value
      ? { tag: value.tag, value: fromWire(value.value) }
      : { tag: value.tag };
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [fromWireName(key), fromWire(item)]),
  );
}

export class NexusPayloadConverter implements PayloadConverterWithEncoding {
  public readonly encodingType = NEXUS_ENCODING;

  public toPayload(value: unknown): Payload | undefined {
    const wrapped = wrappedValue(value);
    if (wrapped == null) {
      return undefined;
    }
    return {
      metadata: {
        encoding: u8(NEXUS_ENCODING),
        [NEXUS_TYPE_METADATA_KEY]: u8(wrapped[NEXUS_TYPE_ID]),
      },
      data: u8(JSON.stringify(toWire(wrapped[NEXUS_VALUE]))),
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
    const value = fromWire(JSON.parse(str(payload.data)));
    const factory = registry().get(typeId);
    if (factory != null) {
      return factory(value as Record<string, unknown>) as T;
    }
    return value as T;
  }
}

export const nexusPayloadConverter = new NexusPayloadConverter();

export const payloadConverter = new CompositePayloadConverter(
  new UndefinedPayloadConverter(),
  new BinaryPayloadConverter(),
  nexusPayloadConverter,
  new JsonPayloadConverter(),
);

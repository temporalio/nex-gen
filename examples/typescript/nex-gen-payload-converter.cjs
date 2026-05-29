const {
  BinaryPayloadConverter,
  CompositePayloadConverter,
  JsonPayloadConverter,
  UndefinedPayloadConverter,
  str,
  u8,
} = require("@temporalio/common");

const NEXUS_ENCODING = "json/nexus";
const NEXUS_TYPE_METADATA_KEY = "nexusType";
const NEXUS_REGISTRY = Symbol.for("nex-gen.registry");
const NEXUS_TYPE_ID = Symbol.for("nex-gen.type-id");
const NEXUS_VALUE = Symbol.for("nex-gen.value");

function registry() {
  globalThis[NEXUS_REGISTRY] ??= new Map();
  return globalThis[NEXUS_REGISTRY];
}

function nexusValue(typeId, value) {
  return {
    [NEXUS_TYPE_ID]: typeId,
    [NEXUS_VALUE]: value,
  };
}

function isObject(value) {
  return typeof value === "object" && value !== null;
}

function wrappedValue(value) {
  if (!isObject(value)) {
    return undefined;
  }
  if (typeof value[NEXUS_TYPE_ID] === "string" && NEXUS_VALUE in value) {
    return value;
  }
  const typeId = value.constructor?.[NEXUS_TYPE_ID];
  if (typeId != null) {
    return nexusValue(typeId, value);
  }
  return undefined;
}

function toWireName(name) {
  return name.replace(/[A-Z]/g, (match) => `-${match.toLowerCase()}`);
}

function fromWireName(name) {
  return name.replace(/-([a-z])/g, (_match, character) => character.toUpperCase());
}

function isTagged(value) {
  return (
    typeof value.tag === "string" &&
    Object.keys(value).every((key) => key === "tag" || key === "value")
  );
}

function toWire(value) {
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

function fromWire(value) {
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

class NexusPayloadConverter {
  encodingType = NEXUS_ENCODING;

  toPayload(value) {
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

  fromPayload(payload) {
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
      return factory(value);
    }
    return value;
  }
}

const nexusPayloadConverter = new NexusPayloadConverter();

const payloadConverter = new CompositePayloadConverter(
  new UndefinedPayloadConverter(),
  new BinaryPayloadConverter(),
  nexusPayloadConverter,
  new JsonPayloadConverter(),
);

module.exports = {
  nexusPayloadConverter,
  payloadConverter,
};

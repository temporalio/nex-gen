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

// CommonJS mirror of nex-gen-runtime.ts for the Temporal TypeScript SDK's
// `payloadConverterPath` data-converter hook. Encoding and decoding are
// type-directed: generated code registers a schema per nex-gen type (shared
// through the global registry symbol), and the converter walks the value and
// schema together. Keep this file in sync with nex-gen-runtime.ts.

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

function schemaForType(typeId) {
  const schema = registry().get(typeId);
  if (schema == null) {
    throw new Error(
      `unknown nex-gen type '${typeId}'; import the generated module before encoding or decoding`,
    );
  }
  return schema;
}

function encodeRecord(typeId, value) {
  const entry = schemaForType(typeId);
  if (!isObject(value)) {
    throw new Error(`expected an object for nex-gen type '${typeId}'`);
  }
  const wire = {};
  for (const [name, field] of Object.entries(entry.fields)) {
    const item = value[name];
    if (item == null) {
      continue;
    }
    wire[field.wire] = encodeValue(item, field.schema);
  }
  return wire;
}

function encodeValue(value, schema) {
  if (value == null) {
    return value;
  }
  switch (schema.kind) {
    case "scalar":
      return value;
    case "bytes":
      return Buffer.from(value).toString("base64");
    case "list":
      return value.map((item) => encodeValue(item, schema.element));
    case "map":
      // Map keys are data, not field names: preserved verbatim.
      return Object.fromEntries(
        Object.entries(value).map(([key, item]) => [
          key,
          encodeValue(item, schema.value),
        ]),
      );
    case "tuple":
      return value.map((item, index) => {
        const element = schema.elements[index];
        if (element == null) {
          throw new Error(
            `tuple value has more elements than its schema (${schema.elements.length})`,
          );
        }
        return encodeValue(item, element);
      });
    case "variant": {
      const caseSchema = schema.cases[value.tag];
      if (caseSchema === undefined) {
        throw new Error(`unknown variant case '${value.tag}'`);
      }
      return caseSchema === null || value.value === undefined
        ? { tag: value.tag }
        : { tag: value.tag, value: encodeValue(value.value, caseSchema) };
    }
    case "ref":
      return encodeRecord(schema.typeId, value);
    default:
      throw new Error(`unknown wire schema kind '${schema.kind}'`);
  }
}

function decodeRecord(typeId, wire) {
  const entry = schemaForType(typeId);
  if (!isObject(wire)) {
    throw new Error(`expected an object for nex-gen type '${typeId}'`);
  }
  const fields = {};
  for (const [name, field] of Object.entries(entry.fields)) {
    const item = wire[field.wire];
    if (item == null) {
      continue;
    }
    fields[name] = decodeValue(item, field.schema);
  }
  return entry.factory != null ? entry.factory(fields) : fields;
}

function decodeValue(value, schema) {
  if (value == null) {
    return value;
  }
  switch (schema.kind) {
    case "scalar":
      return value;
    case "bytes":
      return new Uint8Array(Buffer.from(value, "base64"));
    case "list":
      return value.map((item) => decodeValue(item, schema.element));
    case "map":
      return Object.fromEntries(
        Object.entries(value).map(([key, item]) => [
          key,
          decodeValue(item, schema.value),
        ]),
      );
    case "tuple":
      return value.map((item, index) => {
        const element = schema.elements[index];
        if (element == null) {
          throw new Error(
            `tuple value has more elements than its schema (${schema.elements.length})`,
          );
        }
        return decodeValue(item, element);
      });
    case "variant": {
      const caseSchema = schema.cases[value.tag];
      if (caseSchema === undefined) {
        throw new Error(`unknown variant case '${value.tag}'`);
      }
      return caseSchema === null || value.value === undefined
        ? { tag: value.tag }
        : { tag: value.tag, value: decodeValue(value.value, caseSchema) };
    }
    case "ref":
      return decodeRecord(schema.typeId, value);
    default:
      throw new Error(`unknown wire schema kind '${schema.kind}'`);
  }
}

class NexusPayloadConverter {
  encodingType = NEXUS_ENCODING;

  toPayload(value) {
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

  fromPayload(payload) {
    const typeMetadata = payload.metadata?.[NEXUS_TYPE_METADATA_KEY];
    if (typeMetadata == null) {
      throw new Error("json/nexus payload is missing nexusType metadata");
    }
    if (payload.data == null) {
      throw new Error("json/nexus payload is missing data");
    }
    const typeId = str(typeMetadata);
    return decodeRecord(typeId, JSON.parse(str(payload.data)));
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

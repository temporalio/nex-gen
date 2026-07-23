import { readFileSync } from "node:fs";
import { defaultPayloadConverter } from "@temporalio/common";

/**
 * Minimal shape of a generated model's mapper: the type hint the generator
 * emits alongside each model. `fromIntermediate` validates/parses a plain JSON
 * value into the model; `toIntermediate` projects the model back to a plain
 * JSON value.
 */
export interface IntermediateMapper<T> {
  fromIntermediate(raw: unknown): T;
  toIntermediate(value: T): unknown;
}

const encoder = new TextEncoder();

/** Wrap raw fixture bytes as a json/plain Temporal payload. */
function jsonPayload(data: Uint8Array) {
  return { metadata: { encoding: encoder.encode("json/plain") }, data };
}

/**
 * Deserialize fixture bytes into a generated model *through the Temporal data
 * converter*: the default payload converter decodes the json/plain bytes into a
 * plain intermediate object, which the mapper's `fromIntermediate` turns into
 * the typed model. This proves the generated type hint (the mapper) drives
 * converter-based deserialization.
 */
export function decodeFixture<T>(mapper: IntermediateMapper<T>, bytes: Uint8Array): T {
  const intermediate = defaultPayloadConverter.fromPayload(jsonPayload(bytes));
  return mapper.fromIntermediate(intermediate);
}

/**
 * Serialize a model back through the Temporal data converter and return the
 * re-encoded JSON as a generic parsed value (for JSON-equality assertions).
 */
export function encodeModel<T>(mapper: IntermediateMapper<T>, value: T): unknown {
  const payload = defaultPayloadConverter.toPayload(mapper.toIntermediate(value));
  if (payload?.data == null) {
    throw new Error("payload converter produced no data");
  }
  return JSON.parse(Buffer.from(payload.data).toString("utf8"));
}

/**
 * Round-trip a fixture through the converter: decode the bytes into the model,
 * re-encode, and return both the model and the re-serialized JSON so callers can
 * assert JSON-equality against the fixture.
 */
export function roundTripFixture<T>(
  mapper: IntermediateMapper<T>,
  bytes: Uint8Array,
): { value: T; serialized: unknown } {
  const value = decodeFixture(mapper, bytes);
  return { value, serialized: encodeModel(mapper, value) };
}

/** Read a canonical wire fixture as raw bytes. */
export function fixtureBytes(dir: URL, name: string): Uint8Array {
  return readFileSync(new URL(name, dir));
}

/** Read a canonical wire fixture as a parsed JSON value. */
export function loadFixture<T = unknown>(dir: URL, name: string): T {
  return JSON.parse(readFileSync(new URL(name, dir), "utf8")) as T;
}

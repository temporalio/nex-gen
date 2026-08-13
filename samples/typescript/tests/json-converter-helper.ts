import { readFileSync } from "node:fs";
import { defaultPayloadConverter } from "@temporalio/common";
import type { TransferTypeConverter } from "nexus-rpc";

const encoder = new TextEncoder();

/** Wrap raw fixture bytes as a json/plain Temporal payload. */
function jsonPayload(data: Uint8Array) {
  return { metadata: { encoding: encoder.encode("json/plain") }, data };
}

/**
 * Deserialize fixture bytes into a generated model *through the Temporal data
 * converter*: the default payload converter decodes the json/plain bytes into a
 * plain transfer value, which the generated `TransferTypeConverter`'s
 * `fromTransferType` turns into the typed model. This proves the converter the
 * generator attaches to each operation drives converter-based deserialization.
 */
export function decodeFixture<T>(
  converter: TransferTypeConverter<T>,
  bytes: Uint8Array,
): T {
  const transfer = defaultPayloadConverter.fromPayload(jsonPayload(bytes));
  return converter.fromTransferType(transfer);
}

/**
 * Serialize a model back through the Temporal data converter and return the
 * re-encoded JSON as a generic parsed value (for JSON-equality assertions).
 */
export function encodeModel<T>(converter: TransferTypeConverter<T>, value: T): unknown {
  const payload = defaultPayloadConverter.toPayload(converter.toTransferType(value));
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
  converter: TransferTypeConverter<T>,
  bytes: Uint8Array,
): { value: T; serialized: unknown } {
  const value = decodeFixture(converter, bytes);
  return { value, serialized: encodeModel(converter, value) };
}

/** Read a canonical wire fixture as raw bytes. */
export function fixtureBytes(dir: URL, name: string): Uint8Array {
  return readFileSync(new URL(name, dir));
}

/** Read a canonical wire fixture as a parsed JSON value. */
export function loadFixture<T = unknown>(dir: URL, name: string): T {
  return JSON.parse(readFileSync(new URL(name, dir), "utf8")) as T;
}

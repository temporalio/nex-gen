import { describe, expect, test } from "vitest";

import { TemporalMapper as StringTemporalMapper } from "../json_schema/definitions/temporal/models.ts";
import { TemporalMapper as DateTemporalMapper } from "../json_schema/definitions/temporal-date/models.ts";
import { TemporalMapper as TemporalTemporalMapper } from "../json_schema/definitions/temporal-temporal/models.ts";
import {
  decodeFixture,
  encodeModel,
  fixtureBytes,
  loadFixture as loadFixtureFrom,
  roundTripFixture,
} from "./json-converter-helper.ts";

const wireFixtureDir = new URL("../../wire/json_schema/temporal/", import.meta.url);

function bytes(name: string): Uint8Array {
  return fixtureBytes(wireFixtureDir, name);
}
function load(name: string): unknown {
  return loadFixtureFrom(wireFixtureDir, name);
}

// --- --js-temporal-repr=string (default): all four temporals are string. ---
describe("json-schema temporal (--js-temporal-repr=string, default)", () => {
  test("materialized temporals round-trip losslessly as canonical strings", () => {
    const { value, serialized } = roundTripFixture(
      new StringTemporalMapper(),
      bytes("temporal-full.json"),
    );
    expect(serialized).toEqual(load("temporal-full.json"));
    expect(value.createdAt).toBe("2021-06-15T12:30:45.123456+02:00");
    expect(value.timeout).toBe("PT1H30M");

    const minimal = roundTripFixture(
      new StringTemporalMapper(),
      bytes("temporal-minimal.json"),
    );
    expect(minimal.serialized).toEqual(load("temporal-minimal.json"));
  });

  test("non-canonical input is canonicalized (uppercase T/Z, +00:00 -> Z, PT90M -> PT1H30M)", () => {
    const value = decodeFixture(
      new StringTemporalMapper(),
      bytes("temporal-canonicalize.json"),
    );
    expect(encodeModel(new StringTemporalMapper(), value)).toEqual({
      createdAt: "2021-06-15T12:30:45Z",
      birthday: "2021-02-28",
      alarm: "12:30:45Z",
      timeout: "PT1H30M",
    });
  });

  test("materialized narrowing rejects :60, calendar duration, bad date, missing offset", () => {
    const mapper = new StringTemporalMapper();
    for (const bad of [
      { createdAt: "2021-12-31T23:59:60Z", timeout: "PT0S" },
      { createdAt: "2021-06-15T12:30:45Z", timeout: "P1Y" },
      { createdAt: "2021-06-15T12:30:45Z", birthday: "2021-02-29", timeout: "PT0S" },
      { createdAt: "2021-06-15T12:30:45", timeout: "PT0S" },
    ]) {
      const body = { birthday: "2000-01-01", alarm: "09:00:00", ...bad };
      expect(() => mapper.fromIntermediate(body)).toThrow();
    }
  });
});

// --- --js-temporal-repr=date: date-time -> Date (UTC ms fold); others string. ---
describe("json-schema temporal (--js-temporal-repr=date)", () => {
  test("date-time materializes to a Date and folds to a UTC instant on re-serialize", () => {
    const value = decodeFixture(new DateTemporalMapper(), bytes("temporal-full.json"));
    expect(value.createdAt).toBeInstanceOf(Date);
    expect((value.createdAt as Date).toISOString()).toBe("2021-06-15T10:30:45.123Z");
    expect(value.birthday).toBe("2021-06-15"); // stays string
    const serialized = encodeModel(new DateTemporalMapper(), value) as Record<
      string,
      unknown
    >;
    expect(serialized.createdAt).toBe("2021-06-15T10:30:45.123Z");
    expect(serialized.timeout).toBe("PT1H30M");
  });
});

// --- --js-temporal-repr=temporal: Temporal.* types; time stays string. ---
describe("json-schema temporal (--js-temporal-repr=temporal)", () => {
  test("temporals materialize to Temporal types and round-trip losslessly", () => {
    const { value, serialized } = roundTripFixture(
      new TemporalTemporalMapper(),
      bytes("temporal-full.json"),
    );
    expect(serialized).toEqual(load("temporal-full.json"));
    // ZonedDateTime preserves the offset; PlainDate / Duration are exact; time stays string.
    expect(String(value.createdAt.offset)).toBe("+02:00");
    expect(value.birthday.toString()).toBe("2021-06-15");
    expect(typeof value.alarm).toBe("string");
    // The Temporal.Duration holds 90 minutes; the wire canonical form (asserted
    // via `serialized` above) is PT1H30M.
    expect(value.timeout.total({ unit: "seconds" })).toBe(5400);
  });

  test("non-canonical input canonicalizes through Temporal types", () => {
    const value = decodeFixture(
      new TemporalTemporalMapper(),
      bytes("temporal-canonicalize.json"),
    );
    expect(encodeModel(new TemporalTemporalMapper(), value)).toEqual({
      createdAt: "2021-06-15T12:30:45Z",
      birthday: "2021-02-28",
      alarm: "12:30:45Z",
      timeout: "PT1H30M",
    });
  });
});

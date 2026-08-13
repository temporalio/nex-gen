import { describe, expect, test } from "vitest";

import {
  AddressMapper,
  AttributesMapper,
  ContactTsMapper,
  DEFAULT_DEBUG,
  DEFAULT_GREETING,
  DEFAULT_RETRIES,
  ExtrasMapper,
  LabelsMapper,
  NicknamesMapper,
  QuotasMapper,
  SettingsMapper,
  ShowcaseMapper,
  TokensMapper,
  ValidationError,
  WidgetMapper,
  type LinkNote,
  type Showcase,
  type ShowcaseDetailObject,
  type TextNote,
  type Widget,
} from "../showcase/index.ts";
import {
  fixtureBytes,
  loadFixture as loadFixtureFrom,
  roundTripFixture,
  type IntermediateMapper,
} from "./json-converter-helper.ts";

const wireFixtureDir = new URL("../../wire/json_schema/showcase/", import.meta.url);

function loadFixture(name: string): unknown {
  return loadFixtureFrom(wireFixtureDir, name);
}

// TS mappers preserve explicit nulls, so all showcase fixtures round-trip with
// exact JSON-equality (no optional+nullable collapse — unlike Go/Java).
function expectRoundTrip<T>(name: string, mapper: IntermediateMapper<T>): T {
  const { value, serialized } = roundTripFixture(
    mapper,
    fixtureBytes(wireFixtureDir, name),
  );
  expect(serialized).toEqual(loadFixture(name));
  return value;
}

describe("json-schema showcase generated definitions", () => {
  test("roundtrips canonical wire fixtures through the Temporal converter", () => {
    const minimal = expectRoundTrip("showcase-minimal.json", new ShowcaseMapper());
    expect(minimal).toMatchObject<Partial<Showcase>>({
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "Widget",
      count: 3,
      active: true,
      category: "tools",
    });
    expect(minimal.retries ?? DEFAULT_RETRIES).toBe(3);
    // Scalar defaults of each kind: unset on the wire (undefined), applied by the
    // consumer via the emitted DEFAULT_<FIELD> constant + `??` idiom.
    expect(minimal.greeting).toBeUndefined();
    expect(minimal.greeting ?? DEFAULT_GREETING).toBe("hello");
    expect(minimal.debug).toBeUndefined();
    expect(minimal.debug ?? DEFAULT_DEBUG).toBe(false);
    // Serialize side: an unset default-bearing field is OMITTED on the wire.
    const minimalWire = loadFixture("showcase-minimal.json") as Record<string, unknown>;
    expect(minimalWire).not.toHaveProperty("greeting");
    expect(minimalWire).not.toHaveProperty("debug");
    expect(minimalWire).not.toHaveProperty("retries");

    const full = expectRoundTrip("showcase-full.json", new ShowcaseMapper());
    expect(full.retries).toBe(5);
    expect(full.middleName).toBe("Q");
    expect(full.tags).toEqual(["a", "b"]);
    expect(full.aliases).toEqual(["alpha", "beta"]);
    expect(full.roles).toEqual(["admin", "user"]);
    expect(full.address?.street).toBe("1 Main St");
    expect(full.address?.additionalProperties).toEqual({ region: "west" });
    expect(full.labels?.additionalProperties).toEqual({
      env: "prod",
      team: "core",
    });
    expect(full.settings?.fontSize).toBe(14);

    const nulls = expectRoundTrip("showcase-nulls.json", new ShowcaseMapper());
    expect(nulls.middleName).toBeNull();
    expect(nulls.category).toBeNull();
    expect(nulls.active).toBe(false);

    const address = expectRoundTrip("address-open.json", new AddressMapper());
    expect(address.street).toBe("1 Main St");
    expect(address.additionalProperties).toEqual({ "x-extra": 7 });

    const labels = expectRoundTrip("labels.json", new LabelsMapper());
    expect(labels.additionalProperties).toEqual({ env: "prod", team: "core" });

    const settings = expectRoundTrip("settings.json", new SettingsMapper());
    expect(settings.theme).toBe("dark");
    expect(settings.fontSize).toBe(14);

    const metrics = expectRoundTrip("showcase-metrics.json", new ShowcaseMapper());
    expect(metrics.priority).toBe(5);
    expect(metrics.level).toBe(2);
    expect(metrics.ratio).toBe(15);
    expect(metrics.step).toBe(9);

    // The astral crux: "a😀b" is 3 code points but 6 UTF-8 bytes / 4 UTF-16
    // units; it must round-trip through code (maxLength:5) unchanged.
    const strings = expectRoundTrip("showcase-strings.json", new ShowcaseMapper());
    expect(strings.code).toBe("a😀b");
    expect(strings.nickname).toBe("buddy");
  });

  test("roundtrips the allOf-merged Widget type and enforces its merged bounds", () => {
    // Widget is an allOf base-type extension (WidgetBase folded in + an extension
    // branch): a flat standalone object with the union of properties and required.
    const widget = expectRoundTrip("widget.json", new WidgetMapper());
    expect(widget).toMatchObject<Partial<Widget>>({
      id: "w-1",
      kind: "gadget",
      name: "Widget One",
      size: 15,
    });

    // `size` carries a bound tightened from two allOf branches to [10, 20].
    expect(() =>
      new WidgetMapper().fromIntermediate({ id: "w-1", name: "Widget One", size: 5 }),
    ).toThrow(/must be >= 10, got 5/);
    expect(() =>
      new WidgetMapper().fromIntermediate({ id: "w-1", name: "Widget One", size: 25 }),
    ).toThrow(/must be <= 20, got 25/);

    // A missing required member contributed by the extension branch is rejected.
    expect(() => new WidgetMapper().fromIntermediate({ id: "w-1" })).toThrow(
      ValidationError,
    );
  });

  test("reports JSON schema validation errors", () => {
    // Wrong const value.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        kind: "nope",
        name: "w",
        count: 1,
        active: true,
        category: null,
      }),
    ).toThrow(ValidationError);

    // Missing required (required+nullable) field.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        kind: "showcase",
        name: "w",
        count: 1,
        active: true,
      }),
    ).toThrow(ValidationError);

    // Unknown key on a closed object.
    expect(() =>
      new SettingsMapper().fromIntermediate({ theme: "dark", nope: 1 }),
    ).toThrow(ValidationError);

    // Wrong integer const value.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        kind: "showcase",
        revision: 2,
        enabled: true,
        status: "active",
        tier: 1,
        scale: 1.5,
        name: "w",
        count: 1,
        active: true,
        category: "tools",
      }),
    ).toThrow(/must equal 1/);

    // Numeric bounds fire at runtime with informative reasons.
    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    };

    // Closed value-set (enum/const) rejections with informative reasons.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, status: "archived" }),
    ).toThrow(/must be one of \["active", "inactive", "pending"\], got "archived"/);
    expect(() => new ShowcaseMapper().fromIntermediate({ ...base, tier: 9 })).toThrow(
      /must be one of \[1, 2, 3\], got 9/,
    );
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, scale: 3.5 }),
    ).toThrow(/must be one of \[1.5, 2.5\], got 3.5/);
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, enabled: false }),
    ).toThrow(/must equal true/);
    // Valid enum/const values are accepted.
    expect(
      new ShowcaseMapper().fromIntermediate({
        ...base,
        status: "pending",
        tier: 3,
        scale: 2.5,
      }).status,
    ).toBe("pending");
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, priority: 99 }),
    ).toThrow(/must be <= 10, got 99/);
    expect(() => new ShowcaseMapper().fromIntermediate({ ...base, level: 0 })).toThrow(
      /must be > 0, got 0/,
    );
    expect(() => new ShowcaseMapper().fromIntermediate({ ...base, step: 7 })).toThrow(
      /must be a multiple of 3, got 7/,
    );
    expect(() => new ShowcaseMapper().fromIntermediate({ ...base, ratio: 7 })).toThrow(
      /must be a multiple of 5, got 7/,
    );

    // String-length bounds fire at runtime, counted in code points.
    expect(() => new ShowcaseMapper().fromIntermediate({ ...base, code: "a" })).toThrow(
      /must have length >= 2, got 1/,
    );
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, code: "abcdef" }),
    ).toThrow(/must have length <= 5, got 6/);
    // Astral: 6 emoji = 6 code points (24 bytes); rejected by code-point count.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, code: "😀😀😀😀😀😀" }),
    ).toThrow(/must have length <= 5, got 6/);
    // A multi-byte value within the code-point bound is accepted (byte count 6
    // would exceed maxLength:5 — proving code points, not bytes).
    expect(new ShowcaseMapper().fromIntermediate({ ...base, code: "a😀b" }).code).toBe(
      "a😀b",
    );

    // Array constraints fire at runtime with informative reasons.
    // Too few / too many items (minItems:1 / maxItems:5).
    expect(() => new ShowcaseMapper().fromIntermediate({ ...base, tags: [] })).toThrow(
      /must have at least 1 items, got 0/,
    );
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        tags: ["a", "b", "c", "d", "e", "f"],
      }),
    ).toThrow(/must have at most 5 items, got 6/);
    // Duplicate element (uniqueItems).
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, aliases: ["x", "x"] }),
    ).toThrow(/duplicate items: element at index 1 equals index 0/);
    // Missing required contains match (no "admin").
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, roles: ["user"] }),
    ).toThrow(/too few matching items: at least 1, got 0/);
    // Too many contains matches (maxContains:2).
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        roles: ["admin", "admin", "admin"],
      }),
    ).toThrow(/too many matching items: at most 2, got 3/);
    // Valid arrays are accepted.
    const ok = new ShowcaseMapper().fromIntermediate({
      ...base,
      tags: ["a"],
      aliases: ["x", "y"],
      roles: ["admin"],
    });
    expect(ok.roles).toEqual(["admin"]);
  });

  test("enforces pattern constraints with RE2-safe portable semantics", () => {
    // sku `^[A-Z]{2,4}$` and phrase `^\S+\s\S+$` round-trip.
    const patterns = expectRoundTrip("showcase-patterns.json", new ShowcaseMapper());
    expect(patterns.sku).toBe("AB");
    expect(patterns.phrase).toBe("hello world");

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    };

    // Lowercase / too-long sku.
    expect(() => new ShowcaseMapper().fromIntermediate({ ...base, sku: "ab" })).toThrow(
      /must match pattern/,
    );
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, sku: "ABCDE" }),
    ).toThrow(/must match pattern/);

    // phrase with no whitespace separator.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, phrase: "helloworld" }),
    ).toThrow(/must match pattern/);

    // `\s` ASCII-class crux: a NBSP (U+00A0) is NOT ASCII whitespace. The loader
    // rewrote `\s`/`\S` to an explicit ASCII class spliced into the emitted
    // RegExp, so JS's otherwise-Unicode `\s` rejects it — consistent with
    // Go/Python/Java.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, phrase: "hello world" }),
    ).toThrow(/must match pattern/);

    // `$` end-anchor crux: a trailing newline is rejected. JS `$` is already
    // end-of-input (no `\n` exception), matching the `\Z`/`\z` rewrite applied
    // for Python/Java.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, phrase: "hello world\n" }),
    ).toThrow(/must match pattern/);

    // A valid ASCII-space phrase and sku are accepted.
    const okPattern = new ShowcaseMapper().fromIntermediate({
      ...base,
      sku: "XY",
      phrase: "hello world",
    });
    expect(okPattern.sku).toBe("XY");
    expect(okPattern.phrase).toBe("hello world");
  });

  test("enforces asserted string formats with pinned, portable checks", () => {
    // uuid/email/hostname/uri/ipv4 round-trip (string-typed, no materialization).
    const formats = expectRoundTrip("showcase-format.json", new ShowcaseMapper());
    expect(formats.requestId).toBe("de305d54-75b4-431b-adb2-eb6b9e546013");
    expect(formats.contactEmail).toBe("user@example.com");
    expect(formats.host).toBe("api.example.com");
    expect(formats.homepage).toBe("https://example.com/path?q=1#frag");
    expect(formats.gateway).toBe("192.168.0.1");

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    };

    // A malformed uuid.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, requestId: "not-a-uuid" }),
    ).toThrow(/must be a valid uuid, got "not-a-uuid"/);

    // Single-label email domain (user@localhost) is rejected.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        contactEmail: "user@localhost",
      }),
    ).toThrow(/must be a valid email, got "user@localhost"/);

    // ipv4 octet out of range.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, gateway: "256.0.0.1" }),
    ).toThrow(/must be a valid ipv4, got "256.0.0.1"/);

    // uri with a double-`::` IPv6 IP-literal host (spliced ipv6 grammar rejects).
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, homepage: "http://[1::2::3]" }),
    ).toThrow(/must be a valid uri/);

    // An over-long hostname (> 253 code points) is rejected by the length guard.
    const longHost = Array.from({ length: 64 }, () => "abc").join(".");
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, host: longHost }),
    ).toThrow(/must be a valid hostname/);
  });

  test("enforces object member-count, propertyNames, and dependentRequired", () => {
    // Valid map and object round-trip.
    const attributes = expectRoundTrip("attributes.json", new AttributesMapper());
    expect(attributes.additionalProperties).toEqual({ host: "a", port: "8080" });
    const contact = expectRoundTrip("contact.json", new ContactTsMapper());
    expect(contact.shippingStreet).toBe("1 Main St");
    expect(contact.shippingZip).toBe("90210");

    // minProperties:1 on a map — an empty object is too few.
    expect(() => new AttributesMapper().fromIntermediate({})).toThrow(
      /must have at least 1 properties, got 0/,
    );
    // maxProperties:3 on a map.
    expect(() =>
      new AttributesMapper().fromIntermediate({ a: "1", b: "2", c: "3", d: "4" }),
    ).toThrow(/must have at most 3 properties, got 4/);
    // propertyNames maxLength:8 — an over-long key.
    expect(() => new AttributesMapper().fromIntermediate({ toolongkey: "1" })).toThrow(
      /invalid property name "toolongkey": must have length <= 8, got 10/,
    );

    // dependentRequired — a shipping street present without a shipping zip.
    expect(() =>
      new ContactTsMapper().fromIntermediate({ shippingStreet: "1 Main St" }),
    ).toThrow(/property "shippingZip" is required when "shippingStreet" is present/);
    // minProperties:1 on a declared-property object — an empty object.
    expect(() => new ContactTsMapper().fromIntermediate({})).toThrow(
      /must have at least 1 properties, got 0/,
    );
  });

  test("round-trips oneOf sum types and rejects unmatchable/unknown members", () => {
    // Disjoint-kind union (string | number): each branch round-trips and
    // narrows natively.
    const asString = expectRoundTrip(
      "showcase-union-string.json",
      new ShowcaseMapper(),
    );
    expect(asString.idOrName).toBe("abc");
    const asInt = expectRoundTrip("showcase-union-int.json", new ShowcaseMapper());
    expect(asInt.idOrName).toBe(7);

    // Discriminated (tagged) union (Circle | Square) selected by `kind`.
    const circle = expectRoundTrip("showcase-shape-circle.json", new ShowcaseMapper());
    expect(circle.shape).toMatchObject({ kind: "circle", radius: 2.5 });
    const square = expectRoundTrip("showcase-shape-square.json", new ShowcaseMapper());
    expect(square.shape).toMatchObject({ kind: "square", side: 4 });

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    // An unmatchable wire token (boolean) names the admissible kinds.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, idOrName: true }),
    ).toThrow(/expected one of: string, integer/);

    // An unknown discriminator value is rejected (closed value set, P13.1).
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        shape: { kind: "triangle" },
      }),
    ).toThrow(/unknown discriminator kind triangle/);
  });

  test("holds a union member to the constraints of the branch it selects", () => {
    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;
    const mapper = new ShowcaseMapper();

    // The string branch's own `minLength` and the integer branch's own
    // `minimum` — each enforced only for the branch the token selects.
    expect(mapper.fromIntermediate({ ...base, idOrName: "abc" }).idOrName).toBe("abc");
    expect(mapper.fromIntermediate({ ...base, idOrName: 1 }).idOrName).toBe(1);
    expect(() => mapper.fromIntermediate({ ...base, idOrName: "ab" })).toThrow(
      /idOrName: must have length >= 3, got 2/,
    );
    expect(() => mapper.fromIntermediate({ ...base, idOrName: 0 })).toThrow(
      /idOrName: must be >= 1, got 0/,
    );

    // A closed value set on a branch narrows to a literal union type, and an
    // unknown string is a Violation.
    expect(mapper.fromIntermediate({ ...base, mode: "manual" }).mode).toBe("manual");
    expect(() => mapper.fromIntermediate({ ...base, mode: "turbo" })).toThrow(
      /mode: must be one of \["auto", "manual"\]/,
    );

    // The array branch's `minItems`/`uniqueItems` and the string branch's
    // `pattern`, on the same union.
    expect(() => mapper.fromIntermediate({ ...base, measurements: [] })).toThrow(
      /measurements: must have at least 1 items, got 0/,
    );
    expect(() =>
      mapper.fromIntermediate({ ...base, measurements: [1.5, 1.5] }),
    ).toThrow(/duplicate items: element at index 1 equals index 0/);
    expect(() => mapper.fromIntermediate({ ...base, measurements: "AUTO" })).toThrow(
      /measurements: must match pattern/,
    );

    // Serialize re-runs the selected branch's constraints (P12).
    const valid = mapper.fromIntermediate({ ...base, idOrName: "abc" });
    expect(() => mapper.toIntermediate({ ...valid, idOrName: "ab" })).toThrow(
      /idOrName: must have length >= 3, got 2/,
    );
    expect(() => mapper.toIntermediate({ ...valid, measurements: [2, 2] })).toThrow(
      /duplicate items: element at index 1 equals index 0/,
    );

    // A named element union validates through its own mapper, in both
    // directions, with the element's index on the violation path.
    expect(mapper.fromIntermediate({ ...base, segments: ["ab", 0] }).segments).toEqual([
      "ab",
      0,
    ]);
    expect(() => mapper.fromIntermediate({ ...base, segments: ["a"] })).toThrow(
      /segments\[0\]: must have length >= 2, got 1/,
    );
    expect(() => mapper.toIntermediate({ ...valid, segments: [-1] })).toThrow(
      /must be >= 0, got -1/,
    );
  });

  test("round-trips the free-form object as a union branch and a named model", () => {
    // The inline object branch of the `payload` union, and the named `Extras`
    // model: members are carried verbatim in both.
    const asObject = expectRoundTrip("showcase-freeform.json", new ShowcaseMapper());
    expect(asObject.payload).toEqual({ note: "free-form", big: 9007199254740992 });
    expect(asObject.extras?.additionalProperties).toEqual({ note: "free-form" });

    // The same union's string branch, selected by the wire token.
    const asString = expectRoundTrip(
      "showcase-freeform-string.json",
      new ShowcaseMapper(),
    );
    expect(asString.payload).toBe("text");

    // The named free-form model round-trips standalone, nested members included.
    const extras = expectRoundTrip("extras.json", new ExtrasMapper());
    expect(extras.additionalProperties.nested).toEqual({ a: 1 });

    // maxProperties over the member set is enforced on parse…
    expect(() =>
      new ExtrasMapper().fromIntermediate({ a: 1, b: 2, c: 3, d: 4, e: 5 }),
    ).toThrow(/must have at most 4 properties/);

    // …and on serialize (P12).
    expect(() =>
      new ExtrasMapper().toIntermediate({
        additionalProperties: { a: 1, b: 2, c: 3, d: 4, e: 5 },
      }),
    ).toThrow(/must have at most 4 properties/);

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    // An unmatchable wire token (boolean) names the admissible kinds.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, payload: true }),
    ).toThrow(/expected one of: object, string/);
  });

  test("round-trips a tagged union whose object branches are written inline", () => {
    // Each `note` branch is named by its `x-ts-name` override and emitted as an
    // interface with its own mapper, so the union narrows on the `kind` literal.
    const text = expectRoundTrip("showcase-note-text.json", new ShowcaseMapper());
    const note = text.note as TextNote;
    expect(note.kind).toBe("text");
    expect(note.body).toBe("remember the milk");
    // The branch stays open: an unknown member is preserved (P13).
    expect(note.additionalProperties).toEqual({ pinned: true });

    const link = expectRoundTrip("showcase-note-link.json", new ShowcaseMapper());
    expect((link.note as LinkNote).href).toBe("https://example.test/notes/1");

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    // The selected branch's own constraints are enforced.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        note: { kind: "text", body: "" },
      }),
    ).toThrow(/must have length >= 1/);

    // An unknown tag value matches no branch.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, note: { kind: "audio" } }),
    ).toThrow(/unknown discriminator kind/);

    // Serialize dispatches on the tag and re-runs that branch's constraints (P12).
    expect(() =>
      new ShowcaseMapper().toIntermediate({
        ...link,
        note: { kind: "link", href: "", additionalProperties: {} },
      }),
    ).toThrow(/must have length >= 1/);
  });

  test("round-trips a union written inline on a property with an object branch", () => {
    // `detail`'s lone structured object branch derives `ShowcaseDetailObject` from
    // the union it belongs to and gets an interface + mapper, so its members keep
    // their constraints while the string branch selects on its own token.
    const object = expectRoundTrip("showcase-detail-object.json", new ShowcaseMapper());
    const detail = object.detail as ShowcaseDetailObject;
    expect(detail.code).toBe("E_LIMIT");
    expect(detail.hint).toBe("retry later");
    // The branch stays open: an unknown member is preserved (P13).
    expect(detail.additionalProperties).toEqual({ retryAfterMs: 250 });

    const text = expectRoundTrip("showcase-detail-string.json", new ShowcaseMapper());
    expect(text.detail).toBe("E_LIMIT");

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    // The object branch's own constraints are enforced.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, detail: { code: "" } }),
    ).toThrow(/must have length >= 1/);

    // A value admitted by no branch names the admissible ones.
    expect(() => new ShowcaseMapper().fromIntermediate({ ...base, detail: 7 })).toThrow(
      /expected one of: ShowcaseDetailObject, string/,
    );

    // Serialize picks the object branch by shape and re-runs its constraints (P12).
    expect(() =>
      new ShowcaseMapper().toIntermediate({
        ...object,
        detail: { code: "", additionalProperties: {} },
      }),
    ).toThrow(/must have length >= 1/);
  });

  test("round-trips a tagged object union mixed with a scalar branch", () => {
    // `shapeOrName` composes both selector layers: the JSON token picks
    // object-vs-string, then the `kind` const picks Circle-vs-Square. Circle and
    // Square are the same branch types the `shape` union uses.
    const square = expectRoundTrip(
      "showcase-shape-or-name-square.json",
      new ShowcaseMapper(),
    );
    expect(square.shapeOrName).toMatchObject({ kind: "square", side: 4 });

    const named = expectRoundTrip(
      "showcase-shape-or-name-string.json",
      new ShowcaseMapper(),
    );
    expect(named.shapeOrName).toBe("unit-square");

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    // The object token still routes through the discriminator, so an unknown tag
    // is rejected rather than falling back to the string branch.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        shapeOrName: { kind: "triangle" },
      }),
    ).toThrow(/unknown discriminator kind triangle/);

    // A token matching no branch names all admissible ones.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, shapeOrName: 7 }),
    ).toThrow(/expected one of: Circle, Square, string/);
  });

  test("round-trips a union with an array branch", () => {
    // `measurements` is `number[] | string`: TypeScript carries the array branch
    // structurally (no synthesized variant type) and narrows with Array.isArray.
    const list = expectRoundTrip(
      "showcase-measurements-array.json",
      new ShowcaseMapper(),
    );
    expect(list.measurements).toEqual([1.5, 2.5, 3.75]);

    const preset = expectRoundTrip(
      "showcase-measurements-string.json",
      new ShowcaseMapper(),
    );
    expect(preset.measurements).toBe("auto");

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    // A token matching neither branch names both admissible kinds.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, measurements: true }),
    ).toThrow(/expected one of: number\[\], string/);
  });

  test("round-trips unions in element positions", () => {
    // Three positions with no property of their own: an array element at a
    // named union (`shapes`), an array element at an inline union the loader
    // names `ShowcaseSegmentsItem`, and a map member at an inline union named
    // `ChoicesValue`. Each element runs its union's own mapper, so a bad value
    // is reported at its index / key.
    const value = expectRoundTrip("showcase-element-unions.json", new ShowcaseMapper());

    expect(value.shapes).toEqual([
      { kind: "circle", radius: 2.5, additionalProperties: {} },
      { kind: "square", side: 4, additionalProperties: {} },
    ]);
    expect(value.segments).toEqual(["alpha", 7]);
    // Element nullability is the element's own concern: `(string | null)[]`,
    // so an explicit null is a member rather than a violation.
    expect(value.slots).toEqual(["first", null, "third"]);
    expect(value.choices?.additionalProperties.primary).toEqual({
      kind: "circle",
      radius: 1,
      additionalProperties: {},
    });

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        shapes: [{ kind: "circle", radius: 1 }, true],
      }),
    ).toThrow(/shapes\[1\]/);
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        shapes: [{ kind: "triangle" }],
      }),
    ).toThrow(/triangle/);
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, segments: ["ok", 1.5] }),
    ).toThrow(/segments\[1\]/);
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        choices: { primary: "circle" },
      }),
    ).toThrow(/primary/);
  });

  test("rejects invalid in-memory values on serialize (P12, both directions)", () => {
    // A valid model round-trips; mutating a single field to an out-of-spec value
    // and re-serializing (toIntermediate) is rejected before any wire object is
    // produced, with the same informative reason as the parse path.
    const full = new ShowcaseMapper().fromIntermediate(
      loadFixture("showcase-full.json"),
    );

    // Numeric bound: an in-memory value past `maximum` fails to serialize.
    expect(() =>
      new ShowcaseMapper().toIntermediate({ ...full, priority: 42 }),
    ).toThrow(/must be <= 10, got 42/);

    // String length: an in-memory over-long string fails to serialize.
    expect(() =>
      new ShowcaseMapper().toIntermediate({ ...full, code: "abcdef" }),
    ).toThrow(/must have length <= 5, got 6/);

    // Pattern: an in-memory off-pattern value fails to serialize.
    expect(() => new ShowcaseMapper().toIntermediate({ ...full, sku: "xyz" })).toThrow(
      /must match pattern/,
    );

    // Format: an in-memory malformed uuid fails to serialize.
    expect(() =>
      new ShowcaseMapper().toIntermediate({ ...full, requestId: "nope" }),
    ).toThrow(/must be a valid uuid, got "nope"/);

    // Array: an in-memory duplicate (uniqueItems) fails to serialize.
    expect(() =>
      new ShowcaseMapper().toIntermediate({ ...full, aliases: ["dup", "dup"] }),
    ).toThrow(/duplicate items: element at index 1 equals index 0/);

    // Closed value-set: a mutated enum member fails to serialize.
    expect(() =>
      new ShowcaseMapper().toIntermediate({
        ...full,
        status: "archived" as (typeof full)["status"],
      }),
    ).toThrow(/must be one of \["active", "inactive", "pending"\], got "archived"/);

    // const: a mutated integer const fails to serialize.
    expect(() =>
      new ShowcaseMapper().toIntermediate({
        ...full,
        revision: 2 as (typeof full)["revision"],
      }),
    ).toThrow(/must equal 1/);

    // allOf-merged bound: an in-memory `size` past the tightened maximum fails.
    const widget = new WidgetMapper().fromIntermediate(loadFixture("widget.json"));
    expect(() => new WidgetMapper().toIntermediate({ ...widget, size: 25 })).toThrow(
      /must be <= 20, got 25/,
    );

    // Object dependentRequired: a shipping street with no zip fails to serialize.
    expect(() =>
      new ContactTsMapper().toIntermediate({
        shippingStreet: "1 Main St",
        additionalProperties: {},
      }),
    ).toThrow(/property "shippingZip" is required when "shippingStreet" is present/);

    // Object member-count: an empty map is below minProperties:1 on serialize.
    expect(() =>
      new AttributesMapper().toIntermediate({ additionalProperties: {} }),
    ).toThrow(/must have at least 1 properties, got 0/);
    // propertyNames key-shape: an over-long key fails to serialize.
    expect(() =>
      new AttributesMapper().toIntermediate({
        additionalProperties: { toolongkey: "1" },
      }),
    ).toThrow(/invalid property name "toolongkey": must have length <= 8, got 10/);

    // A valid model still serializes cleanly (no false rejection).
    expect(() => new ShowcaseMapper().toIntermediate(full)).not.toThrow();
  });

  test("roundtrips materialized contentEncoding bytes and rejects malformed", () => {
    // blob (base64) and urlBlob (base64url) round-trip: a JSON string on the
    // wire, a native Uint8Array in the model, re-encoded byte-identically via the
    // pure-JS codec. The same bytes (">>>") encode to "Pj4+" vs "Pj4-".
    const bytes = expectRoundTrip("showcase-bytes.json", new ShowcaseMapper());
    const expected = new Uint8Array([0x3e, 0x3e, 0x3e]);
    expect(bytes.blob).toEqual(expected);
    expect(bytes.urlBlob).toEqual(expected);

    const base = {
      kind: "showcase",
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    // A base64 field using the URL-safe alphabet is rejected by the pinned regex.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, blob: "Pj4-" }),
    ).toThrow(/must be base64-encoded, got "Pj4-"/);

    // A base64 field missing padding is rejected.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, blob: "aGk" }),
    ).toThrow(/must be base64-encoded/);

    // A base64url field carrying padding is rejected.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, urlBlob: "aGk=" }),
    ).toThrow(/must be base64url-encoded, got "aGk="/);
  });

  test("round-trips object shapes written inline", () => {
    // An object written inline in a value position is named after that position
    // and emitted as an ordinary model: a property (`location`, with its own
    // nested `geo`), a nullable property (`audit`), an array element (`rows`), a
    // map and its member (`ledger`), and a free-form bag (`metadata`). The same
    // fixture covers a typed map's member constraints (`quotas`, `tokens`,
    // `nicknames`) and a nested array (`grid`).
    const value = expectRoundTrip("showcase-inline-shapes.json", new ShowcaseMapper());

    expect(value.grid).toEqual([[1, 2], [3]]);
    expect(value.location).toEqual({
      city: "Springfield",
      geo: { lat: 39.8, lon: -89.6, additionalProperties: {} },
      additionalProperties: {},
    });
    expect(value.audit).toEqual({ by: "alice", additionalProperties: {} });
    expect(value.rows?.[0]).toEqual({ cell: "a1", additionalProperties: {} });
    // The member override renamed the member (`ledgerTs`); the hoisted types keep
    // their position-derived names.
    expect(value.ledgerTs?.additionalProperties.opening).toEqual({
      amount: 100,
      additionalProperties: {},
    });
    expect(value.metadata?.additionalProperties).toEqual({
      source: "import",
      batch: 7,
    });
    expect(value.quotas?.additionalProperties).toEqual({ cpu: 20, memory: 100 });
    // A null member of a nullable map is a member, not a violation.
    expect(value.nicknames?.additionalProperties).toEqual({ short: "al", none: null });

    const base = {
      kind: "showcase",
      revision: 1,
      enabled: true,
      status: "active",
      tier: 1,
      scale: 1.5,
      name: "w",
      count: 1,
      active: true,
      category: "tools",
    } as const;

    // A hoisted shape validates like any other model, at the nested path.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, location: { city: "" } }),
    ).toThrow(/location\.city/);
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, rows: [{ cell: "ok" }, {}] }),
    ).toThrow(/rows\[1\]\.cell/);
    // A nested array reports the failing element at its own two-dimensional index.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, grid: [[1], [2, 1.5]] }),
    ).toThrow(/grid\[1\]\[1\]/);
    // A typed map's member constraints are enforced, keyed by the member.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, quotas: { cpu: 7 } }),
    ).toThrow(/cpu/);
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, tokens: { primary: "AB" } }),
    ).toThrow(/primary/);
    expect(() =>
      new ShowcaseMapper().fromIntermediate({ ...base, nicknames: { tiny: "a" } }),
    ).toThrow(/tiny/);
    // The free-form bag's member-count bound rides with the hoisted type.
    expect(() =>
      new ShowcaseMapper().fromIntermediate({
        ...base,
        metadata: { a: 1, b: 2, c: 3, d: 4 },
      }),
    ).toThrow(/at most 3/);

    // Serialize re-runs every member's own constraints before emitting (P12).
    expect(() =>
      new QuotasMapper().toIntermediate({ additionalProperties: { cpu: 7 } }),
    ).toThrow(/cpu/);
    expect(() =>
      new TokensMapper().toIntermediate({ additionalProperties: { primary: "AB" } }),
    ).toThrow(/primary/);
    expect(() =>
      new NicknamesMapper().toIntermediate({ additionalProperties: { tiny: "a" } }),
    ).toThrow(/tiny/);
  });
});

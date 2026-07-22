import { describe, expect, test } from "vitest";

import {
  DEFAULT_PRIORITY,
  LabelsMapper,
  MessageMapper,
  RoomMapper,
  SendMessageInputMapper,
  SendMessageOutputMapper,
  type Labels,
  type Message,
} from "../json_schema/definitions/chat/models.ts";
import { ValidationError } from "../json_schema/definitions/chat/definitions.ts";
import {
  fixtureBytes,
  loadFixture as loadFixtureFrom,
  roundTripFixture,
  type IntermediateMapper,
} from "./json-converter-helper.ts";

const wireFixtureDir = new URL("../../wire/json_schema/chat/", import.meta.url);

function loadFixture(name: string): unknown {
  return loadFixtureFrom(wireFixtureDir, name);
}

// Round-trip a fixture through the Temporal data converter (driven by the
// generated mapper) and assert the re-serialized JSON is JSON-equal to the
// fixture. TS mappers preserve explicit nulls, so all chat fixtures use exact
// JSON-equality (no optional+nullable collapse — unlike Go).
function expectRoundTrip<T>(name: string, mapper: IntermediateMapper<T>): T {
  const { value, serialized } = roundTripFixture(
    mapper,
    fixtureBytes(wireFixtureDir, name),
  );
  expect(serialized).toEqual(loadFixture(name));
  return value;
}

describe("json-schema chat generated definitions", () => {
  test("roundtrips canonical wire fixtures through the Temporal converter", () => {
    const message = expectRoundTrip("message-minimal.json", new MessageMapper());
    expect(message).toMatchObject<Message>({
      kind: "text",
      body: "hi",
    });
    expect(message.replyToId).toBeUndefined();
    expect(message.priority ?? DEFAULT_PRIORITY).toBe(0);

    const fullMessage = expectRoundTrip("message-full.json", new MessageMapper());
    expect(fullMessage.replyToId).toBeNull();
    expect(fullMessage.priority).toBe(7);

    const room = expectRoundTrip("room-open.json", new RoomMapper());
    expect(room.additionalProperties).toEqual({ "x-extra": 42 });

    const labels = expectRoundTrip("labels.json", new LabelsMapper());
    expect(labels).toMatchObject<Labels>({
      additionalProperties: { env: "prod", team: "core" },
    });

    const request = expectRoundTrip(
      "send-message-input.json",
      new SendMessageInputMapper(),
    );
    expect(request.message.body).toBe("hi");

    const response = expectRoundTrip(
      "send-message-output.json",
      new SendMessageOutputMapper(),
    );
    expect(response.messageId).toBe("m1");
  });

  test("reports JSON schema validation errors", () => {
    expect(() =>
      new SendMessageInputMapper().fromIntermediate({
        roomId: "r1",
        message: { kind: "text", body: "hi" },
        extra: true,
      }),
    ).toThrow(ValidationError);

    expect(() =>
      new MessageMapper().fromIntermediate({ kind: "image", body: "hi" }),
    ).toThrow(ValidationError);

    expect(() => new SendMessageOutputMapper().fromIntermediate({})).toThrow(
      ValidationError,
    );
  });
});

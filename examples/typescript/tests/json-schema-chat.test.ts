import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";

import {
  DEFAULT_PRIORITY,
  type Labels,
  type Message,
  parseLabels,
  parseMessage,
  parseRoom,
  parseSendMessageInput,
  parseSendMessageOutput,
  serializeLabels,
  serializeMessage,
  serializeRoom,
  serializeSendMessageInput,
  serializeSendMessageOutput,
} from "../json_schema/definitions/chat/models.ts";
import { ValidationError } from "../json_schema/definitions/chat/json.ts";

const wireFixtureDir = new URL("../../wire/json_schema/chat/", import.meta.url);

function loadFixture(name: string): unknown {
  return JSON.parse(readFileSync(new URL(name, wireFixtureDir), "utf8"));
}

function expectRoundTrip<T>(
  name: string,
  parse: (raw: unknown) => T,
  serialize: (value: T) => unknown,
): T {
  const wire = loadFixture(name);
  const value = parse(wire);
  expect(serialize(value)).toEqual(wire);
  return value;
}

describe("json-schema chat generated definitions", () => {
  test("roundtrips canonical wire fixtures through parse and serialize helpers", () => {
    const message = expectRoundTrip(
      "message-minimal.json",
      parseMessage,
      serializeMessage,
    );
    expect(message).toMatchObject<Message>({
      kind: "text",
      body: "hi",
    });
    expect(message.replyToId).toBeUndefined();
    expect(message.priority ?? DEFAULT_PRIORITY).toBe(0);

    const fullMessage = expectRoundTrip(
      "message-full.json",
      parseMessage,
      serializeMessage,
    );
    expect(fullMessage.replyToId).toBeNull();
    expect(fullMessage.priority).toBe(7);

    const room = expectRoundTrip("room-open.json", parseRoom, serializeRoom);
    expect(room.additionalProperties).toEqual({ "x-extra": 42 });

    const labels = expectRoundTrip("labels.json", parseLabels, serializeLabels);
    expect(labels).toMatchObject<Labels>({
      additionalProperties: { env: "prod", team: "core" },
    });

    const request = expectRoundTrip(
      "send-message-input.json",
      parseSendMessageInput,
      serializeSendMessageInput,
    );
    expect(request.message.body).toBe("hi");

    const response = expectRoundTrip(
      "send-message-output.json",
      parseSendMessageOutput,
      serializeSendMessageOutput,
    );
    expect(response.messageId).toBe("m1");
  });

  test("reports JSON schema validation errors", () => {
    expect(() =>
      parseSendMessageInput({
        roomId: "r1",
        message: { kind: "text", body: "hi" },
        extra: true,
      }),
    ).toThrow(ValidationError);

    expect(() => parseMessage({ kind: "image", body: "hi" })).toThrow(ValidationError);

    expect(() => parseSendMessageOutput({})).toThrow(ValidationError);
  });
});

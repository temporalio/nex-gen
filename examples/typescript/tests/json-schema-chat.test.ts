import { readFileSync } from "node:fs";
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
  test("roundtrips canonical wire fixtures through mapper helpers", () => {
    const messageMapper = new MessageMapper();
    const message = expectRoundTrip(
      "message-minimal.json",
      (raw) => messageMapper.fromIntermediate(raw),
      (value) => messageMapper.toIntermediate(value),
    );
    expect(message).toMatchObject<Message>({
      kind: "text",
      body: "hi",
    });
    expect(message.replyToId).toBeUndefined();
    expect(message.priority ?? DEFAULT_PRIORITY).toBe(0);

    const fullMessage = expectRoundTrip(
      "message-full.json",
      (raw) => messageMapper.fromIntermediate(raw),
      (value) => messageMapper.toIntermediate(value),
    );
    expect(fullMessage.replyToId).toBeNull();
    expect(fullMessage.priority).toBe(7);

    const roomMapper = new RoomMapper();
    const room = expectRoundTrip(
      "room-open.json",
      (raw) => roomMapper.fromIntermediate(raw),
      (value) => roomMapper.toIntermediate(value),
    );
    expect(room.additionalProperties).toEqual({ "x-extra": 42 });

    const labelsMapper = new LabelsMapper();
    const labels = expectRoundTrip(
      "labels.json",
      (raw) => labelsMapper.fromIntermediate(raw),
      (value) => labelsMapper.toIntermediate(value),
    );
    expect(labels).toMatchObject<Labels>({
      additionalProperties: { env: "prod", team: "core" },
    });

    const sendMessageInputMapper = new SendMessageInputMapper();
    const request = expectRoundTrip(
      "send-message-input.json",
      (raw) => sendMessageInputMapper.fromIntermediate(raw),
      (value) => sendMessageInputMapper.toIntermediate(value),
    );
    expect(request.message.body).toBe("hi");

    const sendMessageOutputMapper = new SendMessageOutputMapper();
    const response = expectRoundTrip(
      "send-message-output.json",
      (raw) => sendMessageOutputMapper.fromIntermediate(raw),
      (value) => sendMessageOutputMapper.toIntermediate(value),
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

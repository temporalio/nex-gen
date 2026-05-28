import { describe, expect, test } from "vitest";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as common from "@temporalio/common";
import { temporal } from "@temporalio/proto";
import * as nexus from "nexus-rpc";
import { nexusValue, payloadConverter } from "../nexus-api-gen-runtime.ts";

import {
  User,
  UserCapability,
  UserStatus,
  TypeShowcase,
} from "../type-showcase/index.ts";
import type { SetProfileRequest } from "../type-showcase/index.ts";
import { executeWorkflowWithNexus, withWorkflowEnvironment } from "./helpers.ts";

const wireFixtureDir = fileURLToPath(
  new URL("../../wire/type-showcase/", import.meta.url),
);
const typescriptWireFixture = `${wireFixtureDir}set-profile-request.typescript.payloads`;
const pythonWireFixture = `${wireFixtureDir}set-profile-request.python.payloads`;
const workflowsPath = fileURLToPath(
  new URL("./workflows/type-showcase.ts", import.meta.url),
);

const userProfile = () => ({
  capabilities: UserCapability.ReadProfile | UserCapability.UpdateEmail,
  notificationTarget: { tag: "email" as const, value: "old@example.com" },
  syncState: { tag: "ok" as const, value: "synced" },
  address: {
    street: "1 Main St",
    city: "Portland",
    country: "US",
    coordinates: [45.5152, -122.6784] as [number, number],
  },
  metadata: { tier: "enterprise" },
  tags: ["admin", "beta"],
});

const userResource = (email: string, displayName: string) =>
  new User("user-123", email, displayName, UserStatus.Active, userProfile());

function sampleSetProfileRequest(): SetProfileRequest {
  return {
    userId: "user-123",
    profile: userProfile(),
  };
}

function writePayloads(
  path: string,
  payloads: temporal.api.common.v1.IPayload[],
): void {
  mkdirSync(wireFixtureDir, { recursive: true });
  const payloadWrapper = temporal.api.common.v1.Payloads.create({
    payloads,
  });
  const bytes = temporal.api.common.v1.Payloads.encode(payloadWrapper).finish();
  writeFileSync(path, `${Buffer.from(bytes).toString("base64")}\n`);
}

function readPayloads(path: string): common.Payload[] {
  const bytes = Buffer.from(readFileSync(path, "utf8").trim(), "base64");
  return temporal.api.common.v1.Payloads.decode(bytes).payloads as common.Payload[];
}

function encodeRequest(request: SetProfileRequest): temporal.api.common.v1.IPayload[] {
  return (
    common.toPayloads(
      payloadConverter,
      nexusValue("type-showcase.set-profile-request", request),
    ) ?? []
  );
}

function decodeRequest(payloads: temporal.api.common.v1.IPayload[]): SetProfileRequest {
  return common.fromPayloadsAtIndex<SetProfileRequest>(payloadConverter, 0, payloads);
}

function payloadJson(
  payloads: temporal.api.common.v1.IPayload[],
): Record<string, unknown> {
  expect(payloads).toHaveLength(1);
  const payload = payloads[0];
  expect(Buffer.from(payload.metadata?.encoding ?? []).toString()).toBe("json/nexus");
  expect(Buffer.from(payload.metadata?.nexusType ?? []).toString()).toBe(
    "type-showcase.set-profile-request",
  );
  return JSON.parse(Buffer.from(payload.data ?? []).toString()) as Record<
    string,
    unknown
  >;
}

describe("type-showcase generated output", () => {
  test("exposes WIT-native type showcase metadata", () => {
    expect(TypeShowcase.name).toBe("TypeShowcase");
    expect(TypeShowcase.operations.getUser.name).toBe("GetUser");
    expect(TypeShowcase.operations.updateEmail.name).toBe("UpdateEmail");
    expect(TypeShowcase.operations.rename.name).toBe("Rename");
    expect(TypeShowcase.operations.setProfile.name).toBe("SetProfile");
    expect(TypeShowcase.operations.deactivate.name).toBe("Deactivate");
    expect(UserStatus.Active).toBe(0);
    expect(UserCapability.ReadProfile).toBe(1);
    expect(UserCapability.UpdateEmail).toBe(2);
  });

  test("passes WIT records through a real Nexus client", async () => {
    await withWorkflowEnvironment(async (env) => {
      const calls: Array<[string, unknown]> = [];
      const handler = nexus.serviceHandler(TypeShowcase, {
        async getUser(_ctx, input) {
          calls.push(["GetUser", input]);
          return userResource("old@example.com", "Old Name");
        },
        async updateEmail(_ctx, input) {
          calls.push(["UpdateEmail", input]);
          return userResource(input.email, "Old Name");
        },
        async rename(_ctx, input) {
          calls.push(["Rename", input]);
          return userResource("new@example.com", input.displayName);
        },
        async setProfile(_ctx, input) {
          calls.push(["SetProfile", input]);
          return userResource("new@example.com", "New Name");
        },
        async deactivate(_ctx, input) {
          calls.push(["Deactivate", input]);
        },
      });

      const result = await executeWorkflowWithNexus<{
        deactivated: boolean;
        displayName: string;
        email: string;
        hasReadProfile: boolean;
        notificationTag: string;
        userId: string;
      }>(env, {
        endpoint: "type-showcase",
        nexusServices: [handler],
        workflowType: "typeShowcaseCaller",
        workflowsPath,
      });

      expect(result).toEqual({
        deactivated: true,
        displayName: "New Name",
        email: "new@example.com",
        hasReadProfile: true,
        notificationTag: "email",
        userId: "user-123",
      });
      expect(calls).toEqual([
        [
          "GetUser",
          {
            consistencyToken: "read-123",
            userId: "user-123",
          },
        ],
        [
          "UpdateEmail",
          {
            email: "new@example.com",
            userId: "user-123",
          },
        ],
        [
          "Rename",
          {
            displayName: "New Name",
            userId: "user-123",
          },
        ],
        [
          "Deactivate",
          {
            reason: "requested",
            userId: "user-123",
          },
        ],
      ]);
    });
  });

  test("serializes and reads cross-language request wire fixtures", () => {
    const expected = sampleSetProfileRequest();
    const typescriptPayloads = encodeRequest(expected);
    writePayloads(typescriptWireFixture, typescriptPayloads);

    expect(payloadJson(typescriptPayloads)["user-id"]).toBe("user-123");
    expect(decodeRequest(readPayloads(typescriptWireFixture))).toEqual(expected);
    expect(decodeRequest(readPayloads(pythonWireFixture))).toEqual(expected);
  });
});

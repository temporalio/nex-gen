import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import * as nexus from "nexus-rpc";

import { User } from "../user-service/resources.ts";
import { UserService } from "../user-service/service.ts";
import { executeWorkflowWithNexus, withWorkflowEnvironment } from "./helpers.ts";

const workflowsPath = fileURLToPath(
  new URL("./workflows/user-service.ts", import.meta.url),
);

describe("user-service generated output", () => {
  test("exposes basic WIT-native service metadata", () => {
    expect(UserService.name).toBe("UserService");
    expect(UserService.operations.getUser.name).toBe("GetUser");
    expect(UserService.operations.updateEmail.name).toBe("UpdateEmail");
  });

  test("passes WIT records through a real Nexus client", async () => {
    await withWorkflowEnvironment(async (env) => {
      const calls: Array<[string, unknown]> = [];
      const handler = nexus.serviceHandler(UserService, {
        async getUser(_ctx, input) {
          calls.push(["GetUser", input]);
          return new User(input.userId, "old@example.com");
        },
        async updateEmail(_ctx, input) {
          calls.push(["UpdateEmail", input]);
          return new User(input.userId, input.email);
        },
      });

      const result = await executeWorkflowWithNexus<{
        initialEmail: string;
        updatedEmail: string;
        userId: string;
      }>(env, {
        endpoint: "user-service",
        nexusServices: [handler],
        workflowType: "userServiceCaller",
        workflowsPath,
      });

      expect(result).toEqual({
        initialEmail: "old@example.com",
        updatedEmail: "new@example.com",
        userId: "user-123",
      });
      expect(calls).toEqual([
        [
          "GetUser",
          {
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
      ]);
    });
  });
});

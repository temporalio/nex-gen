import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import * as common from "@temporalio/common";
import type { temporal } from "@temporalio/proto";
import * as nexus from "nexus-rpc";

import {
  ActivityOptions,
  TypeRoundtripService,
  retryPolicyFromProto,
} from "../type-roundtrip/index.ts";
import { executeWorkflowWithNexus, withWorkflowEnvironment } from "./helpers.ts";

const workflowsPath = fileURLToPath(
  new URL("./workflows/type-roundtrip.ts", import.meta.url),
);

describe("type-roundtrip generated output", () => {
  test("exposes type roundtrip service metadata", () => {
    expect(TypeRoundtripService.name).toBe("TypeRoundtripService");
    expect(TypeRoundtripService.operations.retryPolicyOperation.name).toBe(
      "RetryPolicyOperation",
    );
    expect(TypeRoundtripService.operations.activityOptionsOperation.name).toBe(
      "ActivityOptionsOperation",
    );
  });

  test("round-trips activity options", () => {
    const retryPolicy = retryPolicyFromProto(
      common.compileRetryPolicy({ maximumAttempts: 3 }),
    );
    const activityOptions: ActivityOptions = {
      taskQueue: "demo-task-queue",
      retryPolicy,
      scheduleToCloseTimeout: "1 minute",
      priority: {
        priorityKey: 1,
        fairnessKey: "customer-123",
      },
    };

    const proto = ActivityOptions.toProto(activityOptions);
    expect(proto?.taskQueue?.name).toBe("demo-task-queue");
    expect(proto?.retryPolicy?.maximumAttempts).toBe(3);

    const roundTripped = ActivityOptions.fromProto(proto);
    expect(roundTripped?.taskQueue).toBe("demo-task-queue");
    expect(roundTripped?.retryPolicy.maximumAttempts).toBe(3);
    expect(roundTripped?.scheduleToCloseTimeout).toBe(60_000);
    expect(roundTripped?.priority?.priorityKey).toBe(1);
  });

  test("round-trips proto-backed types through a real Nexus client", async () => {
    await withWorkflowEnvironment(async (env) => {
      const calls: Array<[string, unknown]> = [];
      const handler = nexus.serviceHandler(TypeRoundtripService, {
        async retryPolicyOperation(_ctx, input) {
          calls.push(["RetryPolicyOperation", input]);
          return input;
        },
        async activityOptionsOperation(_ctx, input) {
          calls.push(["ActivityOptionsOperation", input]);
          return input;
        },
      });

      const result = await executeWorkflowWithNexus<{
        priorityKey: number | undefined;
        retryMaximumAttempts: number | undefined;
        scheduleToCloseTimeout: common.Duration | undefined;
        taskQueue: string | undefined;
      }>(env, {
        endpoint: "temporal-system",
        nexusServices: [handler],
        workflowType: "typeRoundtripCaller",
        workflowsPath,
      });

      expect(result).toEqual({
        priorityKey: 4,
        retryMaximumAttempts: 3,
        scheduleToCloseTimeout: 7_000,
        taskQueue: "demo-task-queue",
      });
      expect(calls).toHaveLength(2);

      const retryRequest = calls[0]?.[1] as
        | temporal.api.common.v1.IRetryPolicy
        | undefined;
      expect(retryRequest?.maximumAttempts).toBe(3);

      const activityRequest = calls[1]?.[1] as
        | temporal.api.activity.v1.IActivityOptions
        | undefined;
      expect(activityRequest?.taskQueue?.name).toBe("demo-task-queue");
      expect(activityRequest?.retryPolicy?.maximumAttempts).toBe(3);
      expect(activityRequest?.scheduleToCloseTimeout?.seconds).toMatchObject({
        low: 7,
      });
      expect(activityRequest?.priority?.priorityKey).toBe(4);
    });
  });
});

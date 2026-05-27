import type * as common from "@temporalio/common";
import {
  ActivityOptions,
  activityOptionsOperation,
  retryPolicyFromProto,
  retryPolicyOperation,
} from "../../type-roundtrip/index.ts";

const TASK_QUEUE = "demo-task-queue";

export async function typeRoundtripCaller(): Promise<{
  priorityKey: number | undefined;
  retryMaximumAttempts: number | undefined;
  scheduleToCloseTimeout: common.Duration | undefined;
  taskQueue: string | undefined;
}> {
  const retryPolicy: common.RetryPolicy = { maximumAttempts: 3 };
  const retryHandle = await retryPolicyOperation(retryPolicy);
  const retryRoundTrip = retryPolicyFromProto(await retryHandle.result());

  const activityHandle = await activityOptionsOperation({
    taskQueue: TASK_QUEUE,
    scheduleToCloseTimeout: "7 seconds",
    retryPolicy,
    priority: {
      priorityKey: 4,
      fairnessKey: "tenant-a",
      fairnessWeight: 2.5,
    },
  });
  const activityRoundTrip = ActivityOptions.fromProto(await activityHandle.result());

  return {
    priorityKey: activityRoundTrip?.priority?.priorityKey,
    retryMaximumAttempts: retryRoundTrip.maximumAttempts,
    scheduleToCloseTimeout: activityRoundTrip?.scheduleToCloseTimeout,
    taskQueue: activityRoundTrip?.taskQueue,
  };
}

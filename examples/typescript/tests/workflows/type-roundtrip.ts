import type * as common from "@temporalio/common";
import { activityOptionsOperation } from "../../wit/type-roundtrip/index.ts";

const TASK_QUEUE = "demo-task-queue";

function durationSecondsToMillis(
  seconds:
    | { low?: number; toNumber?: () => number }
    | number
    | string
    | null
    | undefined,
): number | undefined {
  if (seconds == null) {
    return undefined;
  }
  if (typeof seconds === "object") {
    if (seconds.toNumber != null) {
      return seconds.toNumber() * 1000;
    }
    return seconds.low == null ? undefined : seconds.low * 1000;
  }
  return Number(seconds) * 1000;
}

export async function typeRoundtripCaller(): Promise<{
  priorityKey: number | undefined;
  retryMaximumAttempts: number | undefined;
  scheduleToCloseTimeout: number | undefined;
  taskQueue: string | undefined;
}> {
  const retryPolicy: common.RetryPolicy = { maximumAttempts: 3 };
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
  const activityRoundTrip = await activityHandle.result();
  const scheduleToCloseSeconds = activityRoundTrip.scheduleToCloseTimeout?.seconds;

  return {
    priorityKey: activityRoundTrip.priority?.priorityKey ?? undefined,
    retryMaximumAttempts: activityRoundTrip.retryPolicy?.maximumAttempts ?? undefined,
    scheduleToCloseTimeout: durationSecondsToMillis(scheduleToCloseSeconds),
    taskQueue: activityRoundTrip.taskQueue?.name ?? undefined,
  };
}

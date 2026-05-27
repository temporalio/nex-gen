import * as workflow from "@temporalio/workflow";
import { signalWithStartWorkflow } from "../../workflow-service/index.ts";

export async function exampleWorkflow(attempts: number, note: string): Promise<void> {
  void attempts;
  void note;
}

const wakeUpSignal = workflow.defineSignal<[number, string]>("wake-up");

export async function workflowServiceCaller(): Promise<{
  runId: string | undefined;
  workflowId: string;
}> {
  const handle = await signalWithStartWorkflow({
    workflow: exampleWorkflow,
    args: [3, "nexus"],
    id: "workflow-id",
    taskQueue: "demo-task-queue",
    requestId: "example-request",
    signal: wakeUpSignal,
    signalArgs: [7, "hello"],
    cronSchedule: "",
    runTimeout: "5 minutes",
  });
  return {
    runId: handle.runId,
    workflowId: handle.workflowId,
  };
}

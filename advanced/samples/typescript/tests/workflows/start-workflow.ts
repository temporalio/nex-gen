import { startWorkflow } from "../../wit/start-workflow/index.ts";

export async function exampleWorkflow(customerId: string): Promise<string> {
  return customerId;
}

export async function startWorkflowCaller(): Promise<{
  namespace: string;
  restartedRunId: string | undefined;
  runId: string | undefined;
  workflowId: string;
}> {
  const handle = await startWorkflow({
    workflow: exampleWorkflow,
    args: ["customer-123"],
    workflowId: "workflow-id",
    taskQueue: "demo-task-queue",
  });
  const restartedHandle = await handle.restartWorkflow(
    exampleWorkflow,
    "demo-task-queue",
  );
  await handle.cancel();
  return {
    namespace: handle.namespace,
    restartedRunId: restartedHandle.runId,
    runId: handle.runId,
    workflowId: handle.workflowId,
  };
}

import { randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import type { DataConverter } from "@temporalio/common";
import { TestWorkflowEnvironment } from "@temporalio/testing";
import { Worker } from "@temporalio/worker";
import type * as nexus from "nexus-rpc";

export const nexusDataConverter: DataConverter = {
  payloadConverterPath: fileURLToPath(
    new URL("../nex-gen-payload-converter.cjs", import.meta.url),
  ),
};

export async function withWorkflowEnvironment<T>(
  fn: (env: TestWorkflowEnvironment) => Promise<T>,
): Promise<T> {
  const temporalCliPath = resolveTemporalCliPath();
  const env = await TestWorkflowEnvironment.createLocal({
    client: { dataConverter: nexusDataConverter },
    server:
      temporalCliPath == null
        ? undefined
        : {
            executable: {
              type: "existing-path",
              path: temporalCliPath,
            },
          },
  });
  try {
    return await fn(env);
  } finally {
    await env.teardown();
  }
}

function resolveTemporalCliPath(): string | undefined {
  if (process.env.TEMPORAL_CLI_PATH != null) {
    return process.env.TEMPORAL_CLI_PATH;
  }
  const result = spawnSync("which", ["temporal"], { encoding: "utf8" });
  if (result.status !== 0) {
    return undefined;
  }
  const path = result.stdout.trim();
  return path === "" ? undefined : path;
}

export async function executeWorkflowWithNexus<T>(
  env: TestWorkflowEnvironment,
  options: {
    endpoint: string;
    nexusServices: nexus.ServiceHandler<any>[];
    workflowsPath: string;
    workflowType: string;
    args?: unknown[];
  },
): Promise<T> {
  const taskQueue = randomUUID();
  const worker = await Worker.create({
    connection: env.nativeConnection,
    dataConverter: nexusDataConverter,
    namespace: env.namespace ?? "default",
    nexusServices: options.nexusServices,
    taskQueue,
    workflowsPath: options.workflowsPath,
  });
  const endpoint = await env.createNexusEndpoint(options.endpoint, taskQueue);
  try {
    return await worker.runUntil(async () => {
      const result = await env.client.workflow.execute(options.workflowType, {
        args: options.args ?? [],
        taskQueue,
        workflowId: randomUUID(),
      });
      return result as T;
    });
  } finally {
    await env.deleteNexusEndpoint(endpoint);
  }
}

using System;
using System.Threading.Tasks;
using Temporalio.Common;
using Temporalio.Workflows;
using WorkflowExample = NexGen.WorkflowService;
using StartWorkflowExample = NexGen.StartWorkflowService;

namespace NexGen.DotNetExamples.Tests
{

    [Workflow]
    internal class ExampleWorkflow
    {
        [WorkflowRun]
        internal Task<string> RunAsync(string input) => Task.FromResult(input);

        [WorkflowSignal]
        internal Task NotifyAsync(string message) => Task.CompletedTask;
    }

    internal static class GeneratedApiCompileChecks
    {
        internal static Task<ExternalWorkflowHandle> SignalWithStartAsync() =>
            WorkflowExample.WorkflowServiceOperations.SignalWithStartWorkflowAsync<ExampleWorkflow, string>(
                workflow => workflow.RunAsync("workflow-input"),
                signal: workflow => workflow.NotifyAsync("signal-input"),
                options: new WorkflowExample.SignalWithStartWorkflowOptions
                {
                    Id = "workflow-id",
                    TaskQueue = "task-queue",
                    ExecutionTimeout = TimeSpan.FromMinutes(5),
                    RetryPolicy = new RetryPolicy { MaximumAttempts = 3 },
                });

        internal static Task<StartWorkflowExample.StartedWorkflow> StartWorkflowAsync() =>
            StartWorkflowExample.StartWorkflowServiceOperations.StartWorkflowAsync<ExampleWorkflow, string>(
                workflow => workflow.RunAsync("workflow-input"),
                options: new StartWorkflowExample.StartWorkflowOptions
                {
                    WorkflowId = "workflow-id",
                    TaskQueue = "task-queue",
                    WorkflowStartDelay = TimeSpan.FromSeconds(1),
                });
    }
}

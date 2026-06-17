using System;
using System.Threading.Tasks;
using Temporalio.Workflows;
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
        internal static Task<StartWorkflowExample.StartedWorkflow> StartWorkflowAsync() =>
            StartWorkflowExample.StartWorkflowServiceOperations.StartWorkflowAsync<ExampleWorkflow, string>(
                workflow => workflow.RunAsync("workflow-input"),
                options: new StartWorkflowExample.StartWorkflowOptions("workflow-id", "task-queue")
                {
                    WorkflowStartDelay = TimeSpan.FromSeconds(1),
                });
    }
}

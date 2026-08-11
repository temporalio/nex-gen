using System;
using System.Threading.Tasks;
using Temporalio.Workflows;
using StartWorkflowExample = Nexgen.StartWorkflowService;

namespace Nexgen.DotNetExamples.Tests
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
            StartWorkflowExample.Operations.StartWorkflowAsync(
                new StartWorkflowExample.StartWorkflowOptions(
                    nameof(ExampleWorkflow),
                    "workflow-id",
                    "task-queue")
                {
                    WorkflowStartDelay = TimeSpan.FromSeconds(1),
                });
    }
}

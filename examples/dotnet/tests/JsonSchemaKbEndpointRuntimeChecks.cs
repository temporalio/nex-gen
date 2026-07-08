using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using NexusRpc;
using NexusRpc.Handlers;
using Temporalio.Client;
using Temporalio.Testing;
using Temporalio.Worker;
using Temporalio.Workflows;
using Kb = NexGen.Generated.Kb;
using KbBlock = NexGen.Generated.Content.Block;
using KbCategory = NexGen.Generated.Tree.Category;
using KbPage = NexGen.Generated.Content.Page;
using Xunit;

namespace NexGen.DotNetExamples.Tests
{

    public class JsonSchemaKbEndpointRuntimeChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        public class KnowledgeBaseWorkflowResult
        {
            public KnowledgeBaseWorkflowResult(
                string blockId,
                string? categoryChildId,
                string pageId,
                long revision)
            {
                BlockId = blockId;
                CategoryChildId = categoryChildId;
                PageId = pageId;
                Revision = revision;
            }

            public string BlockId { get; }
            public string? CategoryChildId { get; }
            public string PageId { get; }
            public long Revision { get; }
        }

        [Workflow]
        public class KnowledgeBaseCallerWorkflow
        {
            [WorkflowRun]
            public async Task<KnowledgeBaseWorkflowResult> RunAsync()
            {
                var service = new Kb.KnowledgeBaseServiceClient("knowledge-base");
                var page = await service.GetPageAsync(new Kb.GetPageInput("page-1"));
                var block = page.Blocks is { Count: > 0 }
                    ? page.Blocks[0]
                    : throw new InvalidOperationException("expected page block");
                var putBlockOutput = await service.PutBlockAsync(block);
                var category = await service.GetCategoryTreeAsync(new Kb.GetCategoryTreeInput("root"));
                return new KnowledgeBaseWorkflowResult(
                    putBlockOutput.BlockId,
                    category.Children is { Count: > 0 } ? category.Children[0].Id : null,
                    page.PageId,
                    putBlockOutput.Revision);
            }
        }

        [Fact]
        public async Task GeneratedKnowledgeBaseServiceUsesAllOperations()
        {
            await using var env = await WorkflowEnvironment.StartLocalAsync(new());
            var taskQueue = Guid.NewGuid().ToString();
            var calls = new List<(string Operation, object Request)>();
            var serviceHandler = KnowledgeBaseServiceHandler(calls);
            var workerOptions = new TemporalWorkerOptions(taskQueue)
                .AddWorkflow<KnowledgeBaseCallerWorkflow>()
                .AddNexusService(serviceHandler);
            using var worker = new TemporalWorker(env.Client, workerOptions);

            var result = await worker.ExecuteAsync(async () =>
            {
                var endpoint = await env.CreateNexusEndpointAsync("knowledge-base", taskQueue);
                try
                {
                    return await env.Client.ExecuteWorkflowAsync(
                        (KnowledgeBaseCallerWorkflow workflow) => workflow.RunAsync(),
                        new WorkflowOptions(Guid.NewGuid().ToString(), taskQueue));
                }
                finally
                {
                    await env.DeleteNexusEndpointAsync(endpoint);
                }
            }, CancellationToken.None);

            Assert.Equal("block-1", result.BlockId);
            Assert.Equal("child", result.CategoryChildId);
            Assert.Equal("page-1", result.PageId);
            Assert.Equal(7, result.Revision);
            Assert.Collection(
                calls,
                call =>
                {
                    Assert.Equal("GetPage", call.Operation);
                    var request = Assert.IsType<Kb.GetPageInput>(call.Request);
                    Assert.Equal("page-1", request.PageId);
                },
                call =>
                {
                    Assert.Equal("PutBlock", call.Operation);
                    var request = Assert.IsType<KbBlock.Block>(call.Request);
                    Assert.Equal("block-1", request.BlockId);
                    Assert.NotNull(request.Style);
                    Assert.True(request.Style.Bold);
                },
                call =>
                {
                    Assert.Equal("GetCategoryTree", call.Operation);
                    var request = Assert.IsType<Kb.GetCategoryTreeInput>(call.Request);
                    Assert.Equal("root", request.RootId);
                });
        }

        private static ServiceHandlerInstance KnowledgeBaseServiceHandler(
            List<(string Operation, object Request)> calls)
        {
            var definition = new ServiceDefinition(
                "example.kb.v1.KnowledgeBaseService",
                new[]
                {
                    new OperationDefinition(
                        "GetPage",
                        typeof(Kb.GetPageInput),
                        typeof(KbPage.Page)),
                    new OperationDefinition(
                        "PutBlock",
                        typeof(KbBlock.Block),
                        typeof(Kb.PutBlockOutput)),
                    new OperationDefinition(
                        "GetCategoryTree",
                        typeof(Kb.GetCategoryTreeInput),
                        typeof(KbCategory.Category)),
                });
            var handlers = new Dictionary<string, IOperationHandler<object?, object?>>
            {
                ["GetPage"] = OperationHandler.WrapAsGenericHandler(
                    OperationHandler.Sync<Kb.GetPageInput, KbPage.Page>(
                        (_ctx, input) =>
                        {
                            calls.Add(("GetPage", input));
                            Assert.Equal("page-1", input.PageId);
                            return ReadFixture<KbPage.Page>("page.json");
                        })),
                ["PutBlock"] = OperationHandler.WrapAsGenericHandler(
                    OperationHandler.Sync<KbBlock.Block, Kb.PutBlockOutput>(
                        (_ctx, input) =>
                        {
                            calls.Add(("PutBlock", input));
                            Assert.Equal("block-1", input.BlockId);
                            Assert.NotNull(input.Style);
                            Assert.True(input.Style.Bold);
                            return ReadFixture<Kb.PutBlockOutput>("put-block-output.json");
                        })),
                ["GetCategoryTree"] = OperationHandler.WrapAsGenericHandler(
                    OperationHandler.Sync<Kb.GetCategoryTreeInput, KbCategory.Category>(
                        (_ctx, input) =>
                        {
                            calls.Add(("GetCategoryTree", input));
                            Assert.Equal("root", input.RootId);
                            return ReadFixture<KbCategory.Category>("category-tree.json");
                        })),
            };
            return new ServiceHandlerInstance(definition, handlers);
        }

        private static T ReadFixture<T>(string fixtureName)
        {
            var json = File.ReadAllText(Path.Combine(FixtureDirectory(), fixtureName));
            return JsonSerializer.Deserialize<T>(json, Options) ??
                throw new InvalidOperationException($"Failed to deserialize {fixtureName}");
        }

        private static string FixtureDirectory()
        {
            var directory = new DirectoryInfo(AppContext.BaseDirectory);
            while (directory is not null)
            {
                var candidate = Path.Combine(
                    directory.FullName,
                    "examples",
                    "wire",
                    "json_schema",
                    "kb");
                if (Directory.Exists(candidate))
                {
                    return candidate;
                }
                directory = directory.Parent;
            }
            throw new DirectoryNotFoundException("Could not find examples/wire/json_schema/kb");
        }
    }
}

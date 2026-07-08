using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using NexusRpc;
using NexusRpc.Handlers;
using Temporalio.Client;
using Temporalio.Testing;
using Temporalio.Worker;
using Temporalio.Workflows;
using UserServiceExample = NexGen.UserService;
using Xunit;

namespace NexGen.DotNetExamples.Tests
{

    public class UserServiceEndpointRuntimeChecks
    {
        [Workflow]
        public class UserServiceCallerWorkflow
        {
            [WorkflowRun]
            public async Task<UserServiceExample.User> RunAsync()
            {
                var service = new UserServiceExample.UserServiceClient("user-service");
                var user = await service.GetUserAsync(new UserServiceExample.GetUserOptions("user-123"));
                return await user.UpdateEmailAsync("new@example.com");
            }
        }

        [Fact]
        public async Task GeneratedServiceObjectUsesEndpointForOperationsAndResourceMethods()
        {
            await using var env = await WorkflowEnvironment.StartLocalAsync(new());
            var taskQueue = Guid.NewGuid().ToString();
            var calls = new List<(string Operation, object Request)>();
            var serviceHandler = UserServiceHandler(calls);
            var workerOptions = new TemporalWorkerOptions(taskQueue)
                .AddWorkflow<UserServiceCallerWorkflow>()
                .AddNexusService(serviceHandler);
            using var worker = new TemporalWorker(env.Client, workerOptions);

            var result = await worker.ExecuteAsync(async () =>
            {
                var endpoint = await env.CreateNexusEndpointAsync("user-service", taskQueue);
                try
                {
                    return await env.Client.ExecuteWorkflowAsync(
                        (UserServiceCallerWorkflow workflow) => workflow.RunAsync(),
                        new WorkflowOptions(Guid.NewGuid().ToString(), taskQueue));
                }
                finally
                {
                    await env.DeleteNexusEndpointAsync(endpoint);
                }
            }, CancellationToken.None);

            Assert.Equal("user-123", result.UserId);
            Assert.Equal("new@example.com", result.Email);
            Assert.Collection(
                calls,
                call =>
                {
                    Assert.Equal("GetUser", call.Operation);
                    var request = Assert.IsType<UserServiceExample.GetUserOptions>(call.Request);
                    Assert.Equal("user-123", request.UserId);
                },
                call =>
                {
                    Assert.Equal("UpdateEmail", call.Operation);
                    var request = Assert.IsType<UserServiceExample.UpdateEmailOptions>(call.Request);
                    Assert.Equal("user-123", request.UserId);
                    Assert.Equal("new@example.com", request.Email);
                });
        }

        private static ServiceHandlerInstance UserServiceHandler(
            List<(string Operation, object Request)> calls)
        {
            var definition = new ServiceDefinition(
                "UserService",
                new[]
                {
                    new OperationDefinition(
                        "GetUser",
                        typeof(UserServiceExample.GetUserOptions),
                        typeof(UserServiceExample.User)),
                    new OperationDefinition(
                        "UpdateEmail",
                        typeof(UserServiceExample.UpdateEmailOptions),
                        typeof(UserServiceExample.User)),
                });
            var handlers = new Dictionary<string, IOperationHandler<object?, object?>>
            {
                ["GetUser"] = OperationHandler.WrapAsGenericHandler(
                    OperationHandler.Sync<UserServiceExample.GetUserOptions, UserServiceExample.User>(
                        (_ctx, input) =>
                        {
                            calls.Add(("GetUser", input));
                            Assert.Equal("user-123", input.UserId);
                            return new UserServiceExample.User(input.UserId, "old@example.com");
                        })),
                ["UpdateEmail"] = OperationHandler.WrapAsGenericHandler(
                    OperationHandler.Sync<UserServiceExample.UpdateEmailOptions, UserServiceExample.User>(
                        (_ctx, input) =>
                        {
                            calls.Add(("UpdateEmail", input));
                            Assert.Equal("user-123", input.UserId);
                            Assert.Equal("new@example.com", input.Email);
                            return new UserServiceExample.User(input.UserId, input.Email);
                        })),
            };
            return new ServiceHandlerInstance(definition, handlers);
        }
    }
}

using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using Google.Protobuf.WellKnownTypes;
using Temporalio.Common;
using Temporalio.Converters;
using Temporalio.Workflows;
using ApiCommon = Temporalio.Api.Common.V1;
using ApiDeployment = Temporalio.Api.Deployment.V1;
using ApiTaskQueue = Temporalio.Api.TaskQueue.V1;
using ApiWorkflow = Temporalio.Api.Workflow.V1;

namespace NexusApiGen.Support;

public static class TemporalWorkflowContext
{
    public static string WorkflowNamespace() => Workflow.Info.Namespace;
}

public static class TemporalFunctionNames
{
    public static string WorkflowName(MethodInfo method)
    {
        if (method.GetCustomAttribute<WorkflowRunAttribute>() == null)
        {
            throw new ArgumentException($"{method} missing WorkflowRun attribute");
        }
        var definition = WorkflowDefinition.Create(method.ReflectedType ??
            throw new ArgumentException($"{method} has no reflected type"));
        return definition.Name ??
            throw new ArgumentException(
                $"{method} cannot be used directly since it is a dynamic workflow");
    }

    public static string SignalName(MethodInfo method)
    {
        var definition = WorkflowSignalDefinition.FromMethod(method);
        return definition.Name ??
            throw new ArgumentException(
                $"{method} cannot be used directly since it is a dynamic signal");
    }
}

public static class ProtoConverters
{
    public static TProto ToProto<TProto>(object? value)
    {
        var targetType = typeof(TProto);
        if (targetType == typeof(ApiCommon.WorkflowType))
        {
            return Cast<TProto>(new ApiCommon.WorkflowType { Name = Require<string>(value) });
        }
        if (targetType == typeof(ApiTaskQueue.TaskQueue))
        {
            return Cast<TProto>(new ApiTaskQueue.TaskQueue { Name = Require<string>(value) });
        }
        if (targetType == typeof(Duration))
        {
            return Cast<TProto>(Duration.FromTimeSpan(Require<TimeSpan>(value)));
        }
        if (targetType == typeof(ApiCommon.Payload))
        {
            return Cast<TProto>(Workflow.PayloadConverter.ToPayload(value));
        }
        if (targetType == typeof(ApiCommon.Payloads))
        {
            return Cast<TProto>(ToPayloads(Require<IEnumerable<object?>>(value)));
        }
        if (targetType == typeof(ApiCommon.RetryPolicy))
        {
            return Cast<TProto>(ToRetryPolicy(Require<Temporalio.Common.RetryPolicy>(value)));
        }
        if (targetType == typeof(ApiCommon.Memo))
        {
            return Cast<TProto>(ToMemo(Require<IReadOnlyDictionary<string, object?>>(value)));
        }
        if (targetType == typeof(ApiCommon.SearchAttributes))
        {
            return Cast<TProto>(Require<SearchAttributeCollection>(value).ToProto());
        }
        if (targetType == typeof(ApiCommon.Priority))
        {
            return Cast<TProto>(ToPriority(Require<Temporalio.Common.Priority>(value)));
        }
        if (targetType == typeof(ApiWorkflow.VersioningOverride))
        {
            return Cast<TProto>(ToVersioningOverride(Require<Temporalio.Common.VersioningOverride>(value)));
        }

        throw new NotSupportedException($"No proto converter is registered for {targetType.FullName}");
    }

    public static TValue FromProto<TProto, TValue>(TProto proto)
    {
        if (proto is null)
        {
            throw new ArgumentNullException(nameof(proto));
        }
        if (proto is ApiCommon.WorkflowType workflowType)
        {
            return Cast<TValue>(workflowType.Name);
        }
        if (proto is ApiTaskQueue.TaskQueue taskQueue)
        {
            return Cast<TValue>(taskQueue.Name);
        }
        if (proto is Duration duration)
        {
            return Cast<TValue>(duration.ToTimeSpan());
        }
        if (proto is ApiCommon.Payload payload)
        {
            return Cast<TValue>(Workflow.PayloadConverter.ToValue<object?>(payload));
        }
        if (proto is ApiCommon.Payloads payloads)
        {
            return Cast<TValue>(PayloadsToValues(payloads));
        }
        if (proto is ApiCommon.RetryPolicy retryPolicy)
        {
            return Cast<TValue>(FromRetryPolicy(retryPolicy));
        }
        if (proto is ApiCommon.Memo memo)
        {
            return Cast<TValue>(memo.Fields.ToDictionary(item => item.Key, item => Workflow.PayloadConverter.ToValue<object?>(item.Value)));
        }
        if (proto is ApiCommon.SearchAttributes searchAttributes)
        {
            return Cast<TValue>(SearchAttributeCollection.FromProto(searchAttributes));
        }
        if (proto is ApiCommon.Priority priority)
        {
            return Cast<TValue>(new Temporalio.Common.Priority(
                priority.PriorityKey == 0 ? null : priority.PriorityKey,
                string.IsNullOrEmpty(priority.FairnessKey) ? null : priority.FairnessKey,
                priority.FairnessWeight == 0f ? null : priority.FairnessWeight));
        }

        throw new NotSupportedException($"No proto converter is registered for {proto.GetType().FullName}");
    }

    private static ApiCommon.Payloads ToPayloads(IEnumerable<object?> values)
    {
        var payloads = new ApiCommon.Payloads();
        payloads.Payloads_.AddRange(Workflow.PayloadConverter.ToPayloads(values as IReadOnlyCollection<object?> ?? new List<object?>(values)));
        return payloads;
    }

    private static IReadOnlyCollection<object?> PayloadsToValues(ApiCommon.Payloads payloads) =>
        payloads.Payloads_.Select(payload => Workflow.PayloadConverter.ToValue<object?>(payload)).ToArray();

    private static ApiCommon.RetryPolicy ToRetryPolicy(Temporalio.Common.RetryPolicy policy)
    {
        var proto = new ApiCommon.RetryPolicy
        {
            InitialInterval = Duration.FromTimeSpan(policy.InitialInterval),
            BackoffCoefficient = policy.BackoffCoefficient,
            MaximumAttempts = policy.MaximumAttempts,
        };
        if (policy.MaximumInterval is { } maximumInterval)
        {
            proto.MaximumInterval = Duration.FromTimeSpan(maximumInterval);
        }
        if (policy.NonRetryableErrorTypes is { Count: > 0 } nonRetryableErrorTypes)
        {
            proto.NonRetryableErrorTypes.AddRange(nonRetryableErrorTypes);
        }
        return proto;
    }

    private static Temporalio.Common.RetryPolicy FromRetryPolicy(ApiCommon.RetryPolicy proto)
    {
        var retryPolicy = new Temporalio.Common.RetryPolicy
        {
            BackoffCoefficient = (float)proto.BackoffCoefficient,
            MaximumAttempts = proto.MaximumAttempts,
            NonRetryableErrorTypes = proto.NonRetryableErrorTypes.ToArray(),
        };
        if (proto.InitialInterval is { } initialInterval)
        {
            retryPolicy.InitialInterval = initialInterval.ToTimeSpan();
        }
        if (proto.MaximumInterval is { } maximumInterval)
        {
            retryPolicy.MaximumInterval = maximumInterval.ToTimeSpan();
        }
        return retryPolicy;
    }

    private static ApiCommon.Memo ToMemo(IReadOnlyDictionary<string, object?> memo)
    {
        var proto = new ApiCommon.Memo();
        foreach (var item in memo)
        {
            if (item.Value == null)
            {
                throw new ArgumentException($"Memo value for {item.Key} is null", nameof(memo));
            }
            proto.Fields.Add(item.Key, Workflow.PayloadConverter.ToPayload(item.Value));
        }
        return proto;
    }

    private static ApiCommon.Priority ToPriority(Temporalio.Common.Priority priority) => new()
    {
        PriorityKey = priority.PriorityKey ?? 0,
        FairnessKey = priority.FairnessKey ?? string.Empty,
        FairnessWeight = priority.FairnessWeight ?? 0f,
    };

    private static ApiWorkflow.VersioningOverride ToVersioningOverride(Temporalio.Common.VersioningOverride versioningOverride) =>
        versioningOverride switch
        {
            Temporalio.Common.VersioningOverride.Pinned pinned => new ApiWorkflow.VersioningOverride
            {
                Behavior = Temporalio.Api.Enums.V1.VersioningBehavior.Pinned,
                PinnedVersion = pinned.Version.ToCanonicalString(),
                Pinned = new ApiWorkflow.VersioningOverride.Types.PinnedOverride
                {
                    Version = new ApiDeployment.WorkerDeploymentVersion
                    {
                        DeploymentName = pinned.Version.DeploymentName,
                        BuildId = pinned.Version.BuildId,
                    },
                    Behavior = (ApiWorkflow.VersioningOverride.Types.PinnedOverrideBehavior)pinned.Behavior,
                },
            },
            Temporalio.Common.VersioningOverride.AutoUpgrade _ => new ApiWorkflow.VersioningOverride
            {
                Behavior = Temporalio.Api.Enums.V1.VersioningBehavior.AutoUpgrade,
                AutoUpgrade = true,
            },
            _ => throw new ArgumentException("Unknown versioning override type", nameof(versioningOverride)),
        };

    private static TValue Require<TValue>(object? value)
    {
        if (value is TValue typed)
        {
            return typed;
        }
        throw new ArgumentException($"Expected value of type {typeof(TValue).FullName}", nameof(value));
    }

    private static TValue Cast<TValue>(object? value) => (TValue)value!;
}

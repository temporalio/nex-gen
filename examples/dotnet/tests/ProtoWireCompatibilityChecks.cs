using System;
using System.IO;
using System.Text.Json;
using Google.Protobuf;
using NexGen.Support;
using Xunit;
using ActivityOptions = NexGen.TypeRoundtripService.ActivityOptions;
using Payload = Temporalio.Api.Common.V1.Payload;

namespace NexGen.DotNetExamples.Tests
{

    public class ProtoWireCompatibilityChecks
    {
        [Fact]
        public void ActivityOptionsIntermediateFixturesDecodeToUserType()
        {
            var converter = new TemporalIntermediatePayloadConverter();

            foreach (var fixtureName in new[]
            {
                "activity-options.python.payload.json",
                "activity-options.dotnet.payload.json",
            })
            {
                var payload = ReadPayload("type_roundtrip", fixtureName);
                Assert.Equal("json/protobuf", payload.Metadata["encoding"].ToStringUtf8());
                Assert.Equal(
                    "temporal.api.activity.v1.ActivityOptions",
                    payload.Metadata["messageType"].ToStringUtf8());
                Assert.DoesNotContain(
                    payload.Metadata,
                    item => item.Key.Contains("temporal-wire", StringComparison.Ordinal));
                AssertActivityOptionsModel(converter.ToValue(payload, typeof(ActivityOptions)));
            }
        }

        private static void AssertActivityOptionsModel(object? value)
        {
            var decoded = Assert.IsType<ActivityOptions>(value);
            Assert.Equal("demo-task-queue", decoded.TaskQueue);
            Assert.Equal(3, decoded.RetryPolicy.MaximumAttempts);
            Assert.Equal(TimeSpan.FromSeconds(7), decoded.ScheduleToCloseTimeout);
            Assert.NotNull(decoded.Priority);
            Assert.Equal(4, decoded.Priority.PriorityKey);
            Assert.Equal("tenant-a", decoded.Priority.FairnessKey);
            Assert.Equal(2.5f, decoded.Priority.FairnessWeight);
        }

        private static Payload ReadPayload(string exampleId, string fixtureName)
        {
            var json = File.ReadAllText(Path.Combine(FixtureDirectory(exampleId), fixtureName));
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            var payload = new Payload
            {
                Data = ByteString.FromBase64(root.GetProperty("data").GetString()),
            };
            foreach (var item in root.GetProperty("metadata").EnumerateObject())
            {
                payload.Metadata[item.Name] = ByteString.FromBase64(item.Value.GetString());
            }
            return payload;
        }

        private static string FixtureDirectory(string exampleId)
        {
            var directory = new DirectoryInfo(AppContext.BaseDirectory);
            while (directory is not null)
            {
                var candidate = Path.Combine(
                    directory.FullName,
                    "examples",
                    "wire",
                    "proto",
                    exampleId);
                if (Directory.Exists(candidate))
                {
                    return candidate;
                }
                directory = directory.Parent;
            }
            throw new DirectoryNotFoundException($"Could not find examples/wire/proto/{exampleId}");
        }
    }
}

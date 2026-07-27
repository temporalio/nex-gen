using System;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Text.Json.Nodes;
using Xunit;
using Chat = NexGen.ChatService;
using Kb = NexGen.Generated.Kb;
using KbBlock = NexGen.Generated.Content.Block;
using KbCategory = NexGen.Generated.Tree.Category;
using KbPage = NexGen.Generated.Content.Page;

namespace NexGen.DotNetExamples.Tests
{

    public class JsonSchemaRuntimeChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        [Fact]
        public void CanonicalWireFixturesRoundTrip()
        {
            var fixtureDirectory = FixtureDirectory("chat");
            var message = RoundTrip<Chat.Message>(fixtureDirectory, "message-minimal.json");
            Assert.Equal("text", message.Kind);
            Assert.Equal("hi", message.Body);
            Assert.Null(message.ReplyToId);
            Assert.Equal(0, message.Priority);

            var fullMessage = RoundTrip<Chat.Message>(fixtureDirectory, "message-full.json");
            Assert.Null(fullMessage.ReplyToId);
            Assert.Equal(7, fullMessage.Priority);

            var room = RoundTrip<Chat.Room>(fixtureDirectory, "room-open.json");
            Assert.Equal("r1", room.RoomId);
            Assert.Contains("x-extra", room.AdditionalProperties.Keys);

            var labels = RoundTrip<Chat.Labels>(fixtureDirectory, "labels.json");
            Assert.Contains("env", labels.AdditionalProperties.Keys);

            var request = RoundTrip<Chat.SendMessageInput>(fixtureDirectory, "send-message-input.json");
            Assert.Equal("hi", request.Message.Body);

            var response = RoundTrip<Chat.SendMessageOutput>(fixtureDirectory, "send-message-output.json");
            Assert.Equal("m1", response.MessageId);
        }

        [Fact]
        public void KnowledgeBaseWireFixturesRoundTrip()
        {
            var fixtureDirectory = FixtureDirectory("kb");

            var page = RoundTrip<KbPage.Page>(fixtureDirectory, "page.json");
            Assert.Equal("page-1", page.PageId);
            Assert.NotNull(page.Blocks);
            var pageBlock = Assert.Single(page.Blocks);
            Assert.Equal("block-1", pageBlock.BlockId);
            Assert.Null(pageBlock.Page);
            Assert.NotNull(pageBlock.Style);
            Assert.True(pageBlock.Style.Bold);

            var block = RoundTrip<KbBlock.Block>(fixtureDirectory, "block.json");
            Assert.Equal("block-1", block.BlockId);
            Assert.Null(block.Page);

            var category = RoundTrip<KbCategory.Category>(fixtureDirectory, "category-tree.json");
            Assert.NotNull(category.Children);
            Assert.Equal("child", Assert.Single(category.Children).Id);

            var request = RoundTrip<Kb.GetPageInput>(fixtureDirectory, "get-page-input.json");
            Assert.Equal("page-1", request.PageId);

            var categoryRequest = RoundTrip<Kb.GetCategoryTreeInput>(fixtureDirectory, "get-category-tree-input.json");
            Assert.Equal("root", categoryRequest.RootId);

            var response = RoundTrip<Kb.PutBlockOutput>(fixtureDirectory, "put-block-output.json");
            Assert.Equal("block-1", response.BlockId);
            Assert.Equal(7, response.Revision);
        }

        [Fact]
        public void RuntimeValidationRejectsSchemaViolations()
        {
            AssertJsonException<Chat.SendMessageInput>(
                "{\"roomId\":\"r1\",\"message\":{\"kind\":\"text\",\"body\":\"hi\"},\"extra\":true}",
                "unknown closed-object field");
            AssertJsonException<Chat.Message>(
                "{\"kind\":\"image\",\"body\":\"hi\"}",
                "const field mismatch");
            AssertJsonException<Chat.SendMessageOutput>(
                "{}",
                "missing required field");
            AssertJsonException<Chat.Labels>(
                "{\"env\":42}",
                "typed map value mismatch");
            var tooManyLabels = "{" + string.Join(
                ",",
                Enumerable.Range(0, 51).Select(index => $"\"key-{index}\":\"value\"")) + "}";
            AssertJsonException<Chat.Labels>(
                tooManyLabels,
                "typed map maxProperties");
            AssertJsonException<Chat.Room>(
                "{\"roomId\":\"r1\",\"displayName\":\"General\",\"topic\":null,\"members\":null}",
                "optional non-nullable array");
        }

        [Fact]
        public void ConstructedModelsPreserveJsonSchemaWireShape()
        {
            AssertJsonEqual(
                "{\"kind\":\"text\",\"body\":\"hello\"}",
                JsonSerializer.SerializeToNode(new Chat.Message("hello"), Options));

            AssertJsonEqual(
                "{\"kind\":\"text\",\"body\":\"hello\",\"priority\":0}",
                JsonSerializer.SerializeToNode(new Chat.Message("hello") { Priority = 0 }, Options));

            var room = new Chat.Room("room-1", "General", null);
            room.AdditionalProperties["color"] = "blue";
            AssertJsonEqual(
                "{\"roomId\":\"room-1\",\"displayName\":\"General\",\"topic\":null,\"color\":\"blue\"}",
                JsonSerializer.SerializeToNode(room, Options));

            var labels = new Chat.Labels();
            labels.AdditionalProperties["channel"] = "general";
            labels.AdditionalProperties["team"] = "support";
            AssertJsonEqual(
                "{\"channel\":\"general\",\"team\":\"support\"}",
                JsonSerializer.SerializeToNode(labels, Options));
        }

        [Fact]
        public void ConstructedModelsRejectExplicitNullForOptionalNonNullableFields()
        {
            _ = new Chat.Room("room-1", "General", null);

            Assert.Throws<JsonException>(() =>
                _ = new Chat.Room("room-1", "General", null) { Members = null });
            Assert.Throws<JsonException>(() =>
                _ = new Chat.Room("room-1", "General", null) { Labels = null });
            Assert.Throws<JsonException>(() =>
                _ = new Chat.Message("hello") { Priority = null });
        }

        [Fact]
        public void IntegerFieldsFollowJsonSchemaNumberSemantics()
        {
            var message = JsonSerializer.Deserialize<Chat.Message>(
                "{\"kind\":\"text\",\"body\":\"hello\",\"priority\":1.0}",
                Options);
            Assert.NotNull(message);
            Assert.Equal(1, message.Priority);

            AssertJsonException<Chat.Message>(
                "{\"kind\":\"text\",\"body\":\"hello\",\"priority\":true}",
                "integer bool");
            AssertJsonException<Chat.Message>(
                "{\"kind\":\"text\",\"body\":\"hello\",\"priority\":1.5}",
                "integer fractional");
            AssertJsonException<Chat.Message>(
                "{\"kind\":\"text\",\"body\":\"hello\",\"priority\":9007199254740992}",
                "integer safe range");
        }

        private static T RoundTrip<T>(string fixtureDirectory, string fixtureName)
        {
            var json = File.ReadAllText(Path.Combine(fixtureDirectory, fixtureName));
            var value = JsonSerializer.Deserialize<T>(json, Options) ??
                throw new InvalidOperationException($"Failed to deserialize {fixtureName}");
            var actual = JsonSerializer.SerializeToNode(value, Options);
            AssertJsonEqual(json, actual);
            return value;
        }

        private static void AssertJsonEqual(string expectedJson, JsonNode? actual)
        {
            var expected = JsonNode.Parse(expectedJson);
            if (!JsonNode.DeepEquals(expected, actual))
            {
                Assert.Fail(
                    $"JSON mismatch. Expected {expected?.ToJsonString()}, got {actual?.ToJsonString()}");
            }
        }

        private static void AssertJsonException<T>(string json, string label)
        {
            try
            {
                _ = JsonSerializer.Deserialize<T>(json, Options);
            }
            catch (JsonException)
            {
                return;
            }
            Assert.Fail($"Expected JSON exception for {label}");
        }

        private static string FixtureDirectory(string exampleId)
        {
            var directory = new DirectoryInfo(AppContext.BaseDirectory);
            while (directory is not null)
            {
                var candidate = Path.Combine(
                    directory.FullName,
                    "samples",
                    "wire",
                    "json_schema",
                    exampleId);
                if (Directory.Exists(candidate))
                {
                    return candidate;
                }
                directory = directory.Parent;
            }
            throw new DirectoryNotFoundException($"Could not find samples/wire/json_schema/{exampleId}");
        }
    }
}

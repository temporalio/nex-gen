using System.Text.Json;
using Xunit;
using Showcase = NexGen.ShowcaseService;

namespace NexGen.DotNetExamples.Tests
{

    /// <summary>
    /// Covers the <c>Shape</c> tagged union — <c>Circle | Square</c>, selected by the
    /// shared required <c>kind</c> const.
    ///
    /// This is the construct that previously generated a class with no members at
    /// all: both branches were dropped and any payload round-tripped as an empty
    /// object.
    /// </summary>
    public class TaggedUnionChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        private static Showcase.Shape Deserialize(string json) =>
            JsonSerializer.Deserialize<Showcase.Shape>(json, Options)!;

        private static Showcase.ValidationException Rejects(string json) =>
            Assert.Throws<Showcase.ValidationException>(() => Deserialize(json));

        [Fact]
        public void CircleTagSelectsTheCircleBranch()
        {
            var shape = Deserialize(@"{""kind"":""circle"",""radius"":2.5}");

            var circle = Assert.IsType<Showcase.Circle>(shape);
            Assert.Equal("circle", circle.Kind);
            Assert.Equal(2.5, circle.Radius);
        }

        [Fact]
        public void SquareTagSelectsTheSquareBranch()
        {
            var shape = Deserialize(@"{""kind"":""square"",""side"":4}");

            var square = Assert.IsType<Showcase.Square>(shape);
            Assert.Equal(4, square.Side);
        }

        /// <summary>
        /// The branches are reachable through the base type, which is the whole
        /// point — a member typed <c>Shape</c> can hold either.
        /// </summary>
        [Fact]
        public void BranchesShareTheAbstractBaseType()
        {
            Assert.IsAssignableFrom<Showcase.Shape>(Deserialize(@"{""kind"":""circle"",""radius"":1}"));
            Assert.IsAssignableFrom<Showcase.Shape>(Deserialize(@"{""kind"":""square"",""side"":1}"));
        }

        [Fact]
        public void UnknownTagIsRejected()
        {
            var exception = Rejects(@"{""kind"":""triangle"",""side"":1}");

            var violation = Assert.Single(exception.Violations);
            Assert.Equal(
                @"unknown discriminator kind ""triangle"": expected one of [""circle"", ""square""]",
                violation.Reason);
        }

        [Fact]
        public void MissingDiscriminatorIsRejected()
        {
            var exception = Rejects(@"{""radius"":2.5}");

            Assert.Equal(@"discriminator ""kind"" is required", Assert.Single(exception.Violations).Reason);
        }

        [Fact]
        public void NonObjectIsRejected()
        {
            var exception = Rejects(@"""circle""");

            Assert.Equal("expected one of: Circle, Square", Assert.Single(exception.Violations).Reason);
        }

        /// <summary>
        /// Round-tripping through the base type writes the branch's own shape, not
        /// an empty object — the converter dispatches on the runtime type.
        /// </summary>
        [Fact]
        public void SerializingThroughTheBaseTypeWritesTheBranch()
        {
            Showcase.Shape shape = new Showcase.Circle(2.5);

            var json = JsonSerializer.Serialize(shape, Options);

            using var document = JsonDocument.Parse(json);
            Assert.Equal("circle", document.RootElement.GetProperty("kind").GetString());
            Assert.Equal(2.5, document.RootElement.GetProperty("radius").GetDouble());
        }

        [Fact]
        public void RoundTripThroughTheBaseTypePreservesTheBranch()
        {
            var original = @"{""kind"":""square"",""side"":3}";

            var reserialized = JsonSerializer.Serialize(Deserialize(original), Options);

            Assert.IsType<Showcase.Square>(Deserialize(reserialized));
        }

        /// <summary>
        /// Validate() is declared on the base, so a caller holding a Shape can check
        /// it without knowing the branch.
        /// </summary>
        [Fact]
        public void ValidateIsCallableThroughTheBaseType()
        {
            Showcase.Shape shape = new Showcase.Square(3);

            shape.Validate();
        }

        /// <summary>
        /// A branch used as a member of a containing model still selects correctly.
        /// </summary>
        [Fact]
        public void UnionNestedInAContainingModelSelectsTheBranch()
        {
            var json = @"{""kind"":""showcase"",""revision"":1,""enabled"":true,"
                + @"""status"":""active"",""tier"":1,""scale"":1.5,""name"":""Widget"","
                + @"""count"":3,""active"":true,""category"":""tools"","
                + @"""shape"":{""kind"":""circle"",""radius"":7}}";

            var value = JsonSerializer.Deserialize<Showcase.Showcase>(json, Options)!;

            var circle = Assert.IsType<Showcase.Circle>(value.Shape);
            Assert.Equal(7, circle.Radius);
        }
    }
}

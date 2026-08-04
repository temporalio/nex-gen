using System.Text.Json;
using Xunit;
using Chat = NexGen.ChatService;
using Showcase = NexGen.ShowcaseService;

namespace NexGen.DotNetExamples.Tests
{

    /// <summary>
    /// Covers the object-level assertions — <c>minProperties</c>,
    /// <c>maxProperties</c>, <c>propertyNames</c> and <c>dependentRequired</c> —
    /// which are checked against the wire member set rather than any one member's
    /// value.
    ///
    /// Two object shapes matter here and are covered separately: showcase's
    /// <c>Attributes</c> is map-shaped, so its extension bag holds every member,
    /// while <c>Contact</c> declares properties.
    /// </summary>
    public class ObjectConstraintChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        private static T Deserialize<T>(string json) =>
            JsonSerializer.Deserialize<T>(json, Options)!;

        private static Showcase.ValidationException RejectsShowcase<T>(string json) =>
            Assert.Throws<Showcase.ValidationException>(() => Deserialize<T>(json));

        [Fact]
        public void MapWithinPropertyCountBoundsIsAccepted()
        {
            var value = Deserialize<Showcase.Attributes>(@"{""a"":""1"",""b"":""2""}");

            Assert.Equal(2, value.AdditionalProperties.Count);
        }

        /// <summary>
        /// Object-level violations carry the containing path with no member segment
        /// appended — the failure belongs to the object, not a member. Matches Go,
        /// which reports these with an empty path.
        /// </summary>
        [Fact]
        public void EmptyMapViolatesMinProperties()
        {
            var exception = RejectsShowcase<Showcase.Attributes>("{}");

            var violation = Assert.Single(exception.Violations);
            Assert.Equal(string.Empty, violation.Path);
            Assert.Equal("must have at least 1 properties, got 0", violation.Reason);
        }

        [Fact]
        public void OversizedMapViolatesMaxProperties()
        {
            var exception = RejectsShowcase<Showcase.Attributes>(
                @"{""a"":""1"",""b"":""2"",""c"":""3"",""d"":""4""}");

            var violation = Assert.Single(exception.Violations);
            Assert.Equal(string.Empty, violation.Path);
            Assert.Equal("must have at most 3 properties, got 4", violation.Reason);
        }

        /// <summary>
        /// <c>propertyNames</c> length is a code-point count, and the reason names
        /// the offending key even though the path carries it too — that duplication
        /// is what keeps the text identical to Go's `invalid property name %q`.
        /// </summary>
        [Fact]
        public void OverlongPropertyNameIsRejected()
        {
            var exception = RejectsShowcase<Showcase.Attributes>(@"{""aVeryLongKey"":""1""}");

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("aVeryLongKey", violation.Path);
            Assert.Equal(
                @"invalid property name ""aVeryLongKey"": must have length <= 8, got 12",
                violation.Reason);
        }

        [Fact]
        public void PropertyNameAtTheLengthBoundIsAccepted()
        {
            var value = Deserialize<Showcase.Attributes>(@"{""12345678"":""1""}");

            Assert.Contains("12345678", value.AdditionalProperties.Keys);
        }

        [Fact]
        public void DeclaredPropertyObjectWithinBoundsIsAccepted()
        {
            var value = Deserialize<Showcase.Contact>(@"{""email"":""a@b.c""}");

            Assert.Equal("a@b.c", value.Email);
        }

        [Fact]
        public void EmptyDeclaredPropertyObjectViolatesMinProperties()
        {
            var exception = RejectsShowcase<Showcase.Contact>("{}");

            Assert.Equal("must have at least 1 properties, got 0", Assert.Single(exception.Violations).Reason);
        }

        /// <summary>
        /// The whole point of <c>dependentRequired</c>: a shipping street obliges a
        /// shipping zip.
        /// </summary>
        [Fact]
        public void DependentRequiredIsViolatedWhenTheTriggerIsPresentAlone()
        {
            var exception = RejectsShowcase<Showcase.Contact>(@"{""shippingStreet"":""1 Main St""}");

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("shippingZip", violation.Path);
            Assert.Equal(
                @"property ""shippingZip"" is required when ""shippingStreet"" is present",
                violation.Reason);
        }

        [Fact]
        public void DependentRequiredIsSatisfiedWhenBothArePresent()
        {
            var value = Deserialize<Showcase.Contact>(
                @"{""shippingStreet"":""1 Main St"",""shippingZip"":""12345""}");

            Assert.Equal("12345", value.ShippingZip);
        }

        /// <summary>
        /// Absent trigger, absent dependent — not a violation.
        /// </summary>
        [Fact]
        public void DependentRequiredDoesNotFireWhenTheTriggerIsAbsent()
        {
            var value = Deserialize<Showcase.Contact>(@"{""shippingZip"":""12345""}");

            Assert.Null(value.ShippingStreet);
        }

        /// <summary>
        /// chat's <c>Labels</c> declares <c>maxProperties: 50</c>. This used to throw a
        /// bare <c>JsonException("maxProperties: at most 50 entries")</c>; it now
        /// aggregates like every other constraint and reads the same as Go.
        ///
        /// Note the exception type is <c>Chat.ValidationException</c>, not
        /// showcase's: the shared runtime is emitted per package, so each generated
        /// package carries its own. Go, Python and Java all work the same way.
        /// </summary>
        [Fact]
        public void MaxPropertiesOnATypedMapNowAggregates()
        {
            var members = new string[51];
            for (var index = 0; index < members.Length; index++)
            {
                members[index] = $@"""k{index}"":""v""";
            }
            var json = "{" + string.Join(",", members) + "}";

            var exception = Assert.Throws<Chat.ValidationException>(
                () => Deserialize<Chat.Labels>(json));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal(string.Empty, violation.Path);
            Assert.Equal("must have at most 50 properties, got 51", violation.Reason);
        }

        [Fact]
        public void ObjectAndMemberViolationsAreReportedTogether()
        {
            // Four members (over maxProperties: 3) and one key too long.
            var exception = RejectsShowcase<Showcase.Attributes>(
                @"{""a"":""1"",""b"":""2"",""c"":""3"",""aVeryLongKey"":""4""}");

            Assert.Equal(2, exception.Violations.Count);
            Assert.Contains("must have at most 3 properties, got 4", exception.Message);
            Assert.Contains(@"invalid property name ""aVeryLongKey""", exception.Message);
        }
    }
}

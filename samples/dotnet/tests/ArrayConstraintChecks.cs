using System.Text.Json;
using Xunit;
using Showcase = NexGen.ShowcaseService;

namespace NexGen.DotNetExamples.Tests
{

    /// <summary>
    /// Covers <c>minItems</c>, <c>maxItems</c>, <c>uniqueItems</c> and the
    /// <c>contains</c>/<c>minContains</c>/<c>maxContains</c> occurrence window,
    /// using showcase's <c>tags</c>, <c>aliases</c> and <c>roles</c> members.
    /// </summary>
    public class ArrayConstraintChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        private const string RequiredMembers =
            @"""kind"":""showcase"",""revision"":1,""enabled"":true,"
                + @"""status"":""active"",""tier"":1,""scale"":1.5,""name"":""Widget"","
                + @"""count"":3,""active"":true,""category"":""tools""";

        private static string Payload(string extraMembers) =>
            "{" + RequiredMembers + "," + extraMembers + "}";

        private static Showcase.Showcase Deserialize(string json) =>
            JsonSerializer.Deserialize<Showcase.Showcase>(json, Options)!;

        private static Showcase.ValidationException Rejects(string json) =>
            Assert.Throws<Showcase.ValidationException>(() => Deserialize(json));

        [Fact]
        public void ArrayWithinBoundsIsAccepted()
        {
            var value = Deserialize(Payload(@"""tags"":[""a"",""b""]"));

            Assert.Equal(2, value.Tags!.Count);
        }

        [Fact]
        public void EmptyArrayBelowMinItemsIsRejected()
        {
            var exception = Rejects(Payload(@"""tags"":[]"));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("tags", violation.Path);
            Assert.Equal("must have at least 1 items, got 0", violation.Reason);
        }

        [Fact]
        public void ArrayAboveMaxItemsIsRejected()
        {
            var exception = Rejects(Payload(@"""tags"":[""a"",""b"",""c"",""d"",""e"",""f""]"));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("tags", violation.Path);
            Assert.Equal("must have at most 5 items, got 6", violation.Reason);
        }

        [Fact]
        public void DistinctItemsSatisfyUniqueItems()
        {
            var value = Deserialize(Payload(@"""aliases"":[""a"",""b"",""c""]"));

            Assert.Equal(3, value.Aliases!.Count);
        }

        [Fact]
        public void DuplicateItemReportsBothIndexes()
        {
            var exception = Rejects(Payload(@"""aliases"":[""a"",""b"",""a""]"));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("aliases", violation.Path);
            Assert.Equal("duplicate items: element at index 2 equals index 0", violation.Reason);
        }

        /// <summary>
        /// A value repeated three times yields one violation per later occurrence,
        /// each against the first sighting — not one per pair. Matches Go.
        /// </summary>
        [Fact]
        public void EveryDuplicateOccurrenceIsReportedAgainstTheFirstSighting()
        {
            var exception = Rejects(Payload(@"""aliases"":[""a"",""a"",""a""]"));

            Assert.Equal(2, exception.Violations.Count);
            Assert.Equal("duplicate items: element at index 1 equals index 0", exception.Violations[0].Reason);
            Assert.Equal("duplicate items: element at index 2 equals index 0", exception.Violations[1].Reason);
        }

        [Fact]
        public void ContainsWindowIsSatisfiedByOneMatch()
        {
            var value = Deserialize(Payload(@"""roles"":[""admin"",""user""]"));

            Assert.Equal(2, value.Roles!.Count);
        }

        [Fact]
        public void NoMatchingItemViolatesMinContains()
        {
            var exception = Rejects(Payload(@"""roles"":[""user"",""guest""]"));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("roles", violation.Path);
            Assert.Equal("too few matching items: at least 1, got 0", violation.Reason);
        }

        [Fact]
        public void TooManyMatchingItemsViolatesMaxContains()
        {
            var exception = Rejects(Payload(@"""roles"":[""admin"",""admin"",""admin""]"));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("roles", violation.Path);
            Assert.Equal("too many matching items: at most 2, got 3", violation.Reason);
        }

        /// <summary>
        /// <c>maxContains</c> counts only matching elements, so a long array with two
        /// matches is fine.
        /// </summary>
        [Fact]
        public void NonMatchingItemsDoNotCountTowardTheWindow()
        {
            var value = Deserialize(Payload(@"""roles"":[""admin"",""a"",""b"",""admin""]"));

            Assert.Equal(4, value.Roles!.Count);
        }

        /// <summary>
        /// An absent optional array is not a violation, even though `tags` declares
        /// `minItems: 1` — the bound applies to a present value.
        /// </summary>
        [Fact]
        public void AbsentOptionalArrayIsNotAViolation()
        {
            var value = Deserialize("{" + RequiredMembers + "}");

            Assert.Null(value.Tags);
        }

        [Fact]
        public void ViolationsAcrossSeveralArraysAreReportedTogether()
        {
            var exception = Rejects(Payload(
                @"""tags"":[],""aliases"":[""a"",""a""],""roles"":[""user""]"));

            Assert.Equal(3, exception.Violations.Count);
            Assert.Contains("tags: must have at least 1 items, got 0", exception.Message);
            Assert.Contains("aliases: duplicate items: element at index 1 equals index 0", exception.Message);
            Assert.Contains("roles: too few matching items: at least 1, got 0", exception.Message);
        }
    }
}

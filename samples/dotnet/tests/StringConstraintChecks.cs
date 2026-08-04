using System.Text.Json;
using Xunit;
using Showcase = NexGen.ShowcaseService;

namespace NexGen.DotNetExamples.Tests
{

    /// <summary>
    /// Covers <c>minLength</c>, <c>maxLength</c> and <c>pattern</c> on the
    /// showcase schema, which is the only input exercising them.
    ///
    /// Two of these guard hazards specific to .NET: <c>Regex</c>'s <c>$</c> anchor
    /// also matches before a trailing newline, and <c>string.Length</c> counts
    /// UTF-16 units rather than code points.
    /// </summary>
    public class StringConstraintChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        /// <summary>
        /// Showcase's required members, all conforming — the same shape as the
        /// checked-in <c>showcase-minimal.json</c> fixture. Cases below add or
        /// override members on top of it.
        /// </summary>
        private const string RequiredMembers =
            @"""kind"":""showcase"",""revision"":1,""enabled"":true,"
                + @"""status"":""active"",""tier"":1,""scale"":1.5,"
                + @"""count"":3,""active"":true,""category"":""tools""";

        private static string Payload(string extraMembers = "", string name = "Widget") =>
            "{" + RequiredMembers + @",""name"":""" + name + @""""
                + (extraMembers.Length == 0 ? "" : "," + extraMembers)
                + "}";

        private static Showcase.Showcase Deserialize(string json) =>
            JsonSerializer.Deserialize<Showcase.Showcase>(json, Options)!;

        private static Showcase.ValidationException Rejects(string json) =>
            Assert.Throws<Showcase.ValidationException>(() => Deserialize(json));

        [Fact]
        public void ConformingPayloadIsAccepted()
        {
            var value = Deserialize(Payload());

            Assert.Equal("Widget", value.Name);
        }

        [Fact]
        public void StringShorterThanMinLengthIsRejected()
        {
            var exception = Rejects(Payload(name: ""));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("name", violation.Path);
            Assert.Equal("must have length >= 1, got 0", violation.Reason);
        }

        [Fact]
        public void StringLongerThanMaxLengthIsRejected()
        {
            var exception = Rejects(Payload(@"""nickname"":""0123456789abc"""));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("nickname", violation.Path);
            Assert.Equal("must have length <= 12, got 13", violation.Reason);
        }

        /// <summary>
        /// <c>maxLength</c> counts code points. An astral character is one code
        /// point but two UTF-16 units, so a naive <c>string.Length</c> check would
        /// reject 12 emoji against <c>maxLength: 12</c>. Go counts runes and Java
        /// counts code points, so accepting this is what keeps .NET in step.
        /// </summary>
        [Fact]
        public void AstralCharactersCountAsOneCodePointEach()
        {
            // 12 U+1F600 characters: 12 code points, 24 UTF-16 units.
            var nickname = string.Concat(System.Linq.Enumerable.Repeat("\U0001F600", 12));
            Assert.Equal(24, nickname.Length);

            var value = Deserialize(Payload(@"""nickname"":""" + nickname + @""""));

            Assert.Equal(nickname, value.Nickname);
        }

        [Fact]
        public void ThirteenAstralCharactersStillExceedMaxLength()
        {
            var nickname = string.Concat(System.Linq.Enumerable.Repeat("\U0001F600", 13));

            var exception = Rejects(Payload(@"""nickname"":""" + nickname + @""""));

            Assert.Equal("must have length <= 12, got 13", Assert.Single(exception.Violations).Reason);
        }

        [Fact]
        public void ValueMatchingPatternIsAccepted()
        {
            var value = Deserialize(Payload(@"""sku"":""ABCD"""));

            Assert.Equal("ABCD", value.Sku);
        }

        [Fact]
        public void ValueViolatingPatternIsRejected()
        {
            var exception = Rejects(Payload(@"""sku"":""abcd"""));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("sku", violation.Path);
            Assert.Equal(@"must match pattern ^[A-Z]{2,4}\z, got abcd", violation.Reason);
        }

        /// <summary>
        /// The reason the emitted pattern ends in <c>\z</c> rather than <c>$</c>.
        ///
        /// .NET's <c>Regex</c> treats <c>$</c> as "end of input, or immediately
        /// before a final newline", so <c>^[A-Z]{2,4}$</c> would happily match
        /// <c>"ABCD\n"</c> — a value the contract forbids and that Go rejects.
        /// </summary>
        [Fact]
        public void TrailingNewlineDoesNotSatisfyTheEndAnchor()
        {
            var exception = Rejects(Payload(@"""sku"":""ABCD\n"""));

            Assert.Equal("sku", Assert.Single(exception.Violations).Path);
        }

        /// <summary>
        /// The loader normalizes Perl <c>\s</c>/<c>\S</c> to explicit ASCII
        /// classes, so a non-ASCII space such as U+00A0 must not satisfy the
        /// separator in showcase's two-word phrase pattern.
        /// </summary>
        [Fact]
        public void NonAsciiSpaceDoesNotSatisfyNormalizedWhitespaceClass()
        {
            // Written as an escape rather than a literal so the character
            // cannot be silently normalized to an ASCII space by an editor.
            var phrase = "one\u00A0two";
            var exception = Rejects(Payload(@"""phrase"":""" + phrase + @""""));

            Assert.Equal("phrase", Assert.Single(exception.Violations).Path);
        }

        [Fact]
        public void AsciiSpaceSatisfiesNormalizedWhitespaceClass()
        {
            var value = Deserialize(Payload(@"""phrase"":""one two"""));

            Assert.Equal("one two", value.Phrase);
        }

        /// <summary>
        /// P11 across constraint families: a length failure and a pattern failure
        /// in one payload must both be reported.
        /// </summary>
        [Fact]
        public void LengthAndPatternViolationsAreReportedTogether()
        {
            var exception = Rejects(Payload(@"""sku"":""abcd""", name: ""));

            Assert.Equal(2, exception.Violations.Count);
            Assert.Contains("name: must have length >= 1, got 0", exception.Message);
            Assert.Contains(@"sku: must match pattern ^[A-Z]{2,4}\z, got abcd", exception.Message);
            Assert.StartsWith("2 validation error(s): ", exception.Message);
        }
    }
}

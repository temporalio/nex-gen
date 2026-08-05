using System.Text.Json;
using Xunit;
using Showcase = NexGen.ShowcaseService;

namespace NexGen.DotNetExamples.Tests
{

    /// <summary>
    /// Covers the 2^53-1 spec integer cap.
    ///
    /// Exceeding it is a contract violation, not a parse failure: it aggregates
    /// with every other violation and names the offending member, matching Go's
    /// `exceeds ±(2^53-1) integer cap`. It previously surfaced as a bare
    /// <c>JsonException("expected integer")</c> with no path.
    /// </summary>
    public class SpecIntegerChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        private const string RequiredMembers =
            @"""kind"":""showcase"",""revision"":1,""enabled"":true,"
                + @"""status"":""active"",""tier"":1,""scale"":1.5,""name"":""Widget"","
                + @"""active"":true,""category"":""tools""";

        private static string PayloadWithCount(string count) =>
            "{" + RequiredMembers + @",""count"":" + count + "}";

        private static Showcase.Showcase Deserialize(string json) =>
            JsonSerializer.Deserialize<Showcase.Showcase>(json, Options)!;

        [Fact]
        public void ValueAtTheCapIsAccepted()
        {
            var value = Deserialize(PayloadWithCount("9007199254740991"));

            Assert.Equal(9007199254740991L, value.Count);
        }

        [Fact]
        public void ValueAboveTheCapIsReportedWithItsPath()
        {
            var exception = Assert.Throws<Showcase.ValidationException>(
                () => Deserialize(PayloadWithCount("9007199254740992")));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("count", violation.Path);
            Assert.Equal("exceeds ±(2^53-1) integer cap", violation.Reason);
        }

        [Fact]
        public void ValueBelowTheNegativeCapIsReportedWithItsPath()
        {
            var exception = Assert.Throws<Showcase.ValidationException>(
                () => Deserialize(PayloadWithCount("-9007199254740992")));

            Assert.Equal("exceeds ±(2^53-1) integer cap", Assert.Single(exception.Violations).Reason);
        }

        /// <summary>
        /// A non-integral number is a *type* error, not a constraint violation, so it
        /// still surfaces as a plain <see cref="JsonException"/>.
        /// </summary>
        [Fact]
        public void NonIntegralNumberRemainsATypeError()
        {
            var exception = Record.Exception(() => Deserialize(PayloadWithCount("1.5")));

            Assert.IsAssignableFrom<JsonException>(exception);
            Assert.IsNotType<Showcase.ValidationException>(exception);
        }

        /// <summary>
        /// The cap aggregates with ordinary bound violations rather than
        /// short-circuiting them, which is the whole point of routing it through the
        /// validator instead of the read path.
        /// </summary>
        [Fact]
        public void CapAggregatesWithOtherViolations()
        {
            var json = "{" + RequiredMembers
                + @",""count"":9007199254740992,""priority"":99}";

            var exception = Assert.Throws<Showcase.ValidationException>(() => Deserialize(json));

            Assert.Equal(2, exception.Violations.Count);
            Assert.Contains("count: exceeds ±(2^53-1) integer cap", exception.Message);
            Assert.Contains("priority: must be <= 10, got 99", exception.Message);
        }

        /// <summary>
        /// <c>tier</c> is `enum: [1,2,3]` and <c>revision</c> is `const: 1`. A closed
        /// value set already bounds the value, so no cap check is emitted for them —
        /// the same members Go skips.
        /// </summary>
        [Fact]
        public void ClosedValueSetMembersCarryNoCapCheck()
        {
            var models = System.IO.File.ReadAllText(
                System.IO.Path.Combine(RepositoryRoot(), "samples/dotnet/showcase/Models.cs"));

            // The cap-checked set is exactly Go's: count, fontSize, level, priority,
            // retries, size, step, zip.
            Assert.Equal(8, System.Text.RegularExpressions.Regex.Matches(
                models, @"exceeds ±\(2\^53-1\) integer cap").Count);
            Assert.DoesNotContain("Tier < -JsonRuntime.IntegerCap", models);
            Assert.DoesNotContain("Revision < -JsonRuntime.IntegerCap", models);
        }

        private static string RepositoryRoot()
        {
            var directory = System.AppContext.BaseDirectory;
            while (directory is not null
                && !System.IO.Directory.Exists(System.IO.Path.Combine(directory, "samples")))
            {
                directory = System.IO.Path.GetDirectoryName(directory);
            }
            Assert.NotNull(directory);
            return directory!;
        }
    }
}

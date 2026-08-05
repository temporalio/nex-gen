using System.Text.Json;
using Xunit;
using Showcase = NexGen.ShowcaseService;

namespace NexGen.DotNetExamples.Tests
{

    /// <summary>
    /// Covers <c>enum</c> closed value sets over the three scalar shapes showcase
    /// declares: string (<c>status</c>), integer (<c>tier</c>) and number
    /// (<c>scale</c>).
    ///
    /// Membership is validated rather than modeled as a C# <c>enum</c> type. The
    /// wire value stays the member's type, so a value outside the set is a
    /// <see cref="Showcase.ValidationException"/> rather than a parse failure, and
    /// the reason names the admitted set exactly as Go does.
    /// </summary>
    public class EnumConstraintChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        private static string Payload(
            string status = "active", string tier = "1", string scale = "1.5") =>
            @"{""kind"":""showcase"",""revision"":1,""enabled"":true,"
                + @"""status"":""" + status + @""",""tier"":" + tier
                + @",""scale"":" + scale
                + @",""name"":""Widget"",""count"":3,""active"":true,""category"":""tools""}";

        private static Showcase.Showcase Deserialize(string json) =>
            JsonSerializer.Deserialize<Showcase.Showcase>(json, Options)!;

        private static Showcase.ValidationException Rejects(string json) =>
            Assert.Throws<Showcase.ValidationException>(() => Deserialize(json));

        [Theory]
        [InlineData("active")]
        [InlineData("inactive")]
        [InlineData("pending")]
        public void EveryAdmittedStringValueIsAccepted(string status)
        {
            var value = Deserialize(Payload(status: status));

            Assert.Equal(status, value.Status);
        }

        [Fact]
        public void StringOutsideTheSetIsRejected()
        {
            var exception = Rejects(Payload(status: "retired"));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("status", violation.Path);
            Assert.Equal(
                @"must be one of [""active"",""inactive"",""pending""], got ""retired""",
                violation.Reason);
        }

        /// <summary>
        /// Membership is case-sensitive — the wire value is compared verbatim.
        /// </summary>
        [Fact]
        public void StringMembershipIsCaseSensitive()
        {
            var exception = Rejects(Payload(status: "Active"));

            Assert.Equal("status", Assert.Single(exception.Violations).Path);
        }

        [Theory]
        [InlineData("1")]
        [InlineData("2")]
        [InlineData("3")]
        public void EveryAdmittedIntegerValueIsAccepted(string tier)
        {
            var value = Deserialize(Payload(tier: tier));

            Assert.Equal(long.Parse(tier), value.Tier);
        }

        [Fact]
        public void IntegerOutsideTheSetIsRejected()
        {
            var exception = Rejects(Payload(tier: "4"));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("tier", violation.Path);
            // Numbers are unquoted in the reason, matching Go's `%v`.
            Assert.Equal("must be one of [1,2,3], got 4", violation.Reason);
        }

        [Fact]
        public void NumberOutsideTheSetIsRejected()
        {
            var exception = Rejects(Payload(scale: "3.5"));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("scale", violation.Path);
            Assert.Equal("must be one of [1.5,2.5], got 3.5", violation.Reason);
        }

        [Fact]
        public void AdmittedNumberValueIsAccepted()
        {
            var value = Deserialize(Payload(scale: "2.5"));

            Assert.Equal(2.5, value.Scale);
        }

        /// <summary>
        /// A closed value set already bounds an integer member, so no spec integer
        /// cap check is emitted alongside it — see <see cref="SpecIntegerChecks"/>.
        /// Three enum failures at once still aggregate.
        /// </summary>
        [Fact]
        public void EnumViolationsAcrossMembersAggregate()
        {
            var exception = Rejects(Payload(status: "retired", tier: "9", scale: "9.5"));

            Assert.Equal(3, exception.Violations.Count);
            Assert.StartsWith("3 validation error(s): ", exception.Message);
            Assert.Contains(@"status: must be one of [""active"",""inactive"",""pending""], got ""retired""", exception.Message);
            Assert.Contains("tier: must be one of [1,2,3], got 9", exception.Message);
            Assert.Contains("scale: must be one of [1.5,2.5], got 9.5", exception.Message);
        }
    }
}

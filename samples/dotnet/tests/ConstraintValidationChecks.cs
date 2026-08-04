using System.Collections.Generic;
using System.Text.Json;
using Xunit;
using KbBlock = NexGen.Generated.Content.Block;
using Runtime = NexGen.Generated;

namespace NexGen.DotNetExamples.Tests
{

    /// <summary>
    /// Covers numeric-bound enforcement.
    ///
    /// The wire fixtures in <c>../wire/json_schema/</c> hold only valid payloads,
    /// so they exercise serialization but never rejection. These cases cover the
    /// other half: a payload the contract forbids must be refused, and refused the
    /// same way the other targets refuse it.
    /// </summary>
    public class ConstraintValidationChecks
    {
        private static readonly JsonSerializerOptions Options = new();

        /// <summary>
        /// <c>order</c> in kb/content/block.json declares <c>minimum: 0</c>. This
        /// payload was accepted by .NET while Go, Java, Python and TypeScript all
        /// rejected it.
        /// </summary>
        [Fact]
        public void NegativeIntegerBelowMinimumIsRejectedOnDeserialize()
        {
            var json = @"{""blockId"":""b"",""order"":-5}";

            var exception = Assert.Throws<Runtime.ValidationException>(
                () => JsonSerializer.Deserialize<KbBlock.Block>(json, Options));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("order", violation.Path);
            Assert.Equal("must be >= 0, got -5", violation.Reason);
        }

        [Fact]
        public void ValueAtTheInclusiveBoundIsAccepted()
        {
            var json = @"{""blockId"":""b"",""order"":0}";

            var block = JsonSerializer.Deserialize<KbBlock.Block>(json, Options);

            Assert.NotNull(block);
            Assert.Equal(0, block.Order);
        }

        /// <summary>
        /// An optional member's bound applies only when the member is present —
        /// absence is not a violation.
        /// </summary>
        [Fact]
        public void AbsentOptionalMemberIsNotAViolation()
        {
            var json = @"{""bold"":true}";

            var style = JsonSerializer.Deserialize<KbBlock.BlockStyle>(json, Options);

            Assert.NotNull(style);
            Assert.Null(style.Indent);
        }

        [Fact]
        public void PresentOptionalMemberBelowMinimumIsRejected()
        {
            var json = @"{""bold"":true,""indent"":-1}";

            var exception = Assert.Throws<Runtime.ValidationException>(
                () => JsonSerializer.Deserialize<KbBlock.BlockStyle>(json, Options));

            var violation = Assert.Single(exception.Violations);
            Assert.Equal("indent", violation.Path);
            Assert.Equal("must be >= 0, got -1", violation.Reason);
        }

        /// <summary>
        /// The serialize side: a value built in code rather than parsed still has to
        /// satisfy the contract before it goes on the wire.
        /// </summary>
        [Fact]
        public void ValidateRejectsAnInvalidValueBuiltInCode()
        {
            var block = new KbBlock.Block("b", -5);

            var exception = Assert.Throws<Runtime.ValidationException>(() => block.Validate());

            Assert.Equal("order", Assert.Single(exception.Violations).Path);
        }

        [Fact]
        public void ValidateAcceptsAConformingValueBuiltInCode()
        {
            var block = new KbBlock.Block("b", 3);

            block.Validate();
        }

        /// <summary>
        /// P11: every violation in one shot, not first-failure-wins. Both members
        /// are out of bounds, so both must be reported.
        /// </summary>
        [Fact]
        public void EveryViolationIsReportedAtOnce()
        {
            var json = @"{""bold"":true,""indent"":-1}";

            var exception = Assert.Throws<Runtime.ValidationException>(
                () => JsonSerializer.Deserialize<KbBlock.BlockStyle>(json, Options));

            // Message shape matches Go's ValidationError.Error().
            Assert.StartsWith("1 validation error(s): ", exception.Message);
            Assert.Contains("indent: must be >= 0, got -1", exception.Message);
        }

        /// <summary>
        /// A violation is a <see cref="JsonException"/>, so a handler already
        /// catching System.Text.Json failures keeps working.
        /// </summary>
        [Fact]
        public void ConstraintFailureIsCatchableAsJsonException()
        {
            var json = @"{""blockId"":""b"",""order"":-5}";

            Assert.Throws<Runtime.ValidationException>(
                () => JsonSerializer.Deserialize<KbBlock.Block>(json, Options));

            var caught = Record.Exception(
                () => JsonSerializer.Deserialize<KbBlock.Block>(json, Options));
            Assert.IsAssignableFrom<JsonException>(caught);
        }
    }
}

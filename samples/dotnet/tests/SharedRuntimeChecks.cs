using System.Collections.Generic;
using System.Text.Json;
using Xunit;
using Runtime = NexGen.Generated;

namespace NexGen.DotNetExamples.Tests
{

    /// <summary>
    /// Covers the shared JSON-Schema runtime emitted as <c>Definitions.cs</c>.
    ///
    /// The contract these assert is P11: a payload with several constraint
    /// failures produces one error carrying every violation, never a partial
    /// first-failure. The message shape matches Go's
    /// <c>ValidationError.Error()</c> so the same payload reads the same across
    /// targets.
    /// </summary>
    public class SharedRuntimeChecks
    {
        [Fact]
        public void ViolationRendersPathAndReason()
        {
            var violation = new Runtime.Violation("order", "must be >= 0, got -5");

            Assert.Equal("order", violation.Path);
            Assert.Equal("must be >= 0, got -5", violation.Reason);
            Assert.Equal("order: must be >= 0, got -5", violation.ToString());
        }

        [Fact]
        public void ViolationOmitsEmptyPath()
        {
            var violation = new Runtime.Violation(string.Empty, "at most 50 entries");

            Assert.Equal("at most 50 entries", violation.ToString());
        }

        [Fact]
        public void ValidationExceptionSurfacesEveryViolation()
        {
            var violations = new List<Runtime.Violation>
            {
                new Runtime.Violation("name", "must be at least 2 characters, got 1"),
                new Runtime.Violation("order", "must be >= 0, got -5"),
                new Runtime.Violation("tags", "must have at most 3 items, got 4"),
            };

            var exception = new Runtime.ValidationException(violations);

            // Every violation, not just the first.
            Assert.Equal(3, exception.Violations.Count);
            Assert.Equal("order", exception.Violations[1].Path);
            Assert.Equal(
                "3 validation error(s): name: must be at least 2 characters, got 1; "
                    + "order: must be >= 0, got -5; tags: must have at most 3 items, got 4",
                exception.Message);
        }

        [Fact]
        public void ValidationExceptionIsCatchableAsJsonException()
        {
            // Handlers already catching System.Text.Json failures keep working,
            // and a Nexus handler can map the whole family to BAD_REQUEST.
            var exception = new Runtime.ValidationException(
                new List<Runtime.Violation> { new Runtime.Violation("a", "bad") });

            Assert.IsAssignableFrom<JsonException>(exception);
        }
    }
}

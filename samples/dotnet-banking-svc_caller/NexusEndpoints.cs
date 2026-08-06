namespace TemporalioSamples.BankingSvcCaller;

/// <summary>
/// Connection and routing settings. Defaults point at the Temporal Cloud
/// namespace this sample was written against; every value is overridable by
/// environment variable.
/// </summary>
public static class NexusEndpoints
{
    /// <summary>
    /// Nexus endpoint name. Must match an endpoint that already exists on the
    /// server, routed to whichever service implements the contract; see the
    /// README. This sample is a caller only — it never serves the contract, so it
    /// has no task queue of its own.
    /// </summary>
    public static string BankService =>
        Environment.GetEnvironmentVariable("NEXUS_ENDPOINT") ?? "josh-nex-gen-java";

    public static string Address =>
        Environment.GetEnvironmentVariable("TEMPORAL_ADDRESS")
            ?? "chrsmith-namespace-of-doom.a2dd6.tmprl.cloud:7233";

    /// <summary>Namespace the operations are called from.</summary>
    public static string Namespace =>
        Environment.GetEnvironmentVariable("TEMPORAL_NAMESPACE")
            ?? "chrsmith-namespace-of-doom.a2dd6";

    /// <summary>
    /// Temporal Cloud API key.
    ///
    /// Deliberately not baked into the source: this is a live credential, and a
    /// sample that carries one leaks it to everyone who clones the repo. Export
    /// <c>TEMPORAL_API_KEY</c> before running.
    /// </summary>
    public static string ApiKey =>
        Environment.GetEnvironmentVariable("TEMPORAL_API_KEY")
            ?? throw new InvalidOperationException(
                "TEMPORAL_API_KEY is not set. Export your Temporal Cloud API key:\n" +
                "  export TEMPORAL_API_KEY='<key>'");
}

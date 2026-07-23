// .NET materialization probe for the `duration` format (prospective target).
//
// Q1: no BCL type holds the full grammar.
//   - System.TimeSpan: fixed ticks; NO Y/M/W concept.
//   - System.Xml.XmlConvert.ToTimeSpan parses P... but COLLAPSES Y/M to
//     fixed spans (P1Y->365d, P1M->30d), REJECTS the week form P1W, and
//     accepts fractions/sign -> diverges from our grammar (already recorded
//     in format_duration/native_parsers_probe). It's a lossy converter, not
//     a faithful materializer.
// Q2: design C - narrowed time-only PTnHnMnS -> TimeSpan, canonical re-emit.
//     Does XmlConvert round-trip the canonical PTnHnMnS byte-equal to Go/Java/Py?
//
// Run: cd cs_full/DurRunner && dotnet run
using System.Xml;

Console.WriteLine("=== Q1: BCL cannot hold the full grammar ===");
foreach (var w in new[] {"P1Y","P1M","P4W","P1W"}) {
    try {
        var ts = XmlConvert.ToTimeSpan(w);
        Console.WriteLine($"  XmlConvert.ToTimeSpan({w,-6}) = {ts} (LOSSY/collapsed, not faithful)");
    } catch (Exception e) {
        Console.WriteLine($"  XmlConvert.ToTimeSpan({w,-6}) REJECTED ({e.GetType().Name})");
    }
}
Console.WriteLine("  => Y/M collapse to fixed spans; week form P1W rejected. Not a faithful type.\n");

Console.WriteLine("=== Q2: design C time-only -> TimeSpan -> canonical ===");
foreach (var w in new[] {"PT1H","PT30M","PT15S","PT1H30M15S","PT1H30M","PT30M15S","PT0S"}) {
    var ts = XmlConvert.ToTimeSpan(w);
    var canon = Canonical(ts);
    // XmlConvert.ToString(TimeSpan) is the BCL's own ISO emitter:
    var bcl = XmlConvert.ToString(ts);
    Console.WriteLine($"  {w,-12} -> TimeSpan {ts,-14} -> ourCanonical={canon,-12} bcl.ToString={bcl,-12} canon==input:{canon == w}");
}
Console.WriteLine("  non-canonical:");
foreach (var w in new[] {"PT90M","PT3600S","PT24H"}) {
    var ts = XmlConvert.ToTimeSpan(w);
    Console.WriteLine($"  {w,-10} -> TimeSpan {ts,-12} -> ourCanonical={Canonical(ts),-10} bcl.ToString={XmlConvert.ToString(ts)}");
}

static string Canonical(TimeSpan ts) {
    long total = (long)ts.TotalSeconds;
    long h = total / 3600, m = (total % 3600) / 60, s = total % 60;
    var sb = new System.Text.StringBuilder("PT");
    if (h != 0) sb.Append(h).Append('H');
    if (m != 0) sb.Append(m).Append('M');
    if (s != 0 || (h == 0 && m == 0)) sb.Append(s).Append('S');
    return sb.ToString();
}

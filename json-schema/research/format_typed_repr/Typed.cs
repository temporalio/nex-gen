// Probe: .NET (C#) STANDARD LIBRARY (BCL) typed reps for the 6 formats.
// BCL only (System.*). Run via: dotnet run  (see typed_cs.csproj)
// Backs features/format typed-repr research. .NET is a PROSPECTIVE target.
using System;
using System.Globalization;
using System.Net;
using System.Net.Sockets;

static void Line(string label, string s, Func<string> f) {
    try { Console.WriteLine($"  {label,-12} {("\"" + s + "\""),-42} -> {f()}"); }
    catch (Exception e) { Console.WriteLine($"  {label,-12} {("\"" + s + "\""),-42} -> ERR {e.GetType().Name}: {e.Message}"); }
}

Console.WriteLine("=== .NET BCL typed representations ===");

// ---- date-time : DateTimeOffset (preserves offset) ----
Console.WriteLine("\n[date-time] type=System.DateTimeOffset  ctor=DateTimeOffset.Parse(s, InvariantCulture, RoundtripKind)");
foreach (var s in new[]{
    "2021-02-28T23:59:60Z",            // leap second
    "2006-01-02T15:04:05Z",
    "2006-01-02T15:04:05+00:00",
    "2006-01-02T15:04:05-00:00",
    "2006-01-02T15:04:05.123456789Z",  // 9-digit
    "2006-01-02t15:04:05z",            // lowercase
    "2006-01-02T15:04:05",             // missing offset
    "2021-02-30T00:00:00Z",            // bad calendar
}) {
    Line("DateTimeOffset", s, () => {
        var d = DateTimeOffset.Parse(s, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind);
        return $"OK -> {d:o}";
    });
}

// ---- date : DateOnly (net6+) ----
Console.WriteLine("\n[date] type=System.DateOnly (net6+)  ctor=DateOnly.ParseExact(s, \"yyyy-MM-dd\")");
foreach (var s in new[]{"2020-02-29", "2021-02-29", "2021-13-01"})
    Line("DateOnly", s, () => "OK -> " + DateOnly.ParseExact(s, "yyyy-MM-dd", CultureInfo.InvariantCulture).ToString("yyyy-MM-dd"));

// ---- time : TimeOnly (net6+) -- but RFC3339 time carries an offset TimeOnly cannot hold ----
Console.WriteLine("\n[time] type=System.TimeOnly (net6+). NOTE: TimeOnly has NO offset; RFC3339 time offset would be lost.");
foreach (var s in new[]{"12:00:00", "23:59:60", "12:00:00.5"})
    Line("TimeOnly", s, () => "OK -> " + TimeOnly.ParseExact(s, new[]{"HH:mm:ss","HH:mm:ss.fffffff","HH:mm:ss.f"}, CultureInfo.InvariantCulture).ToString());

// ---- uuid : System.Guid ----
Console.WriteLine("\n[uuid] type=System.Guid  ctor=Guid.Parse(s) / Guid.ParseExact(s, \"D\")");
foreach (var s in new[]{
    "f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
    "F81D4FAE-7DEC-11D0-A765-00A0C91E6BF6",
    "f81d4fae7dec11d0a76500a0c91e6bf6",      // no dashes (Parse accepts! ParseExact D rejects)
    "{f81d4fae-7dec-11d0-a765-00a0c91e6bf6}",// braces (Parse accepts!)
    "not-a-guid"}) {
    Line("Guid.Parse", s, () => "OK -> " + Guid.Parse(s).ToString());
}
Console.WriteLine("  -- Guid.ParseExact(s, \"D\") strict hyphenated form --");
foreach (var s in new[]{"f81d4fae-7dec-11d0-a765-00a0c91e6bf6", "F81D4FAE-7DEC-11D0-A765-00A0C91E6BF6",
                        "f81d4fae7dec11d0a76500a0c91e6bf6", "{f81d4fae-7dec-11d0-a765-00a0c91e6bf6}"})
    Line("ParseExact D", s, () => "OK -> " + Guid.ParseExact(s, "D").ToString());

// ---- ipv4 / ipv6 : System.Net.IPAddress ----
Console.WriteLine("\n[ipv4/ipv6] type=System.Net.IPAddress  ctor=IPAddress.Parse(s)");
foreach (var s in new[]{"192.168.0.1", "256.0.0.1", "01.2.3.4", "1.2.3",
                        "::1", "2001:db8::1", "2001:DB8::1", "::ffff:192.168.0.1", "fe80::1%12"}) {
    Line("IPAddress", s, () => {
        var a = IPAddress.Parse(s);
        return $"OK fam={a.AddressFamily} to_s={a}";
    });
}

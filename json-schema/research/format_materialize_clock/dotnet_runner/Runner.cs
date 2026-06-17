// Probe: MATERIALIZE model (B) in .NET via DateTimeOffset / DateOnly / TimeOnly.
// PROSPECTIVE. Parse each validated wire string, re-serialize via the
// GENERATOR-OWNED serializer (RFC 3339, original offset preserved with
// +00:00/-00:00 -> Z, fractional seconds at the value's own precision with
// trailing zeros trimmed) -- NO TRUNCATION beyond the type's genuine limit.
// dotnet run -- ../corpus.json
//
//   date-time -> DateTimeOffset  offset preserved; 100-ns TICK resolution
//                (7 fractional digits max -- a nanosecond input is truncated to
//                100 ns, distinct from Go/Java's 9 and Python's 6).
//   date      -> DateOnly        YYYY-MM-DD, lossless.
//   time      -> TimeOnly        offset-LESS only; TimeOnly cannot hold an
//                offset, so an offset-bearing time is UNSUPPORTED here.
using System.Globalization;
using System.Text.Json;

const string ENGINE = "dotnet";

void Emit(string id, string fmt, string canonical, string err) {
    var o = new { id, engine = ENGINE, format = fmt, canonical, err };
    Console.WriteLine(JsonSerializer.Serialize(o));
}

// ".ddd" with trailing zeros trimmed (from a count of 100-ns ticks within the
// second, 0..9_999_999), or "" when zero.
string FracTicks(long subTicks) {
    if (subTicks == 0) return "";
    return "." + subTicks.ToString("D7").TrimEnd('0');
}

string OffsetStr(TimeSpan off) {
    if (off == TimeSpan.Zero) return "Z";
    var sign = off < TimeSpan.Zero ? "-" : "+";
    off = off.Duration();
    return $"{sign}{off.Hours:D2}:{off.Minutes:D2}";
}

string CanonDateTime(string wire) {
    var dto = DateTimeOffset.Parse(wire.ToUpperInvariant(), CultureInfo.InvariantCulture,
        DateTimeStyles.RoundtripKind); // rejects :60; offset preserved
    long subTicks = dto.Ticks % TimeSpan.TicksPerSecond;
    return $"{dto.Year:D4}-{dto.Month:D2}-{dto.Day:D2}" +
           $"T{dto.Hour:D2}:{dto.Minute:D2}:{dto.Second:D2}" +
           FracTicks(subTicks) + OffsetStr(dto.Offset);
}

string CanonDate(string wire) {
    var d = DateOnly.ParseExact(wire, "yyyy-MM-dd", CultureInfo.InvariantCulture);
    return $"{d.Year:D4}-{d.Month:D2}-{d.Day:D2}";
}

string CanonTime(string wire) {
    var w = wire.ToUpperInvariant();
    if (System.Text.RegularExpressions.Regex.IsMatch(w, "(Z|[+-][0-9]{2}:[0-9]{2})$"))
        throw new NotSupportedException("TimeOnly cannot hold an offset (offset-bearing time unsupported in .NET)");
    var t = TimeOnly.Parse(w, CultureInfo.InvariantCulture); // rejects :60
    long subTicks = t.Ticks % TimeSpan.TicksPerSecond;
    return $"{t.Hour:D2}:{t.Minute:D2}:{t.Second:D2}" + FracTicks(subTicks);
}

void Run(JsonElement arr, string fmt, Func<string, string> fn) {
    foreach (var r in arr.EnumerateArray()) {
        var id = r.GetProperty("id").GetString()!;
        var wire = r.GetProperty("wire").GetString()!;
        try { Emit(id, fmt, fn(wire), ""); }
        catch (Exception e) { Emit(id, fmt, "", e.GetType().Name + ": " + e.Message); }
    }
}

var path = args[0];
using var doc = JsonDocument.Parse(File.ReadAllText(path));
var root = doc.RootElement;
Run(root.GetProperty("date-time"), "date-time", CanonDateTime);
Run(root.GetProperty("date"), "date", CanonDate);
Run(root.GetProperty("time"), "time", CanonTime);

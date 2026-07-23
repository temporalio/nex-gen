// C# runner for the JSON-Schema `format` conformance corpus (prospective target).
//
// Implements the SPEC'S PINNED CHECK: a pinned anchored regex compiled once
// (static Regex), plus the shared integer-arithmetic calendar predicate for the
// temporal formats. This is the OWNED check -- we do NOT delegate to
// DateTime.Parse / IPAddress.Parse as the source of truth. As a SECONDARY column
// we record what System native parsers accept, purely to document divergence.
//
// ANCHORING: .NET's `$` matches at end-of-input OR before a trailing `\n`, and
// `\z` is strict end-of-input. The pinned pattern uses `\A`...`\z` so all seven
// runtimes agree (no trailing-\n exception). Explicit char classes ([0-9] etc.)
// mean the Unicode-vs-ASCII `\d` distinction is irrelevant here.
//
// Emits JSON Lines to stdout:
//   {"id","engine":"dotnet","valid":bool,"native":bool}
//
// Run: dotnet run --project DotnetRunner -c Release -- ../corpus.json
using System.Globalization;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

const string OCTET = "(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])";
const string H16 = "[0-9a-fA-F]{1,4}";
string v4 = $"({OCTET}\\.{OCTET}\\.{OCTET}\\.{OCTET})";
string ls32 = $"({H16}:{H16}|{v4})";
string ipv6 = "\\A(" +
    $"({H16}:){{6}}{ls32}|" +
    $"::({H16}:){{5}}{ls32}|" +
    $"({H16})?::({H16}:){{4}}{ls32}|" +
    $"(({H16}:){{0,1}}{H16})?::({H16}:){{3}}{ls32}|" +
    $"(({H16}:){{0,2}}{H16})?::({H16}:){{2}}{ls32}|" +
    $"(({H16}:){{0,3}}{H16})?::({H16}:){ls32}|" +
    $"(({H16}:){{0,4}}{H16})?::{ls32}|" +
    $"(({H16}:){{0,5}}{H16})?::{H16}|" +
    $"(({H16}:){{0,6}}{H16})?::" +
    ")\\z";

var uuidRe = new Regex("\\A[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\\z", RegexOptions.Compiled);
var ipv4Re = new Regex($"\\A{OCTET}\\.{OCTET}\\.{OCTET}\\.{OCTET}\\z", RegexOptions.Compiled);
var ipv6Re = new Regex(ipv6, RegexOptions.Compiled);
var dateRe = new Regex("\\A([0-9]{4})-([0-9]{2})-([0-9]{2})\\z", RegexOptions.Compiled);
var timeRe = new Regex("\\A([0-9]{2}):([0-9]{2}):([0-9]{2})(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})?\\z", RegexOptions.Compiled);
var dateTimeRe = new Regex("\\A([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})\\z", RegexOptions.Compiled);

static bool IsLeap(int y) => (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;

static int DaysInMonth(int y, int m) => m switch
{
    1 or 3 or 5 or 7 or 8 or 10 or 12 => 31,
    4 or 6 or 9 or 11 => 30,
    2 => IsLeap(y) ? 29 : 28,
    _ => 0,
};

static bool ValidCalendarDate(int y, int m, int d) =>
    m >= 1 && m <= 12 && d >= 1 && d <= DaysInMonth(y, m);

static bool ValidTimeFields(int hh, int mm, int ss) =>
    hh <= 23 && mm <= 59 && ss <= 60; // :60 leap second accepted

static bool ValidOffset(string off)
{
    if (string.IsNullOrEmpty(off) || off == "Z" || off == "z") return true;
    int oh = int.Parse(off.Substring(1, 2), CultureInfo.InvariantCulture);
    int om = int.Parse(off.Substring(4, 2), CultureInfo.InvariantCulture);
    return oh <= 23 && om <= 59;
}

static int G(Match m, int i)
{
    var g = m.Groups[i];
    return g.Success && g.Value.Length > 0 ? int.Parse(g.Value, CultureInfo.InvariantCulture) : 0;
}

bool PinnedValid(string format, string v)
{
    switch (format)
    {
        case "uuid": return uuidRe.IsMatch(v);
        case "ipv4": return ipv4Re.IsMatch(v);
        case "ipv6": return ipv6Re.IsMatch(v);
        case "date":
        {
            var m = dateRe.Match(v);
            return m.Success && ValidCalendarDate(G(m, 1), G(m, 2), G(m, 3));
        }
        case "time":
        {
            var m = timeRe.Match(v);
            return m.Success && ValidTimeFields(G(m, 1), G(m, 2), G(m, 3)) && ValidOffset(m.Groups[5].Value);
        }
        case "date-time":
        {
            var m = dateTimeRe.Match(v);
            return m.Success && ValidCalendarDate(G(m, 1), G(m, 2), G(m, 3))
                && ValidTimeFields(G(m, 4), G(m, 5), G(m, 6)) && ValidOffset(m.Groups[8].Value);
        }
    }
    return false;
}

// SECONDARY: .NET native parser (documentation only).
static bool NativeValid(string format, string v)
{
    switch (format)
    {
        case "ipv4":
            return IPAddress.TryParse(v, out var a4) && a4.AddressFamily == AddressFamily.InterNetwork;
        case "ipv6":
            return IPAddress.TryParse(v, out var a6) && a6.AddressFamily == AddressFamily.InterNetworkV6;
        case "date":
            return DateTime.TryParseExact(v, "yyyy-MM-dd", CultureInfo.InvariantCulture,
                DateTimeStyles.None, out _);
        case "date-time":
            return DateTimeOffset.TryParse(v, CultureInfo.InvariantCulture,
                DateTimeStyles.None, out _);
        default:
            return false; // no simple native parser for uuid/time in this column
    }
}

string path = args.Length > 0 ? args[0] : "corpus.json";
byte[] bytes = File.ReadAllBytes(path);
using JsonDocument doc = JsonDocument.Parse(bytes);
JsonElement pairs = doc.RootElement.GetProperty("pairs");

var sb = new StringBuilder();
foreach (JsonElement p in pairs.EnumerateArray())
{
    string id = p.GetProperty("id").GetString()!;
    string format = p.GetProperty("format").GetString()!;
    string value = p.GetProperty("value").GetString()!;

    bool valid = PinnedValid(format, value);
    bool nat = NativeValid(format, value);

    sb.Append("{\"id\":").Append(JsonEncode(id))
      .Append(",\"engine\":\"dotnet\"")
      .Append(",\"valid\":").Append(valid ? "true" : "false")
      .Append(",\"native\":").Append(nat ? "true" : "false")
      .Append("}\n");
}
Console.Out.Write(sb.ToString());

static string JsonEncode(string s)
{
    var b = new StringBuilder("\"");
    foreach (char c in s)
    {
        switch (c)
        {
            case '"': b.Append("\\\""); break;
            case '\\': b.Append("\\\\"); break;
            case '\n': b.Append("\\n"); break;
            case '\r': b.Append("\\r"); break;
            case '\t': b.Append("\\t"); break;
            default:
                if (c < 0x20) b.Append("\\u").Append(((int)c).ToString("x4"));
                else b.Append(c);
                break;
        }
    }
    return b.Append('"').ToString();
}

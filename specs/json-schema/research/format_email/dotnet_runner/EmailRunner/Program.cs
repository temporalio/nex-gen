// C# runner for the pinned `email` regex conformance corpus.
//
// Mirrors the pinned runtime recipe for a prospective .NET target:
//   * unanchored search  -> Regex.IsMatch (irrelevant, regex is ^...$ anchored)
//   * ASCII \d\w\s        -> RegexOptions.ECMAScript (.NET default is Unicode;
//                            unused here, but kept for recipe fidelity)
//   * $ -> \z             -> .NET `\z` is end-of-input only; `\Z` is lenient
//                            (allows a final \n), so the gate rewrites a trailing
//                            `$` to `\z`. We apply the same.
//
// The pinned email regex uses NO bare `.`, so the .NET astral-`.` divergence
// (its `.` is a UTF-16 unit, no `u`-flag equivalent) does NOT apply here.
//
// Emits JSON Lines: {"id","engine":"dotnet","compiled":bool,"matched":bool|null}
//
// Run: dotnet run --project EmailRunner -- ../corpus.json

using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

string path = args.Length > 0 ? args[0] : "corpus.json";
byte[] bytes = File.ReadAllBytes(path);
using JsonDocument doc = JsonDocument.Parse(bytes);
string pinned = doc.RootElement.GetProperty("pinned_regex").GetString()!;
JsonElement pairs = doc.RootElement.GetProperty("pairs");

const RegexOptions PinnedOptions = RegexOptions.ECMAScript;
string pattern = NormalizeDollar(pinned);

Regex? re = null;
bool compiled;
try
{
    re = new Regex(pattern, PinnedOptions);
    compiled = true;
}
catch (Exception ex) when (ex is ArgumentException or RegexParseException)
{
    compiled = false;
}

var sb = new StringBuilder();
foreach (JsonElement p in pairs.EnumerateArray())
{
    string id = p.GetProperty("id").GetString()!;
    string instance = p.GetProperty("instance").GetString()!;
    bool? matched = compiled ? re!.IsMatch(instance) : null;

    sb.Append("{\"id\":").Append(JsonEncode(id))
      .Append(",\"engine\":\"dotnet\"")
      .Append(",\"compiled\":").Append(compiled ? "true" : "false")
      .Append(",\"matched\":").Append(matched is null ? "null" : (matched.Value ? "true" : "false"))
      .Append("}\n");
}
Console.Out.Write(sb.ToString());

static string NormalizeDollar(string pattern)
{
    if (pattern.Length == 0 || pattern[^1] != '$') return pattern;
    int backslashes = 0;
    int j = pattern.Length - 2;
    while (j >= 0 && pattern[j] == '\\') { backslashes++; j--; }
    if (backslashes % 2 == 1) return pattern; // escaped literal \$
    return pattern[..^1] + "\\z";
}

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

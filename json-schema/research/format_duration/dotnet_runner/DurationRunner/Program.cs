// C# runner for the `duration` format conformance corpus.
//
// Compiles the SINGLE generator-owned pinned regex (from corpus.json's
// `pinned_regex`) and matches each corpus value, mirroring the PINNED runtime
// semantics for a prospective .NET target:
//
//   * unanchored "search"  -> Regex.IsMatch(input, pattern, options)
//   * ASCII \d             -> RegexOptions.ECMAScript (.NET default \d is
//                             Unicode; ECMAScript restricts it to ASCII).
//   * $ normalized to \z   -> .NET's `$` matches before an optional final `\n`
//                             and `\Z` is the lenient one, so the strict
//                             end-of-input construct is `\z`. The generator
//                             emits `\z` for the trailing `$`.
//
// Emits JSON Lines to stdout:
//   {"id","engine":"dotnet","compiled":bool,"matched":bool|null}
//
// Run: dotnet run --project DurationRunner -c Release -- ../corpus.json

using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

string path = args.Length > 0 ? args[0] : "corpus.json";
byte[] bytes = File.ReadAllBytes(path);
using JsonDocument doc = JsonDocument.Parse(bytes);
string pinned = doc.RootElement.GetProperty("pinned_regex").GetString()!;
JsonElement cases = doc.RootElement.GetProperty("cases");

// trailing `$` -> `\z` (strict end-of-input; .NET `$` has a trailing-\n exception).
string emitted = pinned.EndsWith("$") ? pinned.Substring(0, pinned.Length - 1) + "\\z" : pinned;

const RegexOptions PinnedOptions = RegexOptions.ECMAScript;

bool compiled;
Regex? re = null;
try
{
    re = new Regex(emitted, PinnedOptions);
    compiled = true;
}
catch (Exception ex) when (ex is ArgumentException or RegexParseException)
{
    compiled = false;
}

var sb = new StringBuilder();
foreach (JsonElement c in cases.EnumerateArray())
{
    string id = c.GetProperty("id").GetString()!;
    string value = c.GetProperty("value").GetString()!;
    bool? matched = compiled ? re!.IsMatch(value) : null; // IsMatch = unanchored search
    sb.Append("{\"id\":").Append(JsonEncode(id))
      .Append(",\"engine\":\"dotnet\"")
      .Append(",\"compiled\":").Append(compiled ? "true" : "false")
      .Append(",\"matched\":").Append(matched is null ? "null" : (matched.Value ? "true" : "false"))
      .Append("}\n");
}
Console.Out.Write(sb.ToString());

static string JsonEncode(string s)
{
    var b = new StringBuilder("\"");
    foreach (char ch in s)
    {
        switch (ch)
        {
            case '"': b.Append("\\\""); break;
            case '\\': b.Append("\\\\"); break;
            case '\n': b.Append("\\n"); break;
            case '\r': b.Append("\\r"); break;
            case '\t': b.Append("\\t"); break;
            default:
                if (ch < 0x20) b.Append("\\u").Append(((int)ch).ToString("x4"));
                else b.Append(ch);
                break;
        }
    }
    return b.Append('"').ToString();
}

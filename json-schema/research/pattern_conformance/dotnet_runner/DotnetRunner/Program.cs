// C# runner for the JSON-Schema `pattern` cross-language conformance corpus.
//
// Mirrors the PINNED runtime semantics for a prospective .NET (C#) target as
// closely as System.Text.RegularExpressions allows:
//
//   * unanchored "search"  -> Regex.IsMatch(input, pattern, options)
//   * ASCII \d \w \s       -> RegexOptions.ECMAScript
//                             (.NET default \d\w\s are Unicode; ECMAScript
//                             restricts them to ASCII, matching Go/Py-ASCII/
//                             Java-default/JS pinning)
//   * $ normalized to      -> the load-time gate rewrites a trailing `$` anchor
//     end-of-input            to `\z` (analogous to Python `\Z` / Java `\z`).
//                             In .NET, `\z` is end-of-input-only and `\Z`
//                             matches before an optional final `\n`, so the
//                             correct end-of-input construct for .NET is `\z`.
//
// The corpus stores RAW patterns. To compare fairly against the reference
// (Go/JS, i.e. end-of-input `$` with no trailing-\n exception), this runner
// emits the match result computed on the pattern after applying the same
// `$`->`\z` normalization the gate would apply for the .NET target.
//
// Emits JSON Lines to stdout:
//   {"id","engine":"dotnet","compiled":bool,"matched":bool|null,"normalized":string}
//
// Run: dotnet run --project DotnetRunner -- ../corpus.json

using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

string path = args.Length > 0 ? args[0] : "corpus.json";
byte[] bytes = File.ReadAllBytes(path);
using JsonDocument doc = JsonDocument.Parse(bytes);
JsonElement pairs = doc.RootElement.GetProperty("pairs");

const RegexOptions PinnedOptions = RegexOptions.ECMAScript;

var sb = new StringBuilder();
foreach (JsonElement p in pairs.EnumerateArray())
{
    string id = p.GetProperty("id").GetString()!;
    string pattern = p.GetProperty("pattern").GetString()!;
    string instance = p.GetProperty("instance").GetString()!;

    string normalized = NormalizeDollar(pattern);

    bool compiled;
    bool? matched;
    try
    {
        // IsMatch performs an UNANCHORED search (confirmed empirically).
        matched = Regex.IsMatch(instance, normalized, PinnedOptions);
        compiled = true;
    }
    catch (Exception ex) when (ex is ArgumentException or RegexParseException)
    {
        compiled = false;
        matched = null;
    }

    sb.Append("{\"id\":").Append(JsonEncode(id))
      .Append(",\"engine\":\"dotnet\"")
      .Append(",\"compiled\":").Append(compiled ? "true" : "false")
      .Append(",\"matched\":").Append(matched is null ? "null" : (matched.Value ? "true" : "false"))
      .Append(",\"normalized\":").Append(JsonEncode(normalized))
      .Append("}\n");
}
Console.Out.Write(sb.ToString());

// Rewrite an UNESCAPED trailing `$` anchor to `\z` (end-of-input, no trailing
// `\n` exception) -- the .NET analogue of Python `\Z` / Java `\z`.
// Only a `$` that is not preceded by an odd number of backslashes and that sits
// at the end of the pattern is rewritten; a `$` inside a character class or
// escaped literal is left alone. This is a deliberately conservative rewrite
// sufficient for the corpus (its `$` anchors are all trailing).
static string NormalizeDollar(string pattern)
{
    if (pattern.Length == 0) return pattern;

    // Count trailing behaviour: find the last char; if it is `$` and it is a
    // real anchor (even number of preceding backslashes, not in a class), swap.
    int i = pattern.Length - 1;
    if (pattern[i] != '$') return pattern;

    // Ensure the `$` is not escaped (odd run of backslashes before it).
    int backslashes = 0;
    int j = i - 1;
    while (j >= 0 && pattern[j] == '\\') { backslashes++; j--; }
    if (backslashes % 2 == 1) return pattern; // escaped `\$` literal

    // Ensure we're not inside an unclosed character class `[...`.
    if (InsideCharClass(pattern, i)) return pattern;

    return pattern.Substring(0, i) + "\\z";
}

// Rough scan: is position `pos` inside an open `[...]` character class?
static bool InsideCharClass(string s, int pos)
{
    bool inClass = false;
    for (int k = 0; k < pos; k++)
    {
        char c = s[k];
        if (c == '\\') { k++; continue; }
        if (!inClass && c == '[') inClass = true;
        else if (inClass && c == ']') inClass = false;
    }
    return inClass;
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

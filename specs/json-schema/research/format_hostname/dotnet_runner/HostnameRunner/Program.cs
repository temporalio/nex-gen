// .NET (C#) runner for the JSON-Schema `hostname` format conformance corpus.
//
// Implements the PINNED generator-owned check for the (prospective) .NET target:
//   1. a compiled Regex, anchored with \A ... \z. In .NET \z is the STRICT
//      end-of-input anchor (\Z would allow a trailing '\n' -- the opposite
//      letter convention from Java; matches the pattern-spec .NET note). The
//      character class is explicit [A-Za-z0-9-], so RegexOptions.ECMAScript is
//      not required (no \d/\w/\s to narrow); we pass none.
//   2. a total-length guard (1..=253 CODE POINTS) OUTSIDE the regex, counted
//      with EnumerateRunes so astral input agrees with the other targets.
//   Verdict = IsMatch AND length-in-range.
//
// Emits JSON Lines: {"id","engine":"dotnet","valid","regex","len_ok"}
// Run: dotnet run --project HostnameRunner -- ../corpus.json
using System.Linq;
using System.Text.Json;
using System.Text.RegularExpressions;

const int MaxTotalLen = 253;

var hostRe = new Regex(
    @"\A[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*\z",
    RegexOptions.Compiled);

string path = args.Length > 0 ? args[0] : "../corpus.json";
using var doc = JsonDocument.Parse(File.ReadAllText(path));
var cases = doc.RootElement.GetProperty("cases");

var sb = new System.Text.StringBuilder();
foreach (var k in cases.EnumerateArray())
{
    string id = k.GetProperty("id").GetString()!;
    string instance = k.GetProperty("instance").GetString()!;

    int n = instance.EnumerateRunes().Count(); // code points
    bool lenOk = n >= 1 && n <= MaxTotalLen;
    bool regex = hostRe.IsMatch(instance);
    bool valid = regex && lenOk;

    var rec = new Dictionary<string, object>
    {
        ["id"] = id,
        ["engine"] = "dotnet",
        ["valid"] = valid,
        ["regex"] = regex,
        ["len_ok"] = lenOk,
    };
    sb.AppendLine(JsonSerializer.Serialize(rec));
}
Console.Out.Write(sb.ToString());

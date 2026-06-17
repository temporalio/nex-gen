// .NET runner for the PINNED `uri` check. Anchors the body with \A...\z (.NET
// \z = end of input, no trailing-\n exception; \Z is the lenient one).
// RegexOptions.ECMAScript makes \d\w\s ASCII (our body uses only explicit ASCII
// classes, so this is belt-and-suspenders). IsMatch with \A...\z anchors = a
// full-input check.
//
// NOTE the body uses only ASCII code points, so the .NET astral-`.` divergence
// noted in the pattern spec does NOT apply here (there is no bare `.` matching
// arbitrary chars -- every `.` in the body is inside a character class or
// escaped as \. -- so no astral rewrite is needed for this pinned regex).
//
// Emits JSON Lines: {"id","engine":"dotnet","compiled":bool,"matched":bool|null}
// Run: dotnet run --project DotnetRunner -- ../corpus.json ../pinned_body.json
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

string corpusPath = args.Length > 0 ? args[0] : "corpus.json";
string bodyPath = args.Length > 1 ? args[1] : "pinned_body.json";

byte[] corpusBytes = File.ReadAllBytes(corpusPath);
byte[] bodyBytes = File.ReadAllBytes(bodyPath);
using JsonDocument corpusDoc = JsonDocument.Parse(corpusBytes);
using JsonDocument bodyDoc = JsonDocument.Parse(bodyBytes);

string body = bodyDoc.RootElement.GetProperty("body").GetString()!;
JsonElement pairs = corpusDoc.RootElement.GetProperty("pairs");

Regex? re = null;
bool compiled = false;
try
{
    re = new Regex(@"\A" + body + @"\z", RegexOptions.ECMAScript);
    compiled = true;
}
catch (Exception ex) when (ex is ArgumentException or RegexParseException)
{
    Console.Error.WriteLine($"DOTNET COMPILE ERROR: {ex.Message}");
}

var sb = new StringBuilder();
foreach (JsonElement p in pairs.EnumerateArray())
{
    string id = p.GetProperty("id").GetString()!;
    string value = p.GetProperty("value").GetString()!;
    string matched = compiled ? (re!.IsMatch(value) ? "true" : "false") : "null";
    sb.Append("{\"id\":");
    sb.Append(JsonSerializer.Serialize(id));
    sb.Append(",\"engine\":\"dotnet\",\"compiled\":");
    sb.Append(compiled ? "true" : "false");
    sb.Append(",\"matched\":");
    sb.Append(matched);
    sb.Append("}\n");
}
Console.Out.Write(sb.ToString());

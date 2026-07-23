// .NET NATIVE URI-parser probe. Uses System.Uri, the parser a naive user
// would reach for. We report:
//   Uri.TryCreate(value, UriKind.Absolute, out _) => "valid absolute".
// System.Uri is lenient/normalizing in its own idiosyncratic way (differs from
// WHATWG URL and from RFC3986).
//
// Emits JSON Lines: {"id","engine":"dotnet-native","valid":bool,"detail":string}
// Run: dotnet run --project DotnetNative -- ../native_inputs.json
using System.Text;
using System.Text.Json;

string path = args.Length > 0 ? args[0] : "../native_inputs.json";
byte[] bytes = File.ReadAllBytes(path);
using JsonDocument doc = JsonDocument.Parse(bytes);
JsonElement inputs = doc.RootElement.GetProperty("inputs");

var sb = new StringBuilder();
foreach (JsonElement inp in inputs.EnumerateArray())
{
    string id = inp.GetProperty("id").GetString()!;
    string value = inp.GetProperty("value").GetString()!;

    bool valid;
    string detail;
    if (Uri.TryCreate(value, UriKind.Absolute, out Uri? u))
    {
        valid = true;
        detail = $"scheme={u!.Scheme} abs={u.AbsoluteUri}";
    }
    else
    {
        valid = false;
        detail = "TryCreate(Absolute) failed";
    }

    var rec = new
    {
        id,
        engine = "dotnet-native",
        valid,
        detail,
    };
    sb.Append(JsonSerializer.Serialize(rec));
    sb.Append('\n');
}
Console.Out.Write(sb.ToString());

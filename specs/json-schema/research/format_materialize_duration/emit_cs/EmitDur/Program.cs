// Emit canonical re-serialization JSON for the corpus (.NET).
// design B custom components for `full`.
// timeonly -> System.TimeSpan, then our GENERATOR-OWNED canonical (NOT
// XmlConvert.ToString, which rolls PT24H -> P1D and would break P1).
using System.Text;
using System.Text.Json;

// Look for corpus.json in the current dir or two levels up (project run).
string corpusPath = File.Exists("corpus.json") ? "corpus.json"
    : File.Exists("../../corpus.json") ? "../../corpus.json"
    : "../../../corpus.json";
string txt = File.ReadAllText(corpusPath);
using var doc = JsonDocument.Parse(txt);
var root = doc.RootElement;

var sb = new StringBuilder("{");
sb.Append("\"full\":"); EmitGroup(root.GetProperty("full"), useStruct: true, sb);
sb.Append(",\"timeonly\":"); EmitGroup(root.GetProperty("timeonly"), useStruct: false, sb);
sb.Append("}");
Console.WriteLine(sb.ToString());

void EmitGroup(JsonElement arr, bool useStruct, StringBuilder outSb) {
    outSb.Append("{");
    bool first = true;
    foreach (var row in arr.EnumerateArray()) {
        if (!first) outSb.Append(","); first = false;
        string id = row.GetProperty("id").GetString()!;
        string wire = row.GetProperty("wire").GetString()!;
        string val = useStruct ? SerializeB(ParseISO(wire)) : NativeCanonical(wire);
        outSb.Append('"').Append(id).Append("\":\"").Append(val).Append('"');
    }
    outSb.Append("}");
}

static long[] ParseISO(string s) {
    // [y,mo,w,d,h,mi,s,week]
    var c = new long[8];
    string body = s.Substring(1);
    if (body.StartsWith("T")) { ParseTime(body.Substring(1), c); return c; }
    if (body.EndsWith("W")) { c[7] = 1; c[2] = long.Parse(body[..^1]); return c; }
    string datePart = body;
    int ti = body.IndexOf('T');
    if (ti >= 0) { datePart = body.Substring(0, ti); ParseTime(body.Substring(ti + 1), c); }
    var num = new StringBuilder();
    foreach (char ch in datePart) {
        if (char.IsDigit(ch)) { num.Append(ch); continue; }
        long v = long.Parse(num.ToString());
        if (ch == 'Y') c[0] = v; else if (ch == 'M') c[1] = v; else if (ch == 'D') c[3] = v;
        num.Clear();
    }
    return c;
}
static void ParseTime(string t, long[] c) {
    var num = new StringBuilder();
    foreach (char ch in t) {
        if (char.IsDigit(ch)) { num.Append(ch); continue; }
        long v = long.Parse(num.ToString());
        if (ch == 'H') c[4] = v; else if (ch == 'M') c[5] = v; else if (ch == 'S') c[6] = v;
        num.Clear();
    }
}
static string SerializeB(long[] c) {
    if (c[7] == 1) return $"P{c[2]}W";
    var date = new StringBuilder(); var tim = new StringBuilder();
    if (c[0] != 0) date.Append(c[0]).Append('Y');
    if (c[1] != 0) date.Append(c[1]).Append('M');
    if (c[3] != 0) date.Append(c[3]).Append('D');
    if (c[4] != 0) tim.Append(c[4]).Append('H');
    if (c[5] != 0) tim.Append(c[5]).Append('M');
    if (c[6] != 0) tim.Append(c[6]).Append('S');
    if (date.Length == 0 && tim.Length == 0) return "PT0S";
    return "P" + date + (tim.Length > 0 ? "T" + tim : "");
}
static string NativeCanonical(string s) {
    var ts = System.Xml.XmlConvert.ToTimeSpan(s); // native parse into TimeSpan
    long total = (long)ts.TotalSeconds;
    long h = total / 3600, m = (total % 3600) / 60, sec = total % 60;
    var b = new StringBuilder("PT");
    if (h != 0) b.Append(h).Append('H');
    if (m != 0) b.Append(m).Append('M');
    if (sec != 0 || (h == 0 && m == 0)) b.Append(sec).Append('S');
    return b.ToString();
}

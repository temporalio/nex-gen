using System;
using System.Text.RegularExpressions;

static string B(bool b) => b ? "true" : "false";
void T(string label, Func<bool> f)
{
    try { Console.WriteLine($"{label,-60} = {B(f())}"); }
    catch (Exception e) { Console.WriteLine($"{label,-60} = ERROR {e.GetType().Name}"); }
}

string astral = "a\U0001F600b";     // a + U+1F600 + b
string astral2 = "a\U0001F600";     // a + U+1F600

Console.WriteLine("=== astral `.` workaround attempts ===");
// Baseline: single `.` fails because emoji is a surrogate pair (2 UTF-16 units).
T(". : ^a.b$ on a<emoji>b", () => Regex.IsMatch(astral, "^a.b$"));

// RegexOptions.Singleline only makes `.` match \n; irrelevant to astral.
T(". Singleline: ^a.b$", () => Regex.IsMatch(astral, "^a.b$", RegexOptions.Singleline));

// There is no `u`/codepoint flag. Rewriting `.` to a surrogate-aware group is
// the only route: `.` -> (?:[\uD800-\uDBFF][\uDC00-\uDFFF]|.) matches a full
// code point. Demonstrate that manual rewrite works.
string cp = @"(?:[\uD800-\uDBFF][\uDC00-\uDFFF]|.)";
T($". rewritten to codepoint group: ^a{cp}b$", () => Regex.IsMatch(astral, "^a" + cp + "b$"));
T($". rewritten: ^a{cp}$ on a<emoji>", () => Regex.IsMatch(astral2, "^a" + cp + "\\z"));
// The rewrite must still match a BMP char as one unit:
T(". rewritten: ^a{cp}b$ on a<cjk>b (BMP)", () => Regex.IsMatch("a中b", "^a" + cp + "b$"));

// Does StringInfo/Runes help? No API-level codepoint regex mode exists.
Console.WriteLine("\n=== enumerate RegexOptions (for the record) ===");
foreach (var v in Enum.GetValues<RegexOptions>())
    Console.WriteLine($"  {v}");

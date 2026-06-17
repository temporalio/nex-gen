// JS half of the string-constraint probe (see main.go for the contract).
// LENGTH: .length counts UTF-16 code *units* — wrong for astral chars.
// [...s].length spreads by code point — the spec-correct count.
const s = "a\u{1F600}b";
console.log("JS   .length(UTF-16 units):", s.length, " [...s].length(codepoints):", [...s].length); // 4, 3
// No normalization: NFC é (1 code point) vs NFD é (2). All four agree per form.
console.log("JS   NFC u00e9 [...].length:", [...("\u00e9")].length);    // 1
console.log("JS   NFD e+u0301 [...].length:", [...("e\u0301")].length); // 2

// PATTERN: .test() is unanchored. The `u` flag is MANDATORY for P1 — it makes
// `.` and quantifiers code-point-aware (matching Go RE2 / Python / Java), while
// JS `\d\w\s` stay ASCII (ECMA-262). Without `u`, `.` is a UTF-16 unit and
// diverges on astral input.
console.log("JS   /a.b/  test 'a😀b' (NO u flag):", /a.b/.test(s));   // false  <- divergent
console.log("JS   /a.b/u test 'a😀b' (u flag):", /a.b/u.test(s));     // true   <- aligned
console.log("JS   /cat/u test 'the cat sat' (unanchored):", /cat/u.test("the cat sat"));
console.log("JS   /\\d/u matches Arabic digit ٣:", /\d/u.test("٣"));  // false (ASCII)
console.log("JS   /\\w/u matches u00e9:", /\w/u.test("\u00e9"));             // false (ASCII)
// RE2-rejected Perl features that JS accepts (why RE2 is the load-time gate):
console.log("JS   lookahead compiles:", (() => { try { new RegExp("(?=foo)", "u"); return true; } catch { return false; } })());
console.log("JS   backref compiles:", (() => { try { new RegExp("(a)\\1", "u"); return true; } catch { return false; } })());

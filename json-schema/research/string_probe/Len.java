// Java half of the string-constraint probe (see main.go for the contract).
// Run: java Len.java   (JDK 11+ single-file mode)
import java.util.regex.*;

public class Len {
  public static void main(String[] a) {
    String s = "a😀b"; // a + 😀 (surrogate pair) + b
    // LENGTH: String.length() counts UTF-16 code UNITS — wrong for astral chars.
    // codePointCount(0, length()) is the spec-correct code-point count.
    System.out.println("JAVA length(UTF-16 units): " + s.length()
        + "  codePointCount(codepoints): " + s.codePointCount(0, s.length())); // 4, 3
    // No normalization: NFC (1 code point) vs NFD (2). All four agree per form.
    String nfc = "\u00e9", nfd = "e\u0301";
    System.out.println("JAVA NFC u00e9 codePointCount: " + nfc.codePointCount(0, nfc.length()));   // 1
    System.out.println("JAVA NFD e+u0301 codePointCount: " + nfd.codePointCount(0, nfd.length())); // 2

    // PATTERN: use Matcher.find() (unanchored) — NOT matches() (anchors the whole
    // input, a silent footgun). Use DEFAULT flags: `.` is already code-point-aware
    // (matches astral), and \d\w\s stay ASCII (ECMA-262-aligned). Do NOT set
    // UNICODE_CHARACTER_CLASS — it flips \d\w\s to Unicode and diverges.
    System.out.println("JAVA 'a.b' find 'a😀b' (. = code point): " + Pattern.compile("a.b").matcher(s).find()); // true
    System.out.println("JAVA 'cat' find    'the cat sat' (unanchored): " + Pattern.compile("cat").matcher("the cat sat").find());
    System.out.println("JAVA 'cat' matches 'the cat sat' (anchored footgun): " + Pattern.compile("cat").matcher("the cat sat").matches()); // false
    System.out.println("JAVA '\\d' find Arabic digit ٣ (DEFAULT): " + Pattern.compile("\\d").matcher("٣").find()); // false (ASCII)
    System.out.println("JAVA '\\d' find Arabic digit ٣ (UNICODE_CHARACTER_CLASS): "
        + Pattern.compile("\\d", Pattern.UNICODE_CHARACTER_CLASS).matcher("٣").find()); // true <- do NOT use
    System.out.println("JAVA '\\w' find u00e9 (DEFAULT): " + Pattern.compile("\\w").matcher("\u00e9").find()); // false (ASCII)
    // Perl features Java accepts but RE2 rejects (gated out at load):
    System.out.println("JAVA lookahead compiles: " + tryCompile("(?=foo)"));
    System.out.println("JAVA backref compiles: " + tryCompile("(a)\\1"));
  }

  static boolean tryCompile(String p) {
    try { Pattern.compile(p); return true; } catch (PatternSyntaxException e) { return false; }
  }
}

// Probe: string-constraint cross-language behavior for minLength/maxLength
// (length unit) and pattern (regex dialect / anchoring / class semantics).
//
// Run alongside the sibling scripts (len.mjs, len.py, Len.java) — the four
// must agree value-for-value for P1. Findings drive features/maxLength,
// features/minLength, features/pattern.
package main

import (
	"fmt"
	"regexp"
	"unicode/utf8"
)

func main() {
	s := "a\U0001F600b" // a + emoji (astral U+1F600) + b
	// LENGTH: the spec (RFC 8259) counts Unicode code points. len() counts
	// UTF-8 *bytes* — the wrong unit; RuneCountInString is the code-point count.
	fmt.Println("GO   len(bytes):", len(s), " RuneCountInString(codepoints):", utf8.RuneCountInString(s)) // 6, 3
	// No normalization: the count is over code points as they appear on the
	// wire. Precomposed e-acute (NFC, U+00E9) is 1; decomposed (NFD, e+U+0301)
	// is 2. Every language agrees per form — the generator never normalizes.
	fmt.Println("GO   NFC \\u00e9 runes:", utf8.RuneCountInString("\u00e9"))    // 1
	fmt.Println("GO   NFD e+\\u0301 runes:", utf8.RuneCountInString("e\u0301")) // 2

	// PATTERN: RE2 is unanchored (find semantics) and is the STRICTEST engine —
	// it rejects the Perl features (lookaround, backreferences) the other three
	// accept, so an RE2-accepted pattern compiles everywhere. Compile-gate at load.
	fmt.Println("GO   'a.b' match 'a<emoji>b' (. = rune):", regexp.MustCompile("a.b").MatchString(s)) // true
	fmt.Println("GO   'cat' match 'the cat sat' (unanchored):", regexp.MustCompile("cat").MatchString("the cat sat"))
	_, e1 := regexp.Compile("(?=foo)")
	fmt.Println("GO   lookahead compile err:", e1)
	_, e2 := regexp.Compile(`(a)\1`)
	fmt.Println("GO   backref compile err:", e2)
	// CLASS SEMANTICS: RE2 \d\w\s are ASCII (ECMA-262-aligned).
	fmt.Println("GO   '\\d' matches Arabic-Indic digit U+0663:", regexp.MustCompile(`\d`).MatchString("٣")) // false (ASCII)
	fmt.Println("GO   '\\w' matches \\u00e9:", regexp.MustCompile(`\w`).MatchString("\u00e9")) // false (ASCII)
}

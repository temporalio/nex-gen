# Python half of the string-constraint probe (see main.go for the contract).
# LENGTH: Python 3 str is a sequence of code points, so len() is already the
# spec-correct count — no adjustment needed.
import re

s = "a\U0001F600b"
print("PY   len(codepoints):", len(s))              # 3
# No normalization: NFC é (1 code point) vs NFD é (2). All four agree per form.
print("PY   NFC u00e9 len:", len("\u00e9"))     # 1
print("PY   NFD e+u0301 len:", len("e\u0301"))  # 2

# PATTERN: use re.search (unanchored) — NOT re.match (anchors at start) or
# re.fullmatch (anchors both ends). Compile with re.ASCII so \d\w\s are ASCII
# (ECMA-262-aligned); Python's DEFAULT \d is Unicode-aware and would diverge.
# `.` is already code-point-aware (str is code points).
print("PY   'a.b' search 'a😀b':", bool(re.search("a.b", s)))                 # True
print("PY   'cat' search 'the cat sat' (unanchored):", bool(re.search("cat", "the cat sat")))
print("PY   'cat' match  'the cat sat' (anchored-start footgun):", bool(re.match("cat", "the cat sat")))  # False
print("PY   '\\d' matches Arabic digit ٣ (DEFAULT=Unicode):", bool(re.search(r"\d", "٣")))          # True  <- divergent
print("PY   '\\d' matches Arabic digit ٣ (re.ASCII):", bool(re.search(r"\d", "٣", re.ASCII)))        # False <- aligned
print("PY   '\\w' matches u00e9 (re.ASCII):", bool(re.search(r"\w", "\u00e9", re.ASCII)))    # False
# Perl features Python accepts but RE2 rejects (gated out at load):
print("PY   lookahead compiles:", bool(re.compile("(?=foo)")))
print("PY   backref compiles:", bool(re.compile(r"(a)\1")))

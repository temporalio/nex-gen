# Probe: can Pydantic v2's NATIVE `pattern=` constraint replace the generator's
# explicit `re.compile(p, re.ASCII).search(v)` AfterValidator for the Python
# target, and stay byte-for-byte semantically identical on the gate-accepted
# subset (no lookaround / backref)?
#
# Pinned cross-target semantics for `pattern`: UNANCHORED (Go MatchString /
# JS RegExp.test / Java Matcher.find), ASCII `\d\w\s`, code-point `.`.
# Native pydantic matches via pydantic-core's Rust `regex` crate, NOT Python's
# `re`, so accept/reject must be VERIFIED, not assumed.
#
# This script builds, for each pattern P, a BaseModel with
# `Annotated[str, StringConstraints(pattern=P)]` and compares its accept/reject
# against `re.compile(P, re.ASCII).search(v)` (the current design's semantics).
#
# Result (pydantic 2.13.4): NATIVE pattern= is NOT a drop-in replacement.
# The decisive divergence is CHARACTER CLASSES (ASCII vs Unicode):
#   - pydantic-core's Rust `regex` crate treats `\d \w \s` as UNICODE by
#     default, so `^\d+$` ACCEPTS "٣" (U+0663) and `^\w+$` ACCEPTS "café"
#     while `re.compile(p, re.ASCII)` REJECTS both. Our pinned semantics = ASCII.
# Things that AGREE (so are NOT reasons to keep the AfterValidator on their own):
#   - Anchoring: pydantic-core is UNANCHORED (find/search semantics) - `cat`
#     matches "the cat sat", "xcatx", "foobar"~"bar" - same as re.search.
#   - `.` (dot): matches one code point (incl. astral), same as re.ASCII.
#   - Lookaround / backref: rejected by pydantic-core at model-BUILD time
#     (Rust regex has no backrefs/lookaround), whereas Python re accepts them.
#     This is moot on the gate-accepted subset (gate already forbids them).
# VERDICT: keep the explicit `re`+`re.ASCII`+`.search` AfterValidator, solely
# because native pattern= cannot express ASCII-only \d\w\s (would need `(?-u:..)`
# rewriting of every user pattern, which is out of scope). Anchoring/dot match.
#
# Run:  /tmp/pydvenv/bin/python pydantic_pattern_probe.py
#   (or: python -m venv v && ./v/bin/pip install pydantic && ./v/bin/python this)
import re
import pydantic
from pydantic import BaseModel, StringConstraints
from typing import Annotated

print("pydantic", pydantic.VERSION)


def native_accepts(pattern, v):
    """Accept/reject via pydantic-core NATIVE pattern= (Rust regex engine)."""
    class M(BaseModel):
        s: Annotated[str, StringConstraints(pattern=pattern)]
    try:
        M(s=v)
        return True
    except Exception:
        return False


def re_accepts(pattern, v):
    """Accept/reject via the current design: re.compile(p, re.ASCII).search."""
    return re.compile(pattern, re.ASCII).search(v) is not None


def native_model_builds(pattern):
    """Does pydantic-core even COMPILE the schema for a model with this pattern?
    (Uses model_json_schema()/validator build, NOT a match, so a non-matching
    instance is not mistaken for a build failure.)"""
    try:
        class M(BaseModel):
            s: Annotated[str, StringConstraints(pattern=pattern)]
        M.__pydantic_validator__          # force core validator build
        M.model_json_schema()
        return True
    except Exception as e:
        return f"REJECTED: {type(e).__name__}"


print("\n=== Q1: ANCHORING (pattern 'cat' etc. vs 'the cat sat') ===")
for pattern, v in [("cat", "the cat sat"),
                   ("cat", "cat sat"),
                   ("cat", "the cat"),
                   ("cat", "cat"),
                   ("^cat", "the cat sat"),
                   ("cat$", "the cat sat"),
                   ("cat$", "the cat"),
                   ("^cat$", "cat")]:
    n = native_accepts(pattern, v)
    r = re_accepts(pattern, v)
    flag = "" if n == r else "  <-- DISAGREE"
    print(f"  P={pattern!r:8} v={v!r:14} native={n!s:5} re.ASCII.search={r!s:5}{flag}")

print("\n=== Q2: CHARACTER CLASSES ASCII vs UNICODE ===")
for pattern, v, note in [(r"^\d+$", "5", "ASCII digit"),
                         (r"^\d+$", "٣", "Arabic-Indic 3 U+0663"),
                         (r"^\w+$", "abc", "ASCII word"),
                         (r"^\w+$", "café", "accented e (café)"),
                         (r"^\s+$", " ", "ASCII space"),
                         (r"^\s+$", " ", "NBSP U+00A0")]:
    n = native_accepts(pattern, v)
    r = re_accepts(pattern, v)
    flag = "" if n == r else "  <-- DISAGREE"
    print(f"  P={pattern!r:8} v={v!r:10} ({note:22}) native={n!s:5} re.ASCII={r!s:5}{flag}")

print("\n=== Q3: DOT matches one code point (astral) ===")
for pattern, v, note in [("^a.b$", "a\U0001F600b", "a<emoji>b astral"),
                         ("^a.b$", "aXb", "aXb bmp"),
                         ("^a.b$", "ab", "no middle char")]:
    n = native_accepts(pattern, v)
    r = re_accepts(pattern, v)
    flag = "" if n == r else "  <-- DISAGREE"
    print(f"  P={pattern!r:8} v={v!r:14} ({note:18}) native={n!s:5} re.ASCII={r!s:5}{flag}")

print("\n=== Q4: non-regular constructs (model build time) ===")
for pattern in [r"(?=cat)", r"(a)\1", r"(?!x)", r"[a-z]+"]:
    print(f"  P={pattern!r:12} native model builds: {native_model_builds(pattern)}")
    try:
        re.compile(pattern, re.ASCII)
        print(f"                 re.compile: OK")
    except re.error as e:
        print(f"                 re.compile: error {e}")

print("\n=== Q5: CORPUS agreement (native vs re.ASCII.search) ===")
corpus = [
    ("cat", "the cat sat"),
    ("cat", "cat"),
    ("cat", "concatenate"),
    ("^cat", "cats"),
    ("^cat", "a cat"),
    ("cat$", "wildcat"),
    ("cat$", "cats"),
    ("^cat$", "cat"),
    ("^cat$", "cats"),
    ("[0-9]+", "abc123"),
    ("^[0-9]+$", "123"),
    ("^[0-9]+$", "12a"),
    (r"\d{3}", "id 456 x"),
    (r"^\d{3}$", "456"),
    (r"^\d+$", "٣"),          # Arabic-Indic
    (r"^\w+$", "hello_world"),
    (r"^\w+$", "café"),       # accented
    (r"^\w+$", "naïve"),      # naïve
    (r"^\s+$", " "),          # NBSP
    (r"^\s+$", "   "),
    ("^a.b$", "a\U0001F600b"),     # astral dot
    ("^a.c$", "abc"),
    ("^h.*o$", "hello"),
    ("colou?r", "my color"),
    ("colou?r", "colour here"),
    ("(foo|bar)", "a bar b"),
    ("^(foo|bar)$", "foo"),
    ("^(foo|bar)$", "foobar"),
    (r"^\S+@\S+$", "a@b"),
    (r"^\S+@\S+$", "a b@c"),
    ("A", "banana"),
    ("^A", "Apple"),
]
disagreements = []
for pattern, v in corpus:
    n = native_accepts(pattern, v)
    r = re_accepts(pattern, v)
    if n != r:
        disagreements.append((pattern, v, n, r))

print(f"  {len(corpus)} pairs tested, {len(disagreements)} DISAGREEMENTS:")
for pattern, v, n, r in disagreements:
    print(f"    P={pattern!r:12} v={v!r:16} native={n!s:5} re.ASCII.search={r!s:5}")
if not disagreements:
    print("    (none)")

print("\n=== VERDICT ===")
print("  NATIVE pattern= is NOT equivalent to re.compile(p, re.ASCII).search.")
print("  Divergence: CHAR CLASSES - Rust regex \\d\\w\\s are UNICODE, not ASCII.")
print("  Anchoring (unanchored) and dot (one code point) AGREE; lookaround/backref")
print("  are rejected at build time (moot on gate-accepted subset).")
print("  => Keep the explicit re + re.ASCII + .search AfterValidator.")

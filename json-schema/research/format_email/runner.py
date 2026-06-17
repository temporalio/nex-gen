#!/usr/bin/env python3
"""Python runner. Compiles the pinned email regex with re.compile(p, re.ASCII)
and applies .search(v) (unanchored -- irrelevant, the regex is ^...$ anchored).

Per the `pattern` gate recipe, Python's `$` matches end-of-input OR just before
a single trailing `\\n`, so the generator rewrites a trailing `$` anchor to
`\\Z` (strict end-of-string). We apply the same rewrite here.

re.ASCII is used for parity with the pinned recipe; the pinned regex uses only
explicit ASCII classes, so re.ASCII changes nothing but is kept for fidelity.

Emits JSON Lines: {"id","engine":"python","compiled":bool,"matched":bool|null}
"""
import json
import re
import sys


def normalize_dollar(pattern: str) -> str:
    """Rewrite a trailing, unescaped, non-char-class `$` anchor to `\\Z`.
    Sufficient for the corpus (its `$` is the single trailing anchor)."""
    if not pattern.endswith("$"):
        return pattern
    # count trailing backslashes before the `$`
    backslashes = 0
    j = len(pattern) - 2
    while j >= 0 and pattern[j] == "\\":
        backslashes += 1
        j -= 1
    if backslashes % 2 == 1:
        return pattern  # escaped literal \$
    return pattern[:-1] + r"\Z"


def main() -> None:
    path = sys.argv[1] if len(sys.argv) > 1 else "corpus.json"
    with open(path, encoding="utf-8") as fh:
        corpus = json.load(fh)
    pattern = normalize_dollar(corpus["pinned_regex"])
    rx = None
    compiled = False
    try:
        rx = re.compile(pattern, re.ASCII)
        compiled = True
    except re.error:
        compiled = False
    out = sys.stdout
    for p in corpus["pairs"]:
        matched = (rx.search(p["instance"]) is not None) if compiled else None
        out.write(
            json.dumps(
                {"id": p["id"], "engine": "python", "compiled": compiled, "matched": matched},
                ensure_ascii=False,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()

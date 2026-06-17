#!/usr/bin/env python3
"""Python runner for the `duration` format conformance corpus.

Compiles the SINGLE generator-owned pinned regex (from corpus.json's
`pinned_regex`) with re.compile(p, re.ASCII) -- the pinned choice for the
Python target -- and matches each corpus value with re.search (unanchored;
the anchors are in the pattern).

$ NORMALIZATION: Python's `$` matches at end-of-input OR just before a single
trailing `\n`, so a raw `$` would ACCEPT "P1Y\n" (the `newline-tail` case),
diverging from Go/JS. The `pattern` spec pins the fix -- the generator emits
`\Z` (strict end-of-input, no trailing-\n exception) instead of `$` for the
Python target. This runner applies that exact trailing `$`->`\Z` rewrite, so it
tests the FORM the generator would emit. (The pinned regex uses [0-9] rather
than \\d, so re.ASCII is not even load-bearing here, but is kept as the pinned
recipe.)

Reads corpus.json (argv[1] or ./corpus.json) and emits JSON Lines to stdout:
    {"id","engine":"python","compiled":bool,"matched":bool|null}

Run: python3 runner.py [corpus.json]
"""
import json
import re
import sys


def main() -> None:
    path = sys.argv[1] if len(sys.argv) > 1 else "corpus.json"
    with open(path, encoding="utf-8") as fh:
        corpus = json.load(fh)

    pinned = corpus["pinned_regex"]
    # trailing `$` -> `\Z` (strict end-of-input; Python `$` has a trailing-\n exception).
    emitted = pinned[:-1] + r"\Z" if pinned.endswith("$") else pinned

    compiled = False
    rx = None
    try:
        rx = re.compile(emitted, re.ASCII)
        compiled = True
    except re.error:
        compiled = False

    out = sys.stdout
    for k in corpus["cases"]:
        matched = (rx.search(k["value"]) is not None) if compiled else None
        out.write(
            json.dumps(
                {"id": k["id"], "engine": "python", "compiled": compiled, "matched": matched},
                ensure_ascii=False,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()

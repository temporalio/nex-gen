#!/usr/bin/env python3
"""Python runner for the `hostname` format conformance corpus.

Implements the PINNED generator-owned check for the Python target:
  1. module-level re.compile(pattern, re.ASCII), fully anchored.
     The end anchor is written `\\Z` (Python's STRICT end-of-string anchor);
     plain `$` in Python matches before a trailing '\\n', which would diverge
     from Go/JS -- exactly the pattern-spec `$`->`\\Z` normalization.
  2. a total-length guard (1..=253 code points) OUTSIDE the regex.
Verdict = (regex matches) AND (length in range).

Emits JSON Lines: {"id","engine":"python","valid","regex","len_ok"}
Run: python3 runner.py [corpus.json]
"""
import json
import re
import sys

HOST_RE = re.compile(
    r"\A[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?"
    r"(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*\Z",
    re.ASCII,
)
MAX_TOTAL_LEN = 253


def main() -> None:
    path = sys.argv[1] if len(sys.argv) > 1 else "corpus.json"
    with open(path, encoding="utf-8") as fh:
        corpus = json.load(fh)
    out = sys.stdout
    for k in corpus["cases"]:
        inst = k["instance"]
        n = len(inst)  # Python str len is code points
        len_ok = 1 <= n <= MAX_TOTAL_LEN
        regex = HOST_RE.search(inst) is not None
        out.write(
            json.dumps(
                {
                    "id": k["id"],
                    "engine": "python",
                    "valid": regex and len_ok,
                    "regex": regex,
                    "len_ok": len_ok,
                },
                ensure_ascii=False,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()

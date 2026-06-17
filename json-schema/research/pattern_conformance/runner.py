#!/usr/bin/env python3
"""Python runner. Uses re.compile(p, re.ASCII) then .search(v) (unanchored),
mirroring the pinned runtime semantics for the Python target.

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
    out = sys.stdout
    for p in corpus["pairs"]:
        compiled = False
        matched = None
        try:
            rx = re.compile(p["pattern"], re.ASCII)
            compiled = True
            matched = rx.search(p["instance"]) is not None
        except re.error:
            compiled = False
            matched = None
        out.write(
            json.dumps(
                {"id": p["id"], "engine": "python", "compiled": compiled, "matched": matched},
                ensure_ascii=False,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()

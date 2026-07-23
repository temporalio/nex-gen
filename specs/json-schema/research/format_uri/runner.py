#!/usr/bin/env python3
"""Python runner for the PINNED `uri` check. Anchors the body with \\A...\\Z
(full-string, no trailing-\\n exception) and compiles with re.ASCII.

Emits JSON Lines: {"id","engine":"python","compiled":bool,"matched":bool|null}
Run: python3 runner.py [corpus.json] [pinned_body.json]
"""
import json
import re
import sys


def main() -> None:
    corpus_path = sys.argv[1] if len(sys.argv) > 1 else "corpus.json"
    body_path = sys.argv[2] if len(sys.argv) > 2 else "pinned_body.json"
    with open(corpus_path, encoding="utf-8") as fh:
        corpus = json.load(fh)
    with open(body_path, encoding="utf-8") as fh:
        body = json.load(fh)["body"]

    out = sys.stdout
    compiled = False
    rx = None
    try:
        rx = re.compile(r"\A" + body + r"\Z", re.ASCII)
        compiled = True
    except re.error as e:
        sys.stderr.write(f"PYTHON COMPILE ERROR: {e}\n")

    for p in corpus["pairs"]:
        matched = None if not compiled else (rx.search(p["value"]) is not None)
        out.write(
            json.dumps(
                {"id": p["id"], "engine": "python", "compiled": compiled, "matched": matched},
                ensure_ascii=False,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()

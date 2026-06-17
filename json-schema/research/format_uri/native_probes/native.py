#!/usr/bin/env python3
"""Python NATIVE URI-parser probe (urllib.parse). For each input, reports what
urllib.parse.urlsplit thinks. urllib almost NEVER raises; a naive stdlib
format:uri check would treat "has a scheme" as valid absolute. We report
`bool(scheme)` as the "valid absolute" verdict, which is what a naive user does.

Emits JSON Lines: {"id","engine":"python-native","valid":bool,"detail":str}
Run: python3 native.py ../native_inputs.json
"""
import json
import sys
from urllib.parse import urlsplit


def main() -> None:
    path = sys.argv[1] if len(sys.argv) > 1 else "../native_inputs.json"
    with open(path, encoding="utf-8") as fh:
        corpus = json.load(fh)
    out = sys.stdout
    for inp in corpus["inputs"]:
        valid = False
        detail = ""
        try:
            parts = urlsplit(inp["value"])
            if parts.scheme:
                valid = True
                detail = f"scheme={parts.scheme} netloc={parts.netloc!r}"
            else:
                detail = "no scheme"
        except ValueError as e:
            detail = f"error: {e}"
        out.write(
            json.dumps(
                {"id": inp["id"], "engine": "python-native", "valid": valid, "detail": detail},
                ensure_ascii=False,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()

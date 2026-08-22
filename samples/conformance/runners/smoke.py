"""Import smoke test for the probe matrix: does each generated package import?

`py_compile` proves the file parses. Importing it additionally runs every
module-level statement — the compiled `re` patterns, the dataclass bodies, the
`nexusrpc` service decorator that evaluates a deprecated operation's annotation
— which is where several defects live.

Usage: ``python smoke.py <result.json> <generated-root> <package>...``
"""

from __future__ import annotations

import importlib
import json
import sys
from pathlib import Path


def main() -> int:
    result_path, generated_root, *packages = sys.argv[1:]
    sys.path.insert(0, generated_root)
    results: dict[str, str] = {}
    for package in packages:
        try:
            importlib.import_module(package)
            results[package] = "ok"
        except BaseException as error:  # noqa: BLE001 - the verdict is the point
            results[package] = f"{type(error).__name__}: {error}"
    Path(result_path).write_text(json.dumps(results, indent=1), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

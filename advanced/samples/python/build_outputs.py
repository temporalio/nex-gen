from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys


def main() -> None:
    app_root = Path(__file__).resolve().parent
    repo_root = app_root.parent.parent
    _ = subprocess.run(
        generator_command() + ["--lang", "python", *sys.argv[1:]],
        check=True,
        cwd=repo_root,
    )


def generator_command() -> list[str]:
    if configured_binary := os.environ.get("NEXGEN_BIN"):
        return [configured_binary, "build-examples"]

    return ["cargo", "build-examples"]


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Python runner for the JSON-Schema `format` conformance corpus.

Implements the SPEC'S PINNED CHECK: a pinned anchored regex compiled once at
module level with re.ASCII (per the `pattern` design), plus the shared
integer-arithmetic calendar predicate for the temporal formats. This is the
OWNED check -- we deliberately do NOT use Pydantic's UUID/datetime types or
datetime.fromisoformat as the source of truth (they coerce/normalize and their
grammars differ). As a SECONDARY column we record what datetime.fromisoformat
accepts, purely to document divergence.

Reads corpus.json (argv[1] or ./corpus.json) and emits JSON Lines to stdout:
    {"id","engine":"python","valid":bool,"native":bool}

Run: python3 runner.py [corpus.json]
"""
import datetime
import json
import re
import sys

# ---- pinned patterns (anchored, re.ASCII, compiled once) --------------------

OCTET = r"(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])"
H16 = r"[0-9a-fA-F]{1,4}"
V4 = rf"({OCTET}\.{OCTET}\.{OCTET}\.{OCTET})"
LS32 = rf"({H16}:{H16}|{V4})"
IPV6 = (
    "^("
    rf"({H16}:){{6}}{LS32}|"
    rf"::({H16}:){{5}}{LS32}|"
    rf"({H16})?::({H16}:){{4}}{LS32}|"
    rf"(({H16}:){{0,1}}{H16})?::({H16}:){{3}}{LS32}|"
    rf"(({H16}:){{0,2}}{H16})?::({H16}:){{2}}{LS32}|"
    rf"(({H16}:){{0,3}}{H16})?::({H16}:){LS32}|"
    rf"(({H16}:){{0,4}}{H16})?::{LS32}|"
    rf"(({H16}:){{0,5}}{H16})?::{H16}|"
    rf"(({H16}:){{0,6}}{H16})?::"
    ")\\Z"
)

UUID_RE = re.compile(
    r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\Z",
    re.ASCII,
)
IPV4_RE = re.compile(rf"^{OCTET}\.{OCTET}\.{OCTET}\.{OCTET}\Z", re.ASCII)
IPV6_RE = re.compile(IPV6, re.ASCII)
DATE_RE = re.compile(r"^([0-9]{4})-([0-9]{2})-([0-9]{2})\Z", re.ASCII)
TIME_RE = re.compile(
    r"^([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})?\Z",
    re.ASCII,
)
DATE_TIME_RE = re.compile(
    r"^([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})\Z",
    re.ASCII,
)

# ---- shared calendar predicate (integer arithmetic only) --------------------


def is_leap(y: int) -> bool:
    return (y % 4 == 0 and y % 100 != 0) or y % 400 == 0


def days_in_month(y: int, m: int) -> int:
    if m in (1, 3, 5, 7, 8, 10, 12):
        return 31
    if m in (4, 6, 9, 11):
        return 30
    if m == 2:
        return 29 if is_leap(y) else 28
    return 0


def valid_calendar_date(y: int, m: int, d: int) -> bool:
    return 1 <= m <= 12 and 1 <= d <= days_in_month(y, m)


def valid_time_fields(hh: int, mm: int, ss: int) -> bool:
    return hh <= 23 and mm <= 59 and ss <= 60  # :60 leap second accepted


def valid_offset(off: str) -> bool:
    if not off or off in ("Z", "z"):
        return True
    return int(off[1:3]) <= 23 and int(off[4:6]) <= 59


# ---- pinned per-format check ------------------------------------------------


def pinned_valid(fmt: str, v: str) -> bool:
    if fmt == "uuid":
        return UUID_RE.match(v) is not None
    if fmt == "ipv4":
        return IPV4_RE.match(v) is not None
    if fmt == "ipv6":
        return IPV6_RE.match(v) is not None
    if fmt == "date":
        g = DATE_RE.match(v)
        return g is not None and valid_calendar_date(int(g[1]), int(g[2]), int(g[3]))
    if fmt == "time":
        g = TIME_RE.match(v)
        return (
            g is not None
            and valid_time_fields(int(g[1]), int(g[2]), int(g[3]))
            and valid_offset(g[5] or "")
        )
    if fmt == "date-time":
        g = DATE_TIME_RE.match(v)
        return (
            g is not None
            and valid_calendar_date(int(g[1]), int(g[2]), int(g[3]))
            and valid_time_fields(int(g[4]), int(g[5]), int(g[6]))
            and valid_offset(g[8] or "")
        )
    return False


# ---- SECONDARY: native datetime parser (documentation only) -----------------


def native_valid(fmt: str, v: str) -> bool:
    try:
        if fmt == "date":
            datetime.date.fromisoformat(v)
            return True
        if fmt == "time":
            datetime.time.fromisoformat(v.replace("Z", "+00:00").replace("z", "+00:00"))
            return True
        if fmt == "date-time":
            datetime.datetime.fromisoformat(v.replace("Z", "+00:00").replace("z", "+00:00"))
            return True
    except ValueError:
        return False
    return False  # no stdlib parser for uuid/ipv4/ipv6 in this column


def main() -> None:
    path = sys.argv[1] if len(sys.argv) > 1 else "corpus.json"
    with open(path, encoding="utf-8") as fh:
        corpus = json.load(fh)
    out = sys.stdout
    for p in corpus["pairs"]:
        out.write(
            json.dumps(
                {
                    "id": p["id"],
                    "engine": "python",
                    "valid": pinned_valid(p["format"], p["value"]),
                    "native": native_valid(p["format"], p["value"]),
                },
                ensure_ascii=False,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()

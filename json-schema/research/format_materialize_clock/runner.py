#!/usr/bin/env python3
"""Probe: MATERIALIZE model (B) in Python via stdlib datetime/date/time.
Parse each validated wire string into the typed construct, then re-serialize via
the GENERATOR-OWNED serializer (RFC 3339, original offset preserved with
+00:00/-00:00 -> Z, T/Z uppercased on the parse path, fractional seconds at the
value's own precision with trailing zeros trimmed, no fractional part when
zero) -- NO TRUNCATION beyond the type's genuine limit.

  date-time -> datetime (aware)      offset preserved; MICROSECOND resolution
                                     (sub-us input truncated -- the one
                                     Python-side loss).
  date      -> date                  YYYY-MM-DD, lossless.
  time      -> time (aware / naive)  offset preserved when present, lossless.

python3 runner.py corpus.json"""
import json, sys
from datetime import datetime, date, time

ENGINE = "py"


def emit(o):
    print(json.dumps(o))


def frac_micros(micro):
    """'.ddd' with trailing zeros trimmed, or '' when zero."""
    if micro == 0:
        return ""
    return "." + f"{micro:06d}".rstrip("0")


def offset_str(td):
    """Offset from a timedelta: 'Z' for zero, '' when None, else +/-HH:MM."""
    if td is None:
        return ""
    secs = int(td.total_seconds())
    if secs == 0:
        return "Z"
    sign = "+" if secs > 0 else "-"
    secs = abs(secs)
    return f"{sign}{secs // 3600:02d}:{(secs % 3600) // 60:02d}"


def canon_datetime(wire):
    # Parse path uppercases case-insensitive t/z (pinned grammar accepts
    # lowercase; fromisoformat rejects it). Safe: no other letters. Rejects :60.
    dt = datetime.fromisoformat(wire.upper())
    if dt.tzinfo is None:
        raise ValueError("missing offset (naive) -- date-time requires an offset")
    # offset PRESERVED (no astimezone(utc)); microsecond resolution is the
    # native limit -- any finer input was already truncated by fromisoformat.
    return (f"{dt.year:04d}-{dt.month:02d}-{dt.day:02d}"
            f"T{dt.hour:02d}:{dt.minute:02d}:{dt.second:02d}"
            f"{frac_micros(dt.microsecond)}{offset_str(dt.utcoffset())}")


def canon_date(wire):
    d = date.fromisoformat(wire)
    return f"{d.year:04d}-{d.month:02d}-{d.day:02d}"


def canon_time(wire):
    t = time.fromisoformat(wire.upper())  # aware when offset present, else naive
    return (f"{t.hour:02d}:{t.minute:02d}:{t.second:02d}"
            f"{frac_micros(t.microsecond)}{offset_str(t.utcoffset())}")


def run(rows, fmt, fn):
    for r in rows:
        try:
            emit({"id": r["id"], "engine": ENGINE, "format": fmt,
                  "canonical": fn(r["wire"]), "err": ""})
        except Exception as e:
            emit({"id": r["id"], "engine": ENGINE, "format": fmt,
                  "canonical": "", "err": f"{type(e).__name__}: {e}"})


def main():
    c = json.load(open(sys.argv[1]))
    run(c["date-time"], "date-time", canon_datetime)
    run(c["date"], "date", canon_date)
    run(c["time"], "time", canon_time)


if __name__ == "__main__":
    main()

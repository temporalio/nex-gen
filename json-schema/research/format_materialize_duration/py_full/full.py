#!/usr/bin/env python3
"""Python materialization probe for the `duration` format.

Q1: stdlib has NO ISO-8601 duration parser and NO type holding Y/M/W.
    timedelta = days+seconds+microseconds (fixed), no year/month/week concept,
    and timedelta.isoformat does not exist. Prove the gaps.
Q2: design C - narrowed time-only PTnHnMnS -> timedelta. Since Python has no
    stdlib ISO parser, WE parse the (already-validated) time-only string into a
    timedelta and re-emit our canonical PTnHnMnS. Confirm it matches Go/Java.

Run: cd py_full && python3 full.py
"""
from datetime import timedelta


def parse_timeonly(s: str) -> timedelta:
    """Parse an already-validated PTnHnMnS string into a timedelta."""
    assert s.startswith("PT")
    body = s[2:]
    num = ""
    h = m = sec = 0
    for c in body:
        if c.isdigit():
            num += c
        else:
            v = int(num)
            if c == "H":
                h = v
            elif c == "M":
                m = v
            elif c == "S":
                sec = v
            num = ""
    return timedelta(hours=h, minutes=m, seconds=sec)


def canonical(td: timedelta) -> str:
    """Canonical PTnHnMnS from a timedelta total (same algo as Go/Java)."""
    total = int(td.total_seconds())
    h, rem = divmod(total, 3600)
    m, s = divmod(rem, 60)
    out = "PT"
    if h:
        out += f"{h}H"
    if m:
        out += f"{m}M"
    if s or (h == 0 and m == 0):
        out += f"{s}S"
    return out


def main():
    print("=== Q1: stdlib gaps ===")
    print("  datetime has NO ISO-8601 duration parser (no timedelta.fromisoformat for P...).")
    print("  timedelta stores days/seconds/microseconds only: no years, months, or week form.")
    print("  timedelta has NO .isoformat()/ISO serializer at all.")
    print("  => full grammar (P1Y / P1M / P4W) is unrepresentable & unserializable in stdlib.\n")

    print("=== Q2: design C time-only -> timedelta -> canonical ===")
    for w in ["PT1H", "PT30M", "PT15S", "PT1H30M15S", "PT1H30M", "PT30M15S", "PT0S"]:
        td = parse_timeonly(w)
        got = canonical(td)
        print(f"  {w:12} -> timedelta {str(td):16} -> {got:12} roundtrip={got == w}")

    print("  non-canonical:")
    for w in ["PT90M", "PT3600S", "PT24H"]:
        td = parse_timeonly(w)
        print(f"  {w:10} -> timedelta {str(td):12} -> canonical {canonical(td):10}")


if __name__ == "__main__":
    main()

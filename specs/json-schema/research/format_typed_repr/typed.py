#!/usr/bin/env python3
"""Probe: Python STANDARD LIBRARY typed reps for the 6 formats.
stdlib only (datetime, uuid, ipaddress). Run: python3 typed.py
Backs features/format typed-repr research."""
import datetime as dt
import uuid
import ipaddress

def line(label, s, fn):
    try:
        print(f"  {label:8} {s!r:40} -> {fn()}")
    except Exception as e:
        print(f"  {label:8} {s!r:40} -> ERR {type(e).__name__}: {e}")

print("=== Python stdlib typed representations ===")

# ---- date-time : datetime.datetime.fromisoformat ----
print("\n[date-time] type=datetime.datetime  ctor=datetime.fromisoformat(s)")
for s in [
    "2021-02-28T23:59:60Z",           # leap second
    "2006-01-02T15:04:05Z",
    "2006-01-02T15:04:05+00:00",
    "2006-01-02T15:04:05-00:00",
    "2006-01-02T15:04:05.123456789Z", # 9-digit fractional
    "2006-01-02t15:04:05z",           # lowercase
    "2006-01-02T15:04:05",            # missing offset
    "2021-02-30T00:00:00Z",           # bad calendar
]:
    def f(s=s):
        d = dt.datetime.fromisoformat(s)
        return f"OK isoformat={d.isoformat()}  tzinfo={d.tzinfo}"
    line("datetime", s, f)

# ---- date : datetime.date.fromisoformat ----
print("\n[date] type=datetime.date  ctor=date.fromisoformat(s)")
for s in ["2020-02-29", "2021-02-29", "2021-13-01"]:
    line("date", s, lambda s=s: f"OK -> {dt.date.fromisoformat(s).isoformat()}")

# ---- time : datetime.time.fromisoformat ----
print("\n[time] type=datetime.time  ctor=time.fromisoformat(s)")
for s in ["12:00:00", "23:59:60Z", "12:00:00.5+01:00", "12:00:00Z"]:
    def f(s=s):
        t = dt.time.fromisoformat(s)
        return f"OK isoformat={t.isoformat()} tzinfo={t.tzinfo}"
    line("time", s, f)

# ---- uuid : uuid.UUID(s) ----
print("\n[uuid] type=uuid.UUID  ctor=uuid.UUID(s)")
for s in ["f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
          "F81D4FAE-7DEC-11D0-A765-00A0C91E6BF6",  # uppercase
          "f81d4fae7dec11d0a76500a0c91e6bf6",       # no dashes (pinned regex REJECTS)
          "{f81d4fae-7dec-11d0-a765-00a0c91e6bf6}", # braces (pinned REJECTS)
          "urn:uuid:f81d4fae-7dec-11d0-a765-00a0c91e6bf6",  # urn (pinned REJECTS)
          "not-a-uuid"]:
    def f(s=s):
        u = uuid.UUID(s)
        return f"OK str={u!s}"
    line("UUID", s, f)

# ---- ipv4 / ipv6 : ipaddress ----
print("\n[ipv4] type=ipaddress.IPv4Address  ctor=IPv4Address(s)")
for s in ["192.168.0.1", "256.0.0.1", "01.2.3.4", "1.2.3", "1.2.3.4.5"]:
    line("IPv4", s, lambda s=s: f"OK -> {ipaddress.IPv4Address(s)!s}")

print("\n[ipv6] type=ipaddress.IPv6Address  ctor=IPv6Address(s)")
for s in ["::1", "2001:db8::1", "2001:DB8::1", "::ffff:192.168.0.1",
          "fe80::1%eth0", "2001:0db8:0000:0000:0000:0000:0000:0001"]:
    line("IPv6", s, lambda s=s: f"OK -> {ipaddress.IPv6Address(s)!s}")

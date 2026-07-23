#!/usr/bin/env python3
"""Emit canonical re-serialization JSON for the corpus (Python).
design B custom components for `full`; native timedelta for `timeonly`."""
import json
from datetime import timedelta


def parse_iso(s):
    c = dict(y=0, mo=0, w=0, d=0, h=0, mi=0, sec=0, week=False)
    body = s[1:]
    if body.startswith("T"):
        _pt(body[1:], c); return c
    if body.endswith("W"):
        c["week"] = True; c["w"] = int(body[:-1]); return c
    date_part = body
    ti = body.find("T")
    if ti >= 0:
        date_part = body[:ti]; _pt(body[ti + 1:], c)
    num = ""
    for ch in date_part:
        if ch.isdigit(): num += ch
        else:
            v = int(num)
            if ch == "Y": c["y"] = v
            elif ch == "M": c["mo"] = v
            elif ch == "D": c["d"] = v
            num = ""
    return c


def _pt(t, c):
    num = ""
    for ch in t:
        if ch.isdigit(): num += ch
        else:
            v = int(num)
            if ch == "H": c["h"] = v
            elif ch == "M": c["mi"] = v
            elif ch == "S": c["sec"] = v
            num = ""


def serialize_b(c):
    if c["week"]: return f"P{c['w']}W"
    date = ""
    if c["y"]: date += f"{c['y']}Y"
    if c["mo"]: date += f"{c['mo']}M"
    if c["d"]: date += f"{c['d']}D"
    tim = ""
    if c["h"]: tim += f"{c['h']}H"
    if c["mi"]: tim += f"{c['mi']}M"
    if c["sec"]: tim += f"{c['sec']}S"
    if not date and not tim: return "PT0S"
    return "P" + date + ("T" + tim if tim else "")


def native_canonical(s):
    c = parse_iso(s)
    td = timedelta(hours=c["h"], minutes=c["mi"], seconds=c["sec"])
    total = int(td.total_seconds())
    h, rem = divmod(total, 3600)
    m, sec = divmod(rem, 60)
    out = "PT"
    if h: out += f"{h}H"
    if m: out += f"{m}M"
    if sec or (h == 0 and m == 0): out += f"{sec}S"
    return out


def main():
    with open("corpus.json") as f:
        corpus = json.load(f)
    out = {"full": {}, "timeonly": {}}
    for r in corpus["full"]:
        out["full"][r["id"]] = serialize_b(parse_iso(r["wire"]))
    for r in corpus["timeonly"]:
        out["timeonly"][r["id"]] = native_canonical(r["wire"])
    print(json.dumps(out))


if __name__ == "__main__":
    main()

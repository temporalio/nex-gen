#!/usr/bin/env python3
"""Empirical probe: native ISO-8601 duration parsers are UNSUITABLE as the
source of truth for the `duration` format -- they either do not exist, parse a
different grammar, or disagree with RFC 3339 Appendix A (and with each other).

This drives one probe per language over a small value set and prints what each
stdlib facility does. See NOTES.md for the distilled findings. `ABNF` is the
strict RFC 3339 verdict our pinned regex produces.

Run: python3 probe.py
(java/dotnet/go/ruby optional; each section is skipped if its toolchain or a
build step is unavailable.)
"""
import subprocess
import sys
import tempfile
from pathlib import Path

VALUES = ["P1Y", "P1M", "PT1.5S", "-P1Y", "P-1Y", "P", "PT", "P1Y4D", "P1W", "P1YT1H", "P1D"]
ABNF = {"P1Y": True, "P1M": True, "PT1.5S": False, "-P1Y": False, "P-1Y": False,
        "P": False, "PT": False, "P1Y4D": False, "P1W": True, "P1YT1H": True, "P1D": True}


def sh(cmd, cwd=None):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)


def section(title):
    print("\n" + "=" * 70)
    print(title)
    print("=" * 70)


def python_probe():
    section("Python stdlib -- NO ISO-8601 duration parser")
    import datetime
    try:
        datetime.timedelta.fromisoformat("P1Y")
        print("  timedelta.fromisoformat('P1Y') unexpectedly parsed")
    except Exception as e:
        print(f"  timedelta.fromisoformat('P1Y') -> {type(e).__name__} "
              f"(it parses 'HH:MM:SS' timedeltas, not P... durations)")


def go_probe():
    section("Go -- time.ParseDuration parses '1h30m', NOT 'P...'")
    src = 'package main\nimport ("fmt";"time")\nfunc main(){for _,v:=range []string{' + \
        ",".join(f'"{v}"' for v in VALUES + ["1h30m"]) + \
        '}{_,e:=time.ParseDuration(v);if e!=nil{fmt.Printf("%-8s REJECT\\n",v)}else{fmt.Printf("%-8s OK\\n",v)}}}'
    with tempfile.TemporaryDirectory() as d:
        f = Path(d) / "p.go"
        f.write_text(src)
        r = sh(["go", "run", str(f)])
        print(r.stdout or r.stderr)


def java_probe():
    section("Java -- Duration.parse (no Y/M, +fractions) vs Period.parse (diverges)")
    src = "import java.time.*;public class P{public static void main(String[] a){String[] v={" + \
        ",".join(f'"{x}"' for x in VALUES) + \
        "};for(String s:v){String d,p;try{d=\"OK\";Duration.parse(s);}catch(Exception e){d=\"REJECT\";}" + \
        "try{p=\"OK\";Period.parse(s);}catch(Exception e){p=\"REJECT\";}" + \
        "System.out.printf(\"%-8s Duration=%-7s Period=%s%n\",s,d,p);}}}"
    with tempfile.TemporaryDirectory() as d:
        f = Path(d) / "P.java"
        f.write_text(src)
        r = sh(["java", str(f)])
        print(r.stdout or r.stderr)


def ruby_probe():
    section("Ruby -- NO stdlib ISO-8601 duration parser")
    r = sh(["ruby", "-rdate", "-e",
            'require "date"; ["P1Y","PT1.5S","P1W"].each{|v| puts "#{v}: Date._iso8601 => #{Date._iso8601(v).inspect} (empty hash = not parsed)"}'])
    print(r.stdout or r.stderr)


if __name__ == "__main__":
    print("strict-ABNF answers (what the pinned regex produces):")
    for v in VALUES:
        print(f"  {v:8} -> {ABNF[v]}")
    python_probe()
    for fn in (go_probe, java_probe, ruby_probe):
        try:
            fn()
        except Exception as e:
            print(f"  (skipped: {e})", file=sys.stderr)
    section(".NET -- XmlConvert.ToTimeSpan: +fractions, +sign, +P1Y4D, REJECTS P1W")
    print("  See NOTES.md; probe with a net8.0 console app calling")
    print("  System.Xml.XmlConvert.ToTimeSpan (rejects P1W, accepts PT1.5S/-P1Y/P1Y4D).")

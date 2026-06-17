# Probe: MATERIALIZE model (B) in Ruby via stdlib DateTime/Date. PROSPECTIVE.
# Parse each validated wire string, re-serialize via the GENERATOR-OWNED
# serializer (RFC 3339, original offset preserved with +00:00/-00:00 -> Z, T/Z
# uppercased on the parse path, fractional seconds at the value's own precision
# with trailing zeros trimmed) -- NO TRUNCATION. ruby runner.rb corpus.json
#
#   date-time -> DateTime  (offset + nanosecond preserved via Rational fraction)
#   date      -> Date      (YYYY-MM-DD, lossless)
#   time      -> UNSUPPORTED (Ruby has no time-of-day-only type; Time carries a date)
#
# FINDING: DateTime.rfc3339 SILENTLY CLAMPS leap :60 -> :59 (no error), like JS
# Temporal. The materialized grammar rejects :60 at VALIDATION; we model that
# with an explicit guard so the leap row is non-materializing, not corrupted.
require "json"
require "date"

ENGINE = "ruby"

def emit(o)
  puts JSON.generate(o)
end

def reject_leap(wire)
  raise "leap second :60 rejected by materialized grammar" if wire =~ /:60(\.\d+)?(Z|[+-]\d{2}:\d{2})?$/i
end

def frac_nanos(nanos)
  return "" if nanos.zero?
  "." + format("%09d", nanos).sub(/0+$/, "")
end

def offset_str(secs)
  return "Z" if secs.zero?
  sign = secs < 0 ? "-" : "+"
  secs = secs.abs
  format("%s%02d:%02d", sign, secs / 3600, (secs % 3600) / 60)
end

def canon_datetime(wire)
  reject_leap(wire)
  d = DateTime.rfc3339(wire.upcase) # preserves offset; requires an offset
  nanos = (d.sec_fraction * 1_000_000_000).to_i
  off = (d.offset * 86_400).to_i
  format("%04d-%02d-%02dT%02d:%02d:%02d%s%s",
         d.year, d.month, d.day, d.hour, d.min, d.sec,
         frac_nanos(nanos), offset_str(off))
end

def canon_date(wire)
  d = Date.iso8601(wire)
  format("%04d-%02d-%02d", d.year, d.month, d.day)
end

def canon_time(_wire)
  raise "UNSUPPORTED: Ruby has no time-of-day-only type"
end

def run(rows, fmt, fn)
  rows.each do |r|
    begin
      emit({ id: r["id"], engine: ENGINE, format: fmt, canonical: fn.call(r["wire"]), err: "" })
    rescue => e
      emit({ id: r["id"], engine: ENGINE, format: fmt, canonical: "", err: "#{e.class}: #{e.message}" })
    end
  end
end

c = JSON.parse(File.read(ARGV[0]))
run(c["date-time"], "date-time", method(:canon_datetime))
run(c["date"], "date", method(:canon_date))
run(c["time"], "time", method(:canon_time))

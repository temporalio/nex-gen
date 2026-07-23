# encoding: UTF-8
# frozen_string_literal: true
#
# Ruby runner for the JSON-Schema `format` conformance corpus (prospective target).
#
# Implements the SPEC'S PINNED CHECK: a pinned anchored regex compiled once, plus
# the shared integer-arithmetic calendar predicate for the temporal formats. This
# is the OWNED check -- we do NOT delegate to Date.parse / Time.parse as the
# source of truth. As a SECONDARY column we record what Ruby's native parser
# accepts, purely to document divergence.
#
# ANCHORING: Ruby's `^`/`$` are LINE anchors (multiline by default, no way to
# disable), so the pinned pattern uses `\A` (start of string) and `\z` (strict
# end of string, NO trailing-\n exception) -- the Ruby analogue of the `\z`/`\Z`
# other targets use. Ruby's `\d` is ASCII by default; the pinned classes here are
# explicit `[0-9]`/`[0-9a-fA-F]`, so no ASCII-mode flag is needed.
#
# Emits JSON Lines to stdout: {"id","engine":"ruby","valid":bool,"native":bool}
#
# Run: ruby runner.rb [corpus.json]

require "json"
require "date"
require "time"

OCTET = '(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])'
H16 = '[0-9a-fA-F]{1,4}'
V4 = "(#{OCTET}\\.#{OCTET}\\.#{OCTET}\\.#{OCTET})"
LS32 = "(#{H16}:#{H16}|#{V4})"
IPV6_BODY = "(" \
  "(#{H16}:){6}#{LS32}|" \
  "::(#{H16}:){5}#{LS32}|" \
  "(#{H16})?::(#{H16}:){4}#{LS32}|" \
  "((#{H16}:){0,1}#{H16})?::(#{H16}:){3}#{LS32}|" \
  "((#{H16}:){0,2}#{H16})?::(#{H16}:){2}#{LS32}|" \
  "((#{H16}:){0,3}#{H16})?::(#{H16}:)#{LS32}|" \
  "((#{H16}:){0,4}#{H16})?::#{LS32}|" \
  "((#{H16}:){0,5}#{H16})?::#{H16}|" \
  "((#{H16}:){0,6}#{H16})?::" \
  ")"

UUID_RE = /\A[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\z/
IPV4_RE = /\A#{OCTET}\.#{OCTET}\.#{OCTET}\.#{OCTET}\z/
IPV6_RE = /\A#{IPV6_BODY}\z/
DATE_RE = /\A([0-9]{4})-([0-9]{2})-([0-9]{2})\z/
TIME_RE = /\A([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})?\z/
DATE_TIME_RE = /\A([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})\z/

def leap?(y)
  (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
end

def days_in_month(y, m)
  case m
  when 1, 3, 5, 7, 8, 10, 12 then 31
  when 4, 6, 9, 11 then 30
  when 2 then leap?(y) ? 29 : 28
  else 0
  end
end

def valid_calendar_date(y, m, d)
  m >= 1 && m <= 12 && d >= 1 && d <= days_in_month(y, m)
end

def valid_time_fields(hh, mm, ss)
  hh <= 23 && mm <= 59 && ss <= 60 # :60 leap second accepted
end

def valid_offset(off)
  return true if off.nil? || off.empty? || off == "Z" || off == "z"

  off[1, 2].to_i <= 23 && off[4, 2].to_i <= 59
end

def pinned_valid(format, v)
  case format
  when "uuid" then !UUID_RE.match(v).nil?
  when "ipv4" then !IPV4_RE.match(v).nil?
  when "ipv6" then !IPV6_RE.match(v).nil?
  when "date"
    m = DATE_RE.match(v)
    !m.nil? && valid_calendar_date(m[1].to_i, m[2].to_i, m[3].to_i)
  when "time"
    m = TIME_RE.match(v)
    !m.nil? && valid_time_fields(m[1].to_i, m[2].to_i, m[3].to_i) && valid_offset(m[5])
  when "date-time"
    m = DATE_TIME_RE.match(v)
    !m.nil? && valid_calendar_date(m[1].to_i, m[2].to_i, m[3].to_i) &&
      valid_time_fields(m[4].to_i, m[5].to_i, m[6].to_i) && valid_offset(m[8])
  else
    false
  end
end

# SECONDARY: Ruby native parser (documentation only).
def native_valid(format, v)
  case format
  when "date"
    begin
      Date.iso8601(v)
      true
    rescue ArgumentError
      false
    end
  when "time", "date-time"
    begin
      Time.iso8601(v)
      true
    rescue ArgumentError
      false
    end
  else
    false # no native parser used for uuid/ipv4/ipv6 in this column
  end
end

def main
  path = ARGV[0] || "corpus.json"
  corpus = JSON.parse(File.read(path, encoding: "UTF-8"))
  corpus["pairs"].each do |p|
    $stdout.puts JSON.generate(
      "id" => p["id"],
      "engine" => "ruby",
      "valid" => pinned_valid(p["format"], p["value"]),
      "native" => native_valid(p["format"], p["value"])
    )
  end
end

main if __FILE__ == $PROGRAM_NAME

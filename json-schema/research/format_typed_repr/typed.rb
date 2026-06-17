# Probe: Ruby STANDARD LIBRARY typed reps for the 6 formats.
# Uses only stdlib requires (date, time, ipaddr, securerandom). Run: ruby typed.rb
# Backs features/format typed-repr research. Ruby is a PROSPECTIVE target.
require 'date'
require 'time'
require 'ipaddr'

def line(label, s)
  begin
    r = yield
    printf("  %-10s %-42s -> %s\n", label, s.inspect, r)
  rescue => e
    printf("  %-10s %-42s -> ERR %s: %s\n", label, s.inspect, e.class, e.message)
  end
end

puts "=== Ruby stdlib typed representations ==="

# ---- date-time : DateTime.rfc3339 / Time.iso8601 (require 'date' / 'time') ----
puts "\n[date-time] type=DateTime (or Time). ctor=DateTime.rfc3339(s) / Time.iso8601(s)"
[
  "2021-02-28T23:59:60Z",           # leap second
  "2006-01-02T15:04:05Z",
  "2006-01-02T15:04:05+00:00",
  "2006-01-02T15:04:05-00:00",
  "2006-01-02T15:04:05.123456789Z", # 9-digit
  "2006-01-02t15:04:05z",           # lowercase
  "2006-01-02T15:04:05",            # missing offset
  "2021-02-30T00:00:00Z",           # bad calendar
].each do |s|
  line("DateTime.rfc3339", s) { d = DateTime.rfc3339(s); "OK -> #{d.iso8601(9)}" }
end
puts "  -- Time.iso8601 --"
["2021-02-28T23:59:60Z", "2006-01-02T15:04:05", "2006-01-02T15:04:05+00:00"].each do |s|
  line("Time.iso8601", s) { t = Time.iso8601(s); "OK -> #{t.iso8601}" }
end

# ---- date : Date.iso8601 / Date.parse ----
puts "\n[date] type=Date  ctor=Date.iso8601(s) (strict) / Date.parse (lax)"
["2020-02-29", "2021-02-29", "2021-13-01"].each do |s|
  line("Date.iso8601", s) { "OK -> #{Date.iso8601(s).iso8601}" }
end

# ---- time : NO time-of-day-only stdlib type ----
puts "\n[time] NO time-of-day-only stdlib type. Time is a full instant; parsing '12:00:00' fills in today's date."
["12:00:00", "23:59:60Z"].each do |s|
  line("Time.parse", s) { Time.parse(s).iso8601 rescue (raise) }
end

# ---- uuid : NO stdlib UUID type. SecureRandom.uuid GENERATES a string only ----
puts "\n[uuid] NO stdlib UUID type. SecureRandom.uuid GENERATES a plain string; no parse/validate type."
require 'securerandom'
puts "  SecureRandom.uuid sample: #{SecureRandom.uuid} (a String, not a typed object)"

# ---- ipv4 / ipv6 : IPAddr (require 'ipaddr') ----
puts "\n[ipv4] type=IPAddr  ctor=IPAddr.new(s)"
["192.168.0.1", "256.0.0.1", "01.2.3.4", "1.2.3", "1.2.3.4.5"].each do |s|
  line("IPAddr", s) { a = IPAddr.new(s); "OK ipv4?=#{a.ipv4?} to_s=#{a.to_s}" }
end
puts "\n[ipv6] type=IPAddr  ctor=IPAddr.new(s)"
["::1", "2001:db8::1", "2001:DB8::1", "::ffff:192.168.0.1",
 "2001:0db8:0000:0000:0000:0000:0000:0001"].each do |s|
  line("IPAddr", s) { a = IPAddr.new(s); "OK ipv6?=#{a.ipv6?} to_s=#{a.to_s}" }
end

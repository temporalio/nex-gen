# Ruby materialization probe for the `duration` format (prospective target).
#
# Q1: Ruby stdlib has NO ISO-8601 duration type or parser. (ActiveSupport's
#     Duration is Rails, a dependency -> P4.) Date._iso8601 returns {} for P...
# Q2: design C native: there is NO stdlib fixed-duration type. Ruby could only
#     do design B (custom class) or keep string.
#
# Run: cd rb_full && ruby full.rb
require 'date'

puts "=== Q1: Ruby stdlib duration facilities ==="
puts "  Date._iso8601('P1Y') = #{Date._iso8601('P1Y').inspect}"   # {} - no parse
puts "  defined?(ActiveSupport::Duration) = #{defined?(ActiveSupport::Duration).inspect}" # nil (Rails only)
puts "  => No stdlib ISO-8601 duration type or parser.\n\n"

# design B custom class (mirrors the Go/Java struct + canonical serializer)
def parse_iso(s)
  d = {y:0,mo:0,w:0,d:0,h:0,mi:0,sec:0,week:false}
  body = s[1..]
  if body.start_with?('T')
    parse_time(body[1..], d); return d
  end
  if body.end_with?('W')
    d[:week] = true; d[:w] = body[0..-2].to_i; return d
  end
  date_part = body
  if (ti = body.index('T'))
    date_part = body[0...ti]; parse_time(body[(ti+1)..], d)
  end
  num = ''
  date_part.each_char do |c|
    if c =~ /[0-9]/ then num << c
    else
      v = num.to_i
      d[:y]=v if c=='Y'; d[:mo]=v if c=='M'; d[:d]=v if c=='D'
      num = ''
    end
  end
  d
end
def parse_time(t, d)
  num = ''
  t.each_char do |c|
    if c =~ /[0-9]/ then num << c
    else
      v = num.to_i
      d[:h]=v if c=='H'; d[:mi]=v if c=='M'; d[:sec]=v if c=='S'
      num = ''
    end
  end
end
def serialize(d)
  return "P#{d[:w]}W" if d[:week]
  date = ''; date << "#{d[:y]}Y" if d[:y]!=0; date << "#{d[:mo]}M" if d[:mo]!=0; date << "#{d[:d]}D" if d[:d]!=0
  tim = ''; tim << "#{d[:h]}H" if d[:h]!=0; tim << "#{d[:mi]}M" if d[:mi]!=0; tim << "#{d[:sec]}S" if d[:sec]!=0
  return "PT0S" if date.empty? && tim.empty?
  "P" + date + (tim.empty? ? '' : "T" + tim)
end

puts "=== Q2: design B custom class round-trip (full corpus) ==="
full = %w[P3Y6M4DT12H30M5S P1Y P2M P10D P4W P1W P1Y6M P1Y6M4D P6M4D P1YT1H P1DT12H P100Y200M300DT400H500M600S P0Y]
full.each do |w|
  got = serialize(parse_iso(w))
  expect = w == 'P0Y' ? 'PT0S' : w
  puts "  #{w.ljust(30)} -> #{got.ljust(20)} #{got == expect}"
end

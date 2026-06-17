# Emit canonical re-serialization JSON for the corpus (Ruby).
# No stdlib duration type -> BOTH groups use design B (custom); timeonly uses
# the same total-based canonical computed by hand.
require 'json'

def parse_iso(s)
  d = {y:0,mo:0,w:0,d:0,h:0,mi:0,s:0,week:false}
  body = s[1..]
  if body.start_with?('T') then pt(body[1..], d); return d end
  if body.end_with?('W') then d[:week]=true; d[:w]=body[0..-2].to_i; return d end
  date_part = body
  if (ti = body.index('T')) then date_part = body[0...ti]; pt(body[(ti+1)..], d) end
  num=''
  date_part.each_char do |c|
    if c =~ /[0-9]/ then num<<c
    else v=num.to_i; d[:y]=v if c=='Y'; d[:mo]=v if c=='M'; d[:d]=v if c=='D'; num='' end
  end
  d
end
def pt(t,d)
  num=''
  t.each_char do |c|
    if c =~ /[0-9]/ then num<<c
    else v=num.to_i; d[:h]=v if c=='H'; d[:mi]=v if c=='M'; d[:s]=v if c=='S'; num='' end
  end
end
def serialize_b(d)
  return "P#{d[:w]}W" if d[:week]
  date=''; date<<"#{d[:y]}Y" if d[:y]!=0; date<<"#{d[:mo]}M" if d[:mo]!=0; date<<"#{d[:d]}D" if d[:d]!=0
  tim=''; tim<<"#{d[:h]}H" if d[:h]!=0; tim<<"#{d[:mi]}M" if d[:mi]!=0; tim<<"#{d[:s]}S" if d[:s]!=0
  return "PT0S" if date.empty? && tim.empty?
  "P"+date+(tim.empty? ? '' : "T"+tim)
end
def native_canonical(s)
  d=parse_iso(s); total=d[:h]*3600+d[:mi]*60+d[:s]
  h=total/3600; m=(total%3600)/60; sec=total%60
  out="PT"; out<<"#{h}H" if h!=0; out<<"#{m}M" if m!=0; out<<"#{sec}S" if sec!=0||(h==0&&m==0); out
end

corpus = JSON.parse(File.read('corpus.json'))
out = {'full'=>{}, 'timeonly'=>{}}
corpus['full'].each { |r| out['full'][r['id']] = serialize_b(parse_iso(r['wire'])) }
corpus['timeonly'].each { |r| out['timeonly'][r['id']] = native_canonical(r['wire']) }
puts JSON.generate(out)

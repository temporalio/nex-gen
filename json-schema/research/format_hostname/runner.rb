# encoding: UTF-8
# frozen_string_literal: true
#
# Ruby runner for the JSON-Schema `hostname` format conformance corpus.
#
# Implements the PINNED generator-owned check for the (prospective) Ruby target:
#   1. a compiled Regexp. Ruby's `^`/`$` are LINE anchors with no non-multiline
#      mode, so we anchor with \A ... \z (start / strict end of STRING), exactly
#      the pattern-spec Ruby normalization. \d/\w are not used here (the class is
#      an explicit [A-Za-z0-9-]), and there is no \b, so no (?a) flag is needed.
#   2. a total-length guard (1..253 code points) OUTSIDE the regex. Ruby
#      String#length counts code points, so it agrees with the other targets.
#   Verdict = match? AND length-in-range. match? is unanchored search, fine
#   because the pattern is fully \A..\z anchored.
#
# Emits JSON Lines: {"id","engine":"ruby","valid","regex","len_ok"}
# Run: ruby runner.rb [corpus.json]

require "json"

HOST_RE = /\A[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*\z/
MAX_TOTAL_LEN = 253

def main
  path = ARGV[0] || "corpus.json"
  corpus = JSON.parse(File.read(path, encoding: "UTF-8"))
  corpus["cases"].each do |k|
    inst = k["instance"]
    n = inst.length # code points
    len_ok = n >= 1 && n <= MAX_TOTAL_LEN
    regex = HOST_RE.match?(inst)
    $stdout.puts JSON.generate("id" => k["id"], "engine" => "ruby",
                               "valid" => (regex && len_ok),
                               "regex" => regex, "len_ok" => len_ok)
  end
end

main if __FILE__ == $PROGRAM_NAME

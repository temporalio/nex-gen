# encoding: UTF-8
# frozen_string_literal: true
#
# Ruby runner for the PINNED `uri` check. Ruby's `^`/`$` are line anchors, so we
# anchor the body with \A...\z (string start / string end, no trailing-\n
# exception). Ruby's \d\w are ASCII by default; our body uses only explicit
# ASCII classes, so no (?a) flag is needed. Match with unanchored `match?`
# (the \A...\z anchors make it a full-string check).
#
# Emits JSON Lines: {"id","engine":"ruby","compiled":bool,"matched":bool|null}
# Run: ruby runner.rb [corpus.json] [pinned_body.json]
require "json"

def main
  corpus_path = ARGV[0] || "corpus.json"
  body_path = ARGV[1] || "pinned_body.json"
  corpus = JSON.parse(File.read(corpus_path, encoding: "UTF-8"))
  body = JSON.parse(File.read(body_path, encoding: "UTF-8"))["body"]

  compiled = false
  rx = nil
  begin
    rx = Regexp.new('\A' + body + '\z')
    compiled = true
  rescue RegexpError => e
    warn "RUBY COMPILE ERROR: #{e.message}"
  end

  corpus["pairs"].each do |p|
    matched = compiled ? rx.match?(p["value"]) : nil
    $stdout.puts JSON.generate("id" => p["id"], "engine" => "ruby",
                               "compiled" => compiled, "matched" => matched)
  end
end

main if __FILE__ == $PROGRAM_NAME

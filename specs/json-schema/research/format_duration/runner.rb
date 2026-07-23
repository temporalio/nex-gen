# encoding: UTF-8
# frozen_string_literal: true
#
# Ruby runner for the `duration` format conformance corpus.
#
# Ruby's regex engine is Onigmo. Two pinned-target adjustments matter for the
# fully-anchored pinned duration regex:
#
#   1. `^`/`$` are LINE anchors in Ruby with no non-multiline mode, so the
#      generator emits `\A` (start of string) for `^` and `\z` (end of string,
#      NO trailing-\n exception) for `$`. Without this, Ruby's `$` would accept
#      a trailing "\n" (the `newline-tail` case), diverging from Go/JS. The
#      pinned regex has exactly one leading `^` and one trailing `$`, so the
#      rewrite is exact.
#
#   2. `\d` is ASCII by default in Ruby (verified), so no ASCII-mode flag is
#      needed for the duration regex (it contains no `\b`, so the Onigmo
#      Unicode-`\b` quirk does not apply here).
#
# Runtime matching is unanchored `match?` (the anchors are in the pattern).
#
# Emits JSON Lines to stdout: {"id","engine":"ruby","compiled":bool,"matched":bool|null}
#
# Run: ruby runner.rb [corpus.json]

require "json"

def main
  path = ARGV[0] || "corpus.json"
  corpus = JSON.parse(File.read(path, encoding: "UTF-8"))
  pinned = corpus["pinned_regex"]

  # ^ -> \A , trailing $ -> \z  (the pinned regex's only anchors).
  emitted = pinned.dup
  emitted = "\\A" + emitted[1..] if emitted.start_with?("^")
  emitted = emitted[0..-2] + "\\z" if emitted.end_with?("$")

  compiled = false
  rx = nil
  begin
    rx = Regexp.new(emitted)
    compiled = true
  rescue RegexpError
    compiled = false
  end

  corpus["cases"].each do |c|
    matched = compiled ? rx.match?(c["value"]) : nil
    $stdout.puts JSON.generate("id" => c["id"], "engine" => "ruby",
                               "compiled" => compiled, "matched" => matched)
  end
end

main if __FILE__ == $PROGRAM_NAME

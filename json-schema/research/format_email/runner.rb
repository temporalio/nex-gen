# encoding: UTF-8
# frozen_string_literal: true
#
# Ruby runner for the pinned `email` regex conformance corpus.
#
# Ruby's engine is Onigmo. Per the `pattern` gate recipe for the (prospective)
# Ruby target:
#   * `^`/`$` are LINE anchors with no non-multiline mode, so normalize the
#     pattern source: `^` -> `\A`, `$` -> `\z` (strict end-of-string, no
#     trailing-\n exception; never `\Z`, which is the lenient one).
#   * `\d`/`\w`/`\s` are ASCII by default (unused here); `.` is a code point.
#   * `\b` is Unicode even when `\w` is ASCII, fixed by a leading `(?a)` flag.
#     The pinned email regex uses no `\b`, but we inject `(?a)` for recipe
#     fidelity -- it is a no-op on explicit classes.
#   * Runtime match is unanchored `match?`.
#
# Emits JSON Lines: {"id","engine":"ruby","compiled":bool,"matched":bool|null}
#
# Run: ruby runner.rb [corpus.json]

require "json"

def normalize_anchors(pattern)
  out = +""
  i = 0
  n = pattern.length
  in_class = false
  while i < n
    c = pattern[i]
    if c == "\\"
      out << c
      out << pattern[i + 1] if i + 1 < n
      i += 2
      next
    end
    if in_class
      out << c
      in_class = false if c == "]"
      i += 1
      next
    end
    case c
    when "["
      in_class = true
      out << c
    when "^"
      out << "\\A"
    when "$"
      out << "\\z"
    else
      out << c
    end
    i += 1
  end
  out
end

def main
  path = ARGV[0] || "corpus.json"
  corpus = JSON.parse(File.read(path, encoding: "UTF-8"))
  pattern = "(?a)" + normalize_anchors(corpus["pinned_regex"])
  compiled = false
  rx = nil
  begin
    rx = Regexp.new(pattern)
    compiled = true
  rescue RegexpError
    compiled = false
  end
  corpus["pairs"].each do |p|
    matched = compiled ? rx.match?(p["instance"]) : nil
    $stdout.puts JSON.generate("id" => p["id"], "engine" => "ruby",
                               "compiled" => compiled, "matched" => matched)
  end
end

main if __FILE__ == $PROGRAM_NAME

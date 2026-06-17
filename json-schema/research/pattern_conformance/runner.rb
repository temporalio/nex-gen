# encoding: UTF-8
# frozen_string_literal: true
#
# Ruby runner for the JSON-Schema `pattern` conformance corpus.
#
# Ruby's regex engine is Onigmo. It differs from the four pinned targets
# (Go RE2, JS, Python re, Java) on TWO axes that matter here:
#
#   1. `^`/`$` are LINE anchors in Ruby with NO way to disable multiline
#      behavior (there is no "not-multiline" mode). To reproduce the pinned
#      STRING-anchor semantics we must normalize the *pattern source*:
#        `^`  ->  `\A`   (start of string)
#        `$`  ->  `\z`   (end of string, NO trailing-\n exception)
#      Only unescaped, non-character-class `^`/`$` are rewritten.
#
#   2. Runtime matching is unanchored "search": `regexp.match?(str)`.
#
# Ruby's `\d`, `\w` are already ASCII by default (verified empirically), so no
# extra flag is needed for those. `\s` is ASCII too but ALSO includes `\v`
# (U+000B) -- a divergence noted in the report. `.` already matches a full
# code point (source is UTF-8) and does not match `\n` by default.
#
# One MORE wrinkle: Ruby's word-boundary `\b` uses a UNICODE word definition
# even though `\w`/`\d` are ASCII, so `\bfoo\b` on "éfoo" disagrees with the
# four pinned engines (which use an ASCII `\b`). Prefixing the pattern with the
# Onigmo ASCII-mode flag `(?a)` narrows `\b` to ASCII and fixes this without
# affecting `.` (still a full code point) or the already-ASCII `\d\w\s`.
#
# This runner emits, per corpus pair, THREE JSON-Lines records so the report can
# show the effect of each transform:
#   {"id","engine":"ruby-raw",         ...}  pattern verbatim, Ruby defaults
#   {"id","engine":"ruby-pinned",      ...}  ^->\A, $->\z only
#   {"id","engine":"ruby-ascii-pinned",...}  (?a) prefix + ^->\A, $->\z
# All use unanchored `match?`.
#
# Run: ruby runner.rb [corpus.json]

require "json"

# Rewrite unescaped, top-level `^` -> \A and `$` -> \z.
# Skips: escaped anchors (\^, \$), and anchors inside a character class [...].
# This is a source-level transform matching what a generator would emit for the
# Ruby target. It does not attempt to handle `(?x)` extended mode (not in gate).
def normalize_anchors(pattern)
  out = +""
  i = 0
  n = pattern.length
  in_class = false
  while i < n
    c = pattern[i]
    if c == "\\"
      # copy the escape and the following char verbatim
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

def emit(io, id, engine, pattern, instance)
  compiled = false
  matched = nil
  begin
    rx = Regexp.new(pattern)
    compiled = true
    matched = rx.match?(instance)
  rescue RegexpError
    compiled = false
    matched = nil
  end
  io.puts JSON.generate("id" => id, "engine" => engine,
                        "compiled" => compiled, "matched" => matched)
end

def main
  path = ARGV[0] || "corpus.json"
  corpus = JSON.parse(File.read(path, encoding: "UTF-8"))
  corpus["pairs"].each do |p|
    normalized = normalize_anchors(p["pattern"])
    emit($stdout, p["id"], "ruby-raw", p["pattern"], p["instance"])
    emit($stdout, p["id"], "ruby-pinned", normalized, p["instance"])
    # (?a) as a leading top-level flag applies ASCII mode to the whole pattern.
    emit($stdout, p["id"], "ruby-ascii-pinned", "(?a)" + normalized, p["instance"])
  end
end

main if __FILE__ == $PROGRAM_NAME

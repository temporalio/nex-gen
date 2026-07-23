# encoding: UTF-8
# frozen_string_literal: true
#
# Ruby NATIVE URI-parser probe. Ruby's `URI.parse` / `URI.regexp` uses an
# RFC-2396-derived grammar (with RFC3986 parser available). We report:
#   URI.parse succeeds AND the result is a URI::Generic with a non-nil scheme
#     AND it is absolute? => "valid absolute".
# `URI.parse` raises URI::InvalidURIError on many malformed inputs.
#
# Emits JSON Lines: {"id","engine":"ruby-native","valid":bool,"detail":string}
# Run: ruby native.rb ../native_inputs.json
require "json"
require "uri"

def main
  path = ARGV[0] || "../native_inputs.json"
  corpus = JSON.parse(File.read(path, encoding: "UTF-8"))
  corpus["inputs"].each do |inp|
    valid = false
    detail = ""
    begin
      u = URI.parse(inp["value"])
      if u.absolute?
        valid = true
        detail = "scheme=#{u.scheme}"
      else
        detail = "parsed but not absolute (scheme=#{u.scheme.inspect})"
      end
    rescue URI::InvalidURIError => e
      detail = "error: #{e.message}"
    rescue StandardError => e
      detail = "error(#{e.class}): #{e.message}"
    end
    $stdout.puts JSON.generate("id" => inp["id"], "engine" => "ruby-native",
                               "valid" => valid, "detail" => detail)
  end
end

main if __FILE__ == $PROGRAM_NAME

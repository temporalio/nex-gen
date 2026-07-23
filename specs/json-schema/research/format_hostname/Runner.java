// Java runner for the `hostname` format conformance corpus (single-file,
// JDK 21: `java Runner.java [corpus.json]`).
//
// Implements the PINNED generator-owned check for the Java target:
//   1. static compiled Pattern, DEFAULT flags (ASCII), fully anchored.
//      The end anchor is `\z` (Java's STRICT end-of-string anchor); plain `$`
//      in Java matches before a trailing '\n', which would diverge from Go/JS
//      -- the pattern-spec `$`->`\z` normalization for the Java target.
//   2. a total-length guard (1..=253 CODE POINTS) OUTSIDE the regex, via
//      String.codePointCount so astral input counts like the other targets.
//   Verdict = matcher(v).find() AND (length in range). find() is fine because
//   the pattern is fully anchored.
//
// Emits JSON Lines: {"id","engine":"java","valid","regex","len_ok"}
import java.io.IOException;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;

public class Runner {

    // labels of [A-Za-z0-9-], 1-63 chars, no leading/trailing hyphen, '.'-
    // separated, anchored with \A ... \z (strict end-of-input).
    static final Pattern HOST_RE = Pattern.compile(
        "\\A[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?"
        + "(?:\\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*\\z");
    static final int MAX_TOTAL_LEN = 253;

    public static void main(String[] args) throws IOException {
        String path = args.length > 0 ? args[0] : "corpus.json";
        String text = Files.readString(Path.of(path), StandardCharsets.UTF_8);
        Json parser = new Json(text);
        Object root = parser.parseValue();

        @SuppressWarnings("unchecked")
        java.util.Map<String, Object> obj = (java.util.Map<String, Object>) root;
        @SuppressWarnings("unchecked")
        List<Object> cases = (List<Object>) obj.get("cases");

        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);
        for (Object co : cases) {
            @SuppressWarnings("unchecked")
            java.util.Map<String, Object> k = (java.util.Map<String, Object>) co;
            String id = (String) k.get("id");
            String instance = (String) k.get("instance");

            int n = instance.codePointCount(0, instance.length());
            boolean lenOk = n >= 1 && n <= MAX_TOTAL_LEN;
            boolean regex = HOST_RE.matcher(instance).find();
            boolean valid = regex && lenOk;

            out.println("{\"id\":" + jstr(id)
                    + ",\"engine\":\"java\""
                    + ",\"valid\":" + valid
                    + ",\"regex\":" + regex
                    + ",\"len_ok\":" + lenOk
                    + "}");
        }
    }

    static String jstr(String s) {
        StringBuilder b = new StringBuilder("\"");
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> b.append("\\\"");
                case '\\' -> b.append("\\\\");
                case '\n' -> b.append("\\n");
                case '\r' -> b.append("\\r");
                case '\t' -> b.append("\\t");
                default -> {
                    if (c < 0x20) b.append(String.format("\\u%04x", (int) c));
                    else b.append(c);
                }
            }
        }
        return b.append('"').toString();
    }

    // ---- tiny recursive-descent JSON parser ----
    static final class Json {
        private final String s;
        private int i = 0;
        Json(String s) { this.s = s; }
        void skipWs() { while (i < s.length() && Character.isWhitespace(s.charAt(i))) i++; }
        Object parseValue() {
            skipWs();
            char c = s.charAt(i);
            return switch (c) {
                case '{' -> parseObject();
                case '[' -> parseArray();
                case '"' -> parseString();
                case 't', 'f' -> parseBool();
                case 'n' -> parseNull();
                default -> parseNumber();
            };
        }
        java.util.Map<String, Object> parseObject() {
            java.util.LinkedHashMap<String, Object> m = new java.util.LinkedHashMap<>();
            i++; skipWs();
            if (s.charAt(i) == '}') { i++; return m; }
            while (true) {
                skipWs();
                String key = parseString();
                skipWs(); i++; // :
                Object val = parseValue();
                m.put(key, val);
                skipWs();
                char c = s.charAt(i++);
                if (c == '}') break;
            }
            return m;
        }
        List<Object> parseArray() {
            List<Object> list = new ArrayList<>();
            i++; skipWs();
            if (s.charAt(i) == ']') { i++; return list; }
            while (true) {
                list.add(parseValue());
                skipWs();
                char c = s.charAt(i++);
                if (c == ']') break;
            }
            return list;
        }
        String parseString() {
            StringBuilder b = new StringBuilder();
            i++;
            while (true) {
                char c = s.charAt(i++);
                if (c == '"') break;
                if (c == '\\') {
                    char e = s.charAt(i++);
                    switch (e) {
                        case '"' -> b.append('"');
                        case '\\' -> b.append('\\');
                        case '/' -> b.append('/');
                        case 'b' -> b.append('\b');
                        case 'f' -> b.append('\f');
                        case 'n' -> b.append('\n');
                        case 'r' -> b.append('\r');
                        case 't' -> b.append('\t');
                        case 'u' -> {
                            int cp = Integer.parseInt(s.substring(i, i + 4), 16);
                            i += 4;
                            b.append((char) cp);
                        }
                        default -> throw new RuntimeException("bad escape \\" + e);
                    }
                } else {
                    b.append(c);
                }
            }
            return b.toString();
        }
        Boolean parseBool() {
            if (s.startsWith("true", i)) { i += 4; return Boolean.TRUE; }
            if (s.startsWith("false", i)) { i += 5; return Boolean.FALSE; }
            throw new RuntimeException("bad bool at " + i);
        }
        Object parseNull() {
            if (s.startsWith("null", i)) { i += 4; return null; }
            throw new RuntimeException("bad null at " + i);
        }
        Double parseNumber() {
            int start = i;
            while (i < s.length() && "+-0123456789.eE".indexOf(s.charAt(i)) >= 0) i++;
            return Double.parseDouble(s.substring(start, i));
        }
    }
}

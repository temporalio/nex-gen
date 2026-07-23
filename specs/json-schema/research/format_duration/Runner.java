// Java runner (single-file, JDK 21: `java Runner.java [corpus.json]`).
//
// Compiles the SINGLE generator-owned pinned regex (from corpus.json's
// `pinned_regex`) with Pattern.compile(p) (DEFAULT flags -> ASCII \d) and, for
// each corpus value, matcher(v).find() (UNANCHORED search).
//
// IMPORTANT -- the `$` normalization. The pinned regex ends in `$`. In Java,
// `$` matches at end-of-input OR just before a single trailing `\n` (verified
// by the `newline-tail` corpus case), so a raw `$` would ACCEPT "P1Y\n", which
// Go/JS reject. The `pattern` spec pins the fix: the generator emits `\z`
// (strict end-of-input, no trailing-\n exception) instead of `$` for the Java
// target. This runner applies that exact `$`->`\z` normalization to the pinned
// regex before compiling, so it tests the FORM the generator would emit. The
// pinned regex's only `$` is the trailing anchor, so a simple tail rewrite is
// exact here.
//
// Emits JSON Lines to stdout:
//   {"id","engine":"java","compiled":bool,"matched":bool|null}
import java.io.IOException;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;

public class Runner {

    public static void main(String[] args) throws IOException {
        String path = args.length > 0 ? args[0] : "corpus.json";
        String text = Files.readString(Path.of(path), StandardCharsets.UTF_8);
        Json parser = new Json(text);
        Object root = parser.parseValue();

        @SuppressWarnings("unchecked")
        java.util.Map<String, Object> obj = (java.util.Map<String, Object>) root;
        String pinned = (String) obj.get("pinned_regex");
        @SuppressWarnings("unchecked")
        List<Object> cases = (List<Object>) obj.get("cases");

        // $ -> \z normalization (Java's $ has a trailing-\n exception; \z does not).
        String emitted = pinned.endsWith("$") ? pinned.substring(0, pinned.length() - 1) + "\\z" : pinned;

        boolean compiled;
        Pattern re = null;
        try {
            re = Pattern.compile(emitted); // default flags -> ASCII \d
            compiled = true;
        } catch (PatternSyntaxException e) {
            compiled = false;
        }

        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);
        for (Object co : cases) {
            @SuppressWarnings("unchecked")
            java.util.Map<String, Object> c = (java.util.Map<String, Object>) co;
            String id = (String) c.get("id");
            String value = (String) c.get("value");
            Boolean matched = compiled ? re.matcher(value).find() : null; // unanchored search
            out.println("{\"id\":" + jstr(id)
                    + ",\"engine\":\"java\""
                    + ",\"compiled\":" + compiled
                    + ",\"matched\":" + (matched == null ? "null" : matched.toString())
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

        void skipWs() {
            while (i < s.length() && Character.isWhitespace(s.charAt(i))) i++;
        }

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
            i++; // {
            skipWs();
            if (s.charAt(i) == '}') { i++; return m; }
            while (true) {
                skipWs();
                String key = parseString();
                skipWs();
                i++; // :
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
            i++; // [
            skipWs();
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
            i++; // opening quote
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

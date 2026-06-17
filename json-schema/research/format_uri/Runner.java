// Java runner for the PINNED `uri` check (single-file, JDK 21).
// Anchors the body with \A...\z (Java \z = end of input, no trailing-\n
// exception; \Z is the LENIENT one). Default flags (ASCII classes). Matches with
// matcher(value).find() (the \A...\z anchors make it a full-input check).
//
// Emits JSON Lines: {"id","engine":"java","compiled":bool,"matched":bool|null}
// Run: java Runner.java [corpus.json] [pinned_body.json]
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
        String corpusPath = args.length > 0 ? args[0] : "corpus.json";
        String bodyPath = args.length > 1 ? args[1] : "pinned_body.json";

        String corpusText = Files.readString(Path.of(corpusPath), StandardCharsets.UTF_8);
        String bodyText = Files.readString(Path.of(bodyPath), StandardCharsets.UTF_8);

        @SuppressWarnings("unchecked")
        java.util.Map<String, Object> corpus =
                (java.util.Map<String, Object>) new Json(corpusText).parseValue();
        @SuppressWarnings("unchecked")
        List<Object> pairs = (List<Object>) corpus.get("pairs");
        @SuppressWarnings("unchecked")
        java.util.Map<String, Object> bodyObj =
                (java.util.Map<String, Object>) new Json(bodyText).parseValue();
        String body = (String) bodyObj.get("body");

        boolean compiled;
        Pattern re = null;
        try {
            re = Pattern.compile("\\A" + body + "\\z"); // default flags -> ASCII
            compiled = true;
        } catch (PatternSyntaxException e) {
            compiled = false;
            System.err.println("JAVA COMPILE ERROR: " + e.getMessage());
        }

        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);
        for (Object po : pairs) {
            @SuppressWarnings("unchecked")
            java.util.Map<String, Object> pair = (java.util.Map<String, Object>) po;
            String id = (String) pair.get("id");
            String value = (String) pair.get("value");
            String matched = compiled
                    ? Boolean.toString(re.matcher(value).find())
                    : "null";
            out.println("{\"id\":" + jstr(id)
                    + ",\"engine\":\"java\""
                    + ",\"compiled\":" + compiled
                    + ",\"matched\":" + matched
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
                skipWs(); i++;
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
                } else b.append(c);
            }
            return b.toString();
        }
        Boolean parseBool() {
            if (s.startsWith("true", i)) { i += 4; return Boolean.TRUE; }
            if (s.startsWith("false", i)) { i += 5; return Boolean.FALSE; }
            throw new RuntimeException("bad bool");
        }
        Object parseNull() {
            if (s.startsWith("null", i)) { i += 4; return null; }
            throw new RuntimeException("bad null");
        }
        Double parseNumber() {
            int start = i;
            while (i < s.length() && "+-0123456789.eE".indexOf(s.charAt(i)) >= 0) i++;
            return Double.parseDouble(s.substring(start, i));
        }
    }
}

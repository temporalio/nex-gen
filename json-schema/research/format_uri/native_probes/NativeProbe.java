// Java NATIVE URI-parser probe. Java has TWO parsers that DISAGREE:
//   - java.net.URI  (RFC 2396-ish, syntax parser)
//   - java.net.URL  (requires a known protocol handler)
// We report java.net.URI parse success AND isAbsolute() (has a scheme) as the
// "valid absolute" verdict for the URI parser, plus the URL verdict separately
// in `detail` to document the divergence.
//
// Emits JSON Lines: {"id","engine":"java-native","valid":bool,"detail":string}
// Run: java NativeProbe.java ../native_inputs.json
import java.io.IOException;
import java.io.PrintStream;
import java.net.URI;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

public class NativeProbe {
    public static void main(String[] args) throws IOException {
        String path = args.length > 0 ? args[0] : "../native_inputs.json";
        String text = Files.readString(Path.of(path), StandardCharsets.UTF_8);
        Json parser = new Json(text);
        @SuppressWarnings("unchecked")
        java.util.Map<String, Object> obj = (java.util.Map<String, Object>) parser.parseValue();
        @SuppressWarnings("unchecked")
        List<Object> inputs = (List<Object>) obj.get("inputs");

        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);
        for (Object io : inputs) {
            @SuppressWarnings("unchecked")
            java.util.Map<String, Object> in = (java.util.Map<String, Object>) io;
            String id = (String) in.get("id");
            String value = (String) in.get("value");

            boolean uriValid = false;
            String uriDetail;
            try {
                URI u = new URI(value);
                if (u.isAbsolute()) {
                    uriValid = true;
                    uriDetail = "URI-ok scheme=" + u.getScheme();
                } else {
                    uriDetail = "URI-ok but not absolute";
                }
            } catch (Exception e) {
                uriDetail = "URI-error: " + e.getClass().getSimpleName();
            }

            String urlDetail;
            try {
                @SuppressWarnings("deprecation")
                URL uu = new URL(value);
                urlDetail = "URL-ok proto=" + uu.getProtocol();
            } catch (Exception e) {
                urlDetail = "URL-error: " + e.getClass().getSimpleName();
            }

            out.println("{\"id\":" + jstr(id)
                    + ",\"engine\":\"java-native\""
                    + ",\"valid\":" + uriValid
                    + ",\"detail\":" + jstr(uriDetail + " | " + urlDetail)
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

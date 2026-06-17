// Java runner (single-file, JDK 21: `java Runner.java [corpus.json]`).
//
// Implements the SPEC'S PINNED CHECK: a pinned anchored regex compiled once
// (static final Pattern, default flags -> ASCII), plus the shared
// integer-arithmetic calendar predicate for the temporal formats. This is the
// OWNED check -- we do NOT delegate to LocalDate.parse / OffsetDateTime.parse
// as the source of truth. As a SECONDARY column we record what java.time's
// native parser accepts, purely to document divergence.
//
// NOTE on anchoring: Java's `$` (default flags) matches at end-of-input OR just
// before a final line terminator, so a trailing `\n` would slip through. The
// pinned pattern therefore ends in `\z` (strict end-of-input), the Java analogue
// of Python `\Z`, so all seven runtimes agree.
//
// Emits JSON Lines to stdout:
//   {"id","engine":"java","valid":bool,"native":bool}
//
// The JDK has no bundled JSON parser, so this file contains a small recursive-
// descent JSON parser sufficient for corpus.json.
import java.io.IOException;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.LocalDate;
import java.time.LocalTime;
import java.time.OffsetDateTime;
import java.time.OffsetTime;
import java.time.format.DateTimeParseException;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public class Runner {

    // ---- pinned patterns (anchored with \z, default flags = ASCII) ----------

    static final String OCTET = "(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])";
    static final String H16 = "[0-9a-fA-F]{1,4}";
    static final String V4 = "(" + OCTET + "\\." + OCTET + "\\." + OCTET + "\\." + OCTET + ")";
    static final String LS32 = "(" + H16 + ":" + H16 + "|" + V4 + ")";
    static final String IPV6 = "^("
            + "(" + H16 + ":){6}" + LS32 + "|"
            + "::(" + H16 + ":){5}" + LS32 + "|"
            + "(" + H16 + ")?::(" + H16 + ":){4}" + LS32 + "|"
            + "((" + H16 + ":){0,1}" + H16 + ")?::(" + H16 + ":){3}" + LS32 + "|"
            + "((" + H16 + ":){0,2}" + H16 + ")?::(" + H16 + ":){2}" + LS32 + "|"
            + "((" + H16 + ":){0,3}" + H16 + ")?::(" + H16 + ":)" + LS32 + "|"
            + "((" + H16 + ":){0,4}" + H16 + ")?::" + LS32 + "|"
            + "((" + H16 + ":){0,5}" + H16 + ")?::" + H16 + "|"
            + "((" + H16 + ":){0,6}" + H16 + ")?::"
            + ")\\z";

    static final Pattern UUID_RE = Pattern.compile(
            "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\\z");
    static final Pattern IPV4_RE = Pattern.compile(
            "^" + OCTET + "\\." + OCTET + "\\." + OCTET + "\\." + OCTET + "\\z");
    static final Pattern IPV6_RE = Pattern.compile(IPV6);
    static final Pattern DATE_RE = Pattern.compile("^([0-9]{4})-([0-9]{2})-([0-9]{2})\\z");
    static final Pattern TIME_RE = Pattern.compile(
            "^([0-9]{2}):([0-9]{2}):([0-9]{2})(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})?\\z");
    static final Pattern DATE_TIME_RE = Pattern.compile(
            "^([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})\\z");

    // ---- shared calendar predicate (integer arithmetic only) ----------------

    static boolean isLeap(int y) {
        return (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    }

    static int daysInMonth(int y, int m) {
        switch (m) {
            case 1: case 3: case 5: case 7: case 8: case 10: case 12: return 31;
            case 4: case 6: case 9: case 11: return 30;
            case 2: return isLeap(y) ? 29 : 28;
            default: return 0;
        }
    }

    static boolean validCalendarDate(int y, int m, int d) {
        return m >= 1 && m <= 12 && d >= 1 && d <= daysInMonth(y, m);
    }

    static boolean validTimeFields(int hh, int mm, int ss) {
        return hh <= 23 && mm <= 59 && ss <= 60; // :60 leap second accepted
    }

    static boolean validOffset(String off) {
        if (off == null || off.isEmpty() || off.equals("Z") || off.equals("z")) return true;
        int oh = Integer.parseInt(off.substring(1, 3));
        int om = Integer.parseInt(off.substring(4, 6));
        return oh <= 23 && om <= 59;
    }

    static int gi(Matcher m, int i) {
        String s = m.group(i);
        return s == null ? 0 : Integer.parseInt(s);
    }

    // ---- pinned per-format check --------------------------------------------

    static boolean pinnedValid(String format, String v) {
        switch (format) {
            case "uuid": return UUID_RE.matcher(v).matches();
            case "ipv4": return IPV4_RE.matcher(v).matches();
            case "ipv6": return IPV6_RE.matcher(v).matches();
            case "date": {
                Matcher m = DATE_RE.matcher(v);
                if (!m.matches()) return false;
                return validCalendarDate(gi(m, 1), gi(m, 2), gi(m, 3));
            }
            case "time": {
                Matcher m = TIME_RE.matcher(v);
                if (!m.matches()) return false;
                return validTimeFields(gi(m, 1), gi(m, 2), gi(m, 3)) && validOffset(m.group(5));
            }
            case "date-time": {
                Matcher m = DATE_TIME_RE.matcher(v);
                if (!m.matches()) return false;
                return validCalendarDate(gi(m, 1), gi(m, 2), gi(m, 3))
                        && validTimeFields(gi(m, 4), gi(m, 5), gi(m, 6))
                        && validOffset(m.group(8));
            }
        }
        return false;
    }

    // ---- SECONDARY: native java.time parser (documentation only) ------------

    static boolean nativeValid(String format, String v) {
        try {
            switch (format) {
                case "date": LocalDate.parse(v); return true;
                case "time":
                    try { OffsetTime.parse(v); return true; }
                    catch (DateTimeParseException e) { LocalTime.parse(v); return true; }
                case "date-time": OffsetDateTime.parse(v); return true;
                default: return false; // no java.time parser for uuid/ipv4/ipv6
            }
        } catch (DateTimeParseException e) {
            return false;
        }
    }

    public static void main(String[] args) throws IOException {
        String path = args.length > 0 ? args[0] : "corpus.json";
        String text = Files.readString(Path.of(path), StandardCharsets.UTF_8);
        Json parser = new Json(text);
        Object root = parser.parseValue();

        @SuppressWarnings("unchecked")
        java.util.Map<String, Object> obj = (java.util.Map<String, Object>) root;
        @SuppressWarnings("unchecked")
        List<Object> pairs = (List<Object>) obj.get("pairs");

        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);
        for (Object po : pairs) {
            @SuppressWarnings("unchecked")
            java.util.Map<String, Object> pair = (java.util.Map<String, Object>) po;
            String id = (String) pair.get("id");
            String format = (String) pair.get("format");
            String value = (String) pair.get("value");

            boolean valid = pinnedValid(format, value);
            boolean nat = nativeValid(format, value);
            out.println("{\"id\":" + jstr(id)
                    + ",\"engine\":\"java\""
                    + ",\"valid\":" + valid
                    + ",\"native\":" + nat
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

    // ---- tiny recursive-descent JSON parser ---------------------------------
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
            i++;
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
            i++;
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

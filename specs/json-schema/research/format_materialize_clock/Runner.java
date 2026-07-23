// Probe: MATERIALIZE model (B) in Java via java.time. Parse each validated wire
// string into the native construct, then re-serialize via the GENERATOR-OWNED
// serializer (RFC 3339, original offset preserved with +00:00/-00:00 -> Z, T/Z
// uppercased on the parse path, fractional seconds at the value's own precision
// with trailing zeros trimmed, no fractional part when zero) -- NO TRUNCATION.
//
//   date-time -> OffsetDateTime            offset + nanosecond preserved (lossless)
//   date      -> LocalDate                 YYYY-MM-DD, lossless
//   time      -> OffsetTime / LocalTime    offset preserved when present, lossless
//
// NOTE: java.time's native toString() emits fractional seconds in 3/6/9-digit
// groups (".500", ".250") and does NOT trim trailing zeros, so we do NOT use it;
// the serializer below is generator-owned to match Go/Python/TS byte-for-byte.
//
//   java Runner.java corpus.json
import java.nio.file.*;
import java.time.*;
import java.util.regex.*;

public class Runner {
    static final String ENGINE = "java";

    static void emit(String id, String fmt, String canonical, String err) {
        System.out.println("{\"id\":\"" + id + "\",\"engine\":\"" + ENGINE
            + "\",\"format\":\"" + fmt + "\",\"canonical\":\"" + esc(canonical)
            + "\",\"err\":\"" + esc(err) + "\"}");
    }
    static String esc(String s) {
        return s.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    // ".ddd" with trailing zeros trimmed, or "" when zero.
    static String fracNanos(int nanos) {
        if (nanos == 0) return "";
        String s = String.format("%09d", nanos).replaceAll("0+$", "");
        return "." + s;
    }

    // offset from a count of seconds: "Z" for zero, else "+HH:MM" / "-HH:MM".
    static String offsetStr(int secs) {
        if (secs == 0) return "Z";
        String sign = secs < 0 ? "-" : "+";
        secs = Math.abs(secs);
        return String.format("%s%02d:%02d", sign, secs / 3600, (secs % 3600) / 60);
    }

    // date-time -> OffsetDateTime (offset + nanoseconds retained; no atOffset(UTC),
    // no truncatedTo). Rejects leap :60.
    static String canonDateTime(String wire) {
        OffsetDateTime o = OffsetDateTime.parse(wire.toUpperCase());
        return String.format("%04d-%02d-%02dT%02d:%02d:%02d%s%s",
            o.getYear(), o.getMonthValue(), o.getDayOfMonth(),
            o.getHour(), o.getMinute(), o.getSecond(),
            fracNanos(o.getNano()), offsetStr(o.getOffset().getTotalSeconds()));
    }

    static String canonDate(String wire) {
        LocalDate d = LocalDate.parse(wire);
        return String.format("%04d-%02d-%02d", d.getYear(), d.getMonthValue(), d.getDayOfMonth());
    }

    // time -> OffsetTime when the wire carries an offset (RFC 3339 offset is
    // OPTIONAL), else LocalTime. Offset PRESERVED. Rejects leap :60.
    static String canonTime(String wire) {
        String w = wire.toUpperCase();
        int h, mi, s, nano, offSecs;
        boolean hasOffset;
        try {
            OffsetTime t = OffsetTime.parse(w);
            h = t.getHour(); mi = t.getMinute(); s = t.getSecond(); nano = t.getNano();
            offSecs = t.getOffset().getTotalSeconds();
            hasOffset = true;
        } catch (java.time.format.DateTimeParseException e) {
            LocalTime t = LocalTime.parse(w);
            h = t.getHour(); mi = t.getMinute(); s = t.getSecond(); nano = t.getNano();
            offSecs = 0;
            hasOffset = false;
        }
        return String.format("%02d:%02d:%02d%s%s", h, mi, s,
            fracNanos(nano), hasOffset ? offsetStr(offSecs) : "");
    }

    public static void main(String[] args) throws Exception {
        String txt = new String(Files.readAllBytes(Paths.get(args[0])));
        runArray(txt, "date-time", Runner::canonDateTime);
        runArray(txt, "date", Runner::canonDate);
        runArray(txt, "time", Runner::canonTime);
    }

    interface Fn { String apply(String s) throws Exception; }

    static void runArray(String txt, String fmt, Fn fn) {
        int key = txt.indexOf("\"" + fmt + "\"");
        int lb = txt.indexOf('[', key);
        int rb = txt.indexOf(']', lb);
        String block = txt.substring(lb, rb);
        Matcher m = Pattern.compile("\\{\\s*\"id\"\\s*:\\s*\"([^\"]*)\"\\s*,\\s*\"wire\"\\s*:\\s*\"([^\"]*)\"").matcher(block);
        while (m.find()) {
            String id = m.group(1), wire = m.group(2);
            try {
                emit(id, fmt, fn.apply(wire), "");
            } catch (Exception e) {
                emit(id, fmt, "", e.getClass().getSimpleName() + ": " + e.getMessage());
            }
        }
    }
}

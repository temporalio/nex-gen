// Probe: Java STANDARD LIBRARY typed reps for the 6 formats.
// JDK only (java.time, java.util.UUID, java.net). Run: java Typed.java (single-file mode)
// Backs features/format typed-repr research.
import java.time.*;
import java.time.format.DateTimeFormatter;
import java.util.UUID;
import java.net.InetAddress;
import java.net.Inet4Address;
import java.net.Inet6Address;

public class Typed {
    interface P { String run() throws Exception; }
    static void line(String label, String s, P p) {
        try { System.out.printf("  %-10s %-42s -> %s%n", label, "\"" + s + "\"", p.run()); }
        catch (Exception e) { System.out.printf("  %-10s %-42s -> ERR %s: %s%n", label, "\"" + s + "\"", e.getClass().getSimpleName(), e.getMessage()); }
    }

    public static void main(String[] args) {
        System.out.println("=== Java stdlib typed representations ===");

        // date-time : OffsetDateTime.parse (ISO_OFFSET_DATE_TIME)
        System.out.println("\n[date-time] type=java.time.OffsetDateTime  ctor=OffsetDateTime.parse(s)");
        for (String s : new String[]{
            "2021-02-28T23:59:60Z",            // leap second
            "2006-01-02T15:04:05Z",
            "2006-01-02T15:04:05+00:00",
            "2006-01-02T15:04:05-00:00",
            "2006-01-02T15:04:05.123456789Z",  // 9-digit
            "2006-01-02t15:04:05z",            // lowercase
            "2006-01-02T15:04:05",             // missing offset
            "2021-02-30T00:00:00Z",            // bad calendar
        }) {
            line("OffsetDT", s, () -> {
                OffsetDateTime d = OffsetDateTime.parse(s);
                return "OK toString=" + d.toString();
            });
        }

        // date : LocalDate.parse
        System.out.println("\n[date] type=java.time.LocalDate  ctor=LocalDate.parse(s)");
        for (String s : new String[]{"2020-02-29", "2021-02-29", "2021-13-01"})
            line("LocalDate", s, () -> "OK -> " + LocalDate.parse(s));

        // time : OffsetTime (RFC3339 time carries an offset) / LocalTime
        System.out.println("\n[time] type=java.time.OffsetTime (offset) or LocalTime (none)");
        for (String s : new String[]{"12:00:00Z", "23:59:60Z", "12:00:00.5+01:00"})
            line("OffsetTime", s, () -> "OK -> " + OffsetTime.parse(s));
        for (String s : new String[]{"12:00:00"})
            line("LocalTime", s, () -> "OK -> " + LocalTime.parse(s));

        // uuid : UUID.fromString
        System.out.println("\n[uuid] type=java.util.UUID  ctor=UUID.fromString(s)");
        for (String s : new String[]{
            "f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
            "F81D4FAE-7DEC-11D0-A765-00A0C91E6BF6",
            "f81d4fae7dec11d0a76500a0c91e6bf6",   // no dashes
            "1-2-3-4-5",                           // too-short groups (lax!)
            "not-a-uuid"})
            line("UUID", s, () -> "OK str=" + UUID.fromString(s));

        // ipv4 / ipv6 : InetAddress.getByName (WARNING: may do DNS) ; use InetAddress on literal
        System.out.println("\n[ipv4/ipv6] type=java.net.InetAddress (Inet4Address/Inet6Address). ctor=InetAddress.getByName(s) -- WARNING can trigger DNS for non-literals");
        for (String s : new String[]{"192.168.0.1", "256.0.0.1", "01.2.3.4", "1.2.3",
                                     "::1", "2001:db8::1", "2001:DB8::1", "::ffff:192.168.0.1"})
            line("InetAddr", s, () -> {
                InetAddress a = InetAddress.getByName(s);
                String fam = (a instanceof Inet4Address) ? "v4" : (a instanceof Inet6Address ? "v6" : "?");
                return "OK " + fam + " getHostAddress=" + a.getHostAddress();
            });
    }
}

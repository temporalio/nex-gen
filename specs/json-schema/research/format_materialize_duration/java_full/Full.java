// Java materialization probe for the `duration` format.
//
// Q1: does ANY single stdlib type hold the FULL grammar (incl. Y/M/W + H/M/S)?
//     - Duration: seconds+nanos, NO Y/M/W, and normalizes P1D->PT24H.
//     - Period:   Y/M/D only, NO H/M/S, expands P1W->P7D, allows negatives.
//     Neither alone holds P1Y1DT1H. Prove it.
// Q2: design C - narrowed time-only PTnHnMnS -> java.time.Duration. Does
//     Duration.toString() emit a form byte-equal to our canonical PTnHnMnS,
//     so it matches Go/Python? Test each time-only case AND non-canonical.
//
// Run: cd java_full && java Full.java
import java.time.Duration;
import java.time.Period;

public class Full {
    public static void main(String[] args) {
        System.out.println("=== Q1: no single stdlib type holds the full grammar ===");
        // Duration cannot parse Y or M.
        tryDuration("P1Y");           // reject
        tryDuration("P1M");           // reject
        tryDuration("P1Y1DT1H");      // reject (has Y)
        tryDuration("P4W");           // reject (Duration has no W)
        tryDuration("P1D");           // OK but normalizes to PT24H
        // Period cannot parse H/M/S.
        tryPeriod("P1YT1H");          // reject (has time)
        tryPeriod("P4W");             // OK but EXPANDS to P28D
        tryPeriod("P1W");             // OK but EXPANDS to P7D
        tryPeriod("-P1Y");            // OK - accepts negatives (grammar too loose)
        System.out.println("  => Duration lacks Y/M/W; Period lacks H/M/S and mangles W; neither holds P1Y1DT1H.\n");

        System.out.println("=== Q2: design C - time-only -> Duration, canonical re-emit ===");
        String[] timeonly = {"PT1H","PT30M","PT15S","PT1H30M15S","PT1H30M","PT30M15S","PT0S"};
        for (String w : timeonly) {
            Duration d = Duration.parse(w);
            String canon = canonical(d);
            String toStr = d.toString();
            System.out.printf("  %-12s -> Duration.toString()=%-12s | ourCanonical=%-12s | canon==input:%s | toString==input:%s%n",
                    w, toStr, canon, canon.equals(w), toStr.equals(w));
        }
        System.out.println("  non-canonical:");
        for (String w : new String[]{"PT90M","PT3600S","PT24H"}) {
            Duration d = Duration.parse(w);
            System.out.printf("  %-10s -> Duration.toString()=%-12s | ourCanonical=%-12s%n",
                    w, d.toString(), canonical(d));
        }
    }

    // Same canonical algorithm as the Go probe: PTnHnMnS from the total seconds.
    static String canonical(Duration d) {
        long total = d.getSeconds();
        long h = total / 3600;
        long m = (total % 3600) / 60;
        long s = total % 60;
        StringBuilder b = new StringBuilder("PT");
        if (h != 0) b.append(h).append('H');
        if (m != 0) b.append(m).append('M');
        if (s != 0 || (h == 0 && m == 0)) b.append(s).append('S');
        return b.toString();
    }

    static void tryDuration(String s) {
        try {
            Duration d = Duration.parse(s);
            System.out.printf("  Duration.parse(%-10s) = %s  (accepted, toString=%s)%n", s, d, d.toString());
        } catch (Exception e) {
            System.out.printf("  Duration.parse(%-10s) REJECTED (%s)%n", s, e.getClass().getSimpleName());
        }
    }
    static void tryPeriod(String s) {
        try {
            Period p = Period.parse(s);
            System.out.printf("  Period.parse(%-10s)   = %s  (accepted, toString=%s)%n", s, p, p.toString());
        } catch (Exception e) {
            System.out.printf("  Period.parse(%-10s)   REJECTED (%s)%n", s, e.getClass().getSimpleName());
        }
    }
}

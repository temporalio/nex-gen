// Emit canonical re-serialization JSON for the corpus (Java).
// design B custom components for `full`; NATIVE java.time.Duration for
// `timeonly` (proven byte-equal to our canonical, so we use Duration.toString()
// to demonstrate the native path materializes correctly).
//
// Reads ../corpus.json with a tiny regex (no JSON dep in single-file mode).
import java.nio.file.*;
import java.time.Duration;
import java.util.*;
import java.util.regex.*;

public class Emit {
    public static void main(String[] args) throws Exception {
        String txt = Files.readString(Path.of("../corpus.json"));
        Map<String, LinkedHashMap<String, String>> out = new LinkedHashMap<>();
        out.put("full", emitGroup(txt, "full", true));
        out.put("timeonly", emitGroup(txt, "timeonly", false));
        // hand-serialize to JSON
        StringBuilder sb = new StringBuilder("{");
        boolean firstG = true;
        for (var g : out.entrySet()) {
            if (!firstG) sb.append(","); firstG = false;
            sb.append("\"").append(g.getKey()).append("\":{");
            boolean first = true;
            for (var e : g.getValue().entrySet()) {
                if (!first) sb.append(","); first = false;
                sb.append("\"").append(e.getKey()).append("\":\"").append(e.getValue()).append("\"");
            }
            sb.append("}");
        }
        sb.append("}");
        System.out.println(sb);
    }

    // Extract the id/wire rows inside a named group array.
    static LinkedHashMap<String, String> emitGroup(String txt, String group, boolean useStruct) {
        LinkedHashMap<String, String> res = new LinkedHashMap<>();
        int gi = txt.indexOf("\"" + group + "\"");
        int start = txt.indexOf('[', gi);
        int end = txt.indexOf(']', start);
        String body = txt.substring(start, end);
        Matcher m = Pattern.compile("\\{[^}]*\"id\"\\s*:\\s*\"([^\"]+)\"[^}]*\"wire\"\\s*:\\s*\"([^\"]+)\"[^}]*\\}").matcher(body);
        while (m.find()) {
            String id = m.group(1), wire = m.group(2);
            res.put(id, useStruct ? serializeB(parseISO(wire)) : Duration.parse(wire).toString());
        }
        return res;
    }

    static long[] parseISO(String s) {
        // [y, mo, w, d, h, mi, s, week(0/1)]
        long[] c = new long[8];
        String body = s.substring(1);
        if (body.startsWith("T")) { parseTime(body.substring(1), c); return c; }
        if (body.endsWith("W")) { c[7] = 1; c[2] = Long.parseLong(body.substring(0, body.length()-1)); return c; }
        String datePart = body;
        int ti = body.indexOf('T');
        if (ti >= 0) { datePart = body.substring(0, ti); parseTime(body.substring(ti+1), c); }
        StringBuilder num = new StringBuilder();
        for (char ch : datePart.toCharArray()) {
            if (Character.isDigit(ch)) { num.append(ch); continue; }
            long v = Long.parseLong(num.toString());
            if (ch=='Y') c[0]=v; else if (ch=='M') c[1]=v; else if (ch=='D') c[3]=v;
            num.setLength(0);
        }
        return c;
    }
    static void parseTime(String t, long[] c) {
        StringBuilder num = new StringBuilder();
        for (char ch : t.toCharArray()) {
            if (Character.isDigit(ch)) { num.append(ch); continue; }
            long v = Long.parseLong(num.toString());
            if (ch=='H') c[4]=v; else if (ch=='M') c[5]=v; else if (ch=='S') c[6]=v;
            num.setLength(0);
        }
    }
    static String serializeB(long[] c) {
        if (c[7] == 1) return "P" + c[2] + "W";
        StringBuilder date = new StringBuilder(), tim = new StringBuilder();
        if (c[0]!=0) date.append(c[0]).append('Y');
        if (c[1]!=0) date.append(c[1]).append('M');
        if (c[3]!=0) date.append(c[3]).append('D');
        if (c[4]!=0) tim.append(c[4]).append('H');
        if (c[5]!=0) tim.append(c[5]).append('M');
        if (c[6]!=0) tim.append(c[6]).append('S');
        if (date.length()==0 && tim.length()==0) return "PT0S";
        return "P" + date + (tim.length()>0 ? "T" + tim : "");
    }
}

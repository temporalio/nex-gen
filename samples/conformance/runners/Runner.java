import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.SerializationFeature;
import com.fasterxml.jackson.datatype.jsr310.JavaTimeModule;
import java.io.File;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Generic cross-language conformance runner for generated <b>Java</b> models.
 *
 * <p>Driven by {@code tests/json_schema_conformance_manifest.rs} through a plan
 * file (protocol in {@code tests/toolchain/mod.rs}). Models are located by name
 * with {@code Class.forName} and driven through Jackson, which is what the
 * generator's {@code @JsonSerialize}/{@code @JsonDeserialize} annotations bind,
 * so a new conformance case needs no runner change.
 *
 * <p>Java 8 source level on purpose: the generated code targets that baseline
 * and this file is compiled in the same {@code javac --release 8} pass, so the
 * baseline is asserted rather than assumed.
 */
public final class Runner {

    /**
     * Configured the way Temporal's {@code DefaultDataConverter} configures its
     * mapper: the generated serializers hand {@code java.time} values inside
     * collections to Jackson's defaults, which without jsr310 registered either
     * throw or write a numeric array instead of an RFC 3339 string.
     */
    private static final ObjectMapper MAPPER = new ObjectMapper()
            .registerModule(new JavaTimeModule())
            .disable(SerializationFeature.WRITE_DATES_AS_TIMESTAMPS);

    private static final ObjectMapper CANONICAL =
            new ObjectMapper().configure(SerializationFeature.ORDER_MAP_ENTRIES_BY_KEYS, true);
    private static final Pattern SEGMENT = Pattern.compile("([A-Za-z0-9]+)((?:\\[\\d+\\])*)");
    private static final Pattern INDEX = Pattern.compile("\\[(\\d+)\\]");

    public static void main(String[] args) throws Exception {
        JsonNode plan = MAPPER.readTree(new File(args[0]));
        Map<String, Object> results = new LinkedHashMap<String, Object>();
        for (JsonNode testCase : plan.get("cases")) {
            String id = testCase.get("id").asText();
            Map<String, Object> probes = new LinkedHashMap<String, Object>();
            results.put(id, probes);
            Class<?> model;
            try {
                model = Class.forName(
                        "conformance." + testCase.get("dir").asText() + "."
                                + testCase.get("java_model").asText());
            } catch (Throwable error) {
                String message = "import failed: " + describe(error);
                for (JsonNode probe : testCase.get("probes")) {
                    probes.put(probe.get("id").asText(), error(message));
                }
                continue;
            }
            for (JsonNode probe : testCase.get("probes")) {
                probes.put(probe.get("id").asText(), runProbe(model, probe));
            }
        }
        Files.write(Paths.get(args[1]),
                CANONICAL.writerWithDefaultPrettyPrinter().writeValueAsBytes(results));
    }

    private static Map<String, Object> runProbe(Class<?> model, JsonNode probe) {
        Object value;
        try {
            value = MAPPER.readValue(probe.get("wire").asText(), model);
        } catch (Throwable error) {
            List<Map<String, String>> violations = violationsOf(error);
            return violations == null
                    ? error(describe(error))
                    : verdict("parse_rejected", violations);
        }
        if ("parse".equals(probe.get("kind").asText())) {
            return verdict("accepted", null);
        }
        JsonNode mutations = probe.get("mutations");
        if (mutations != null) {
            for (JsonNode mutation : mutations) {
                try {
                    applyMutation(value, mutation);
                } catch (Throwable error) {
                    return error("mutation failed: " + describe(error));
                }
            }
        }
        String serialized;
        try {
            serialized = MAPPER.writeValueAsString(value);
        } catch (Throwable error) {
            List<Map<String, String>> violations = violationsOf(error);
            return violations == null
                    ? error(describe(error))
                    : verdict("serialize_rejected", violations);
        }
        Map<String, Object> accepted = verdict("accepted", null);
        try {
            accepted.put("wire", CANONICAL.writeValueAsString(
                    CANONICAL.readValue(serialized, Object.class)));
        } catch (Throwable error) {
            accepted.put("wire", null);
            accepted.put("note", "output is not JSON: " + describe(error));
        }
        return accepted;
    }

    /** One step of a mutation path: a named member, or an array index. */
    private static final class Step {
        final String name;
        final int index;

        Step(String name, int index) {
            this.name = name;
            this.index = index;
        }
    }

    /** {@code a.b[0][1]} -> field a, field b, index 0, index 1. */
    private static List<Step> stepsOf(String path) {
        List<Step> steps = new ArrayList<Step>();
        for (String segment : path.split("\\.")) {
            Matcher matcher = SEGMENT.matcher(segment);
            if (!matcher.matches()) {
                throw new IllegalArgumentException("unparsable path segment " + segment);
            }
            steps.add(new Step(matcher.group(1), -1));
            Matcher indices = INDEX.matcher(matcher.group(2));
            while (indices.find()) {
                steps.add(new Step(null, Integer.parseInt(indices.group(1))));
            }
        }
        return steps;
    }

    private static Object read(Object owner, Step step) throws Exception {
        if (step.name == null) {
            return ((List<?>) owner).get(step.index);
        }
        return fieldOf(owner.getClass(), step.name).get(owner);
    }

    @SuppressWarnings("unchecked")
    private static void write(Object owner, Step step, Object value) throws Exception {
        if (step.name == null) {
            ((List<Object>) owner).set(step.index, value);
            return;
        }
        fieldOf(owner.getClass(), step.name).set(owner, value);
    }

    private static Class<?> slotType(Object owner, Step step) throws Exception {
        return step.name == null ? Long.class : fieldOf(owner.getClass(), step.name).getType();
    }

    @SuppressWarnings("unchecked")
    private static void applyMutation(Object model, JsonNode mutation) throws Exception {
        List<Step> steps = stepsOf(mutation.get("path").asText());
        Object owner = model;
        for (int index = 0; index < steps.size() - 1; index++) {
            owner = read(owner, steps.get(index));
        }
        Step last = steps.get(steps.size() - 1);
        if (mutation.has("duplicate_element")) {
            List<Object> sequence = (List<Object>) read(owner, last);
            sequence.add(sequence.get(mutation.get("duplicate_element").asInt()));
            return;
        }
        if (mutation.has("remove_array_element")) {
            List<Object> sequence = (List<Object>) read(owner, last);
            sequence.remove(mutation.get("remove_array_element").asInt());
            return;
        }
        if (mutation.has("put_map_entry")) {
            Map<String, Object> map = typedMap(read(owner, last));
            JsonNode entry = mutation.get("put_map_entry");
            Object replacement;
            if (!map.isEmpty()) {
                replacement = MAPPER.treeToValue(entry.get("value"), map.values().iterator().next().getClass());
            } else {
                replacement = MAPPER.treeToValue(entry.get("value"), Object.class);
            }
            map.put(entry.get("key").asText(), replacement);
            return;
        }
        if (mutation.has("remove_map_entry")) {
            typedMap(read(owner, last)).remove(mutation.get("remove_map_entry").asText());
            return;
        }
        Object replacement;
        if (mutation.has("set_integer")) {
            replacement = coerceInteger(slotType(owner, last), mutation.get("set_integer").asText());
        } else if (mutation.has("set_number")) {
            replacement = Double.valueOf(numberOf(mutation.get("set_number").asText()));
        } else if (mutation.has("set_string")) {
            replacement = mutation.get("set_string").asText();
        } else if (mutation.has("set_null")) {
            replacement = null;
        } else if (mutation.has("set_absent")) {
            replacement = null;
        } else if (mutation.has("set_bytes")) {
            JsonNode bytes = mutation.get("set_bytes");
            replacement = new byte[bytes.size()];
            for (int index = 0; index < bytes.size(); index++) {
                ((byte[]) replacement)[index] = (byte) bytes.get(index).asInt();
            }
        } else if (mutation.has("set_duration")) {
            JsonNode duration = mutation.get("set_duration");
            replacement = Duration.ofSeconds(
                    duration.get("seconds").asLong(), duration.get("nanoseconds").asLong());
        } else {
            throw new IllegalArgumentException("unknown mutation " + mutation);
        }
        write(owner, last, replacement);
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> typedMap(Object value) throws Exception {
        if (value instanceof Map<?, ?>) {
            return (Map<String, Object>) value;
        }
        return (Map<String, Object>) fieldOf(value.getClass(), "additionalProperties").get(value);
    }

    private static Object coerceInteger(Class<?> type, String text) {
        if (type == Integer.class || type == int.class) {
            return Integer.valueOf(text);
        }
        return Long.valueOf(text);
    }

    private static double numberOf(String spec) {
        if ("nan".equals(spec)) {
            return Double.NaN;
        }
        if ("inf".equals(spec)) {
            return Double.POSITIVE_INFINITY;
        }
        if ("-inf".equals(spec)) {
            return Double.NEGATIVE_INFINITY;
        }
        return Double.parseDouble(spec);
    }

    /**
     * The declared field a JSON property maps to. Conformance schemas use
     * lowerCamel ASCII property names, for which the generated Java member name
     * is the property name itself.
     */
    private static Field fieldOf(Class<?> type, String name) throws Exception {
        Field field = type.getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }

    private static List<Map<String, String>> violationsOf(Throwable error) {
        for (Throwable current = error; current != null; current = current.getCause()) {
            if (!current.getClass().getSimpleName().equals("ValidationException")) {
                continue;
            }
            try {
                Method accessor = current.getClass().getMethod("getViolations");
                List<Map<String, String>> out = new ArrayList<Map<String, String>>();
                for (Iterator<?> it = ((List<?>) accessor.invoke(current)).iterator();
                        it.hasNext(); ) {
                    Object violation = it.next();
                    Map<String, String> entry = new LinkedHashMap<String, String>();
                    entry.put("path", String.valueOf(
                            violation.getClass().getMethod("getPath").invoke(violation)));
                    entry.put("reason", String.valueOf(
                            violation.getClass().getMethod("getReason").invoke(violation)));
                    out.add(entry);
                }
                return out;
            } catch (Exception unreachable) {
                return null;
            }
        }
        return null;
    }

    private static Map<String, Object> verdict(String outcome, List<Map<String, String>> found) {
        Map<String, Object> out = new LinkedHashMap<String, Object>();
        out.put("outcome", outcome);
        if (found != null) {
            out.put("violations", found);
        }
        return out;
    }

    private static Map<String, Object> error(String message) {
        Map<String, Object> out = new LinkedHashMap<String, Object>();
        out.put("outcome", "error");
        out.put("message", message);
        return out;
    }

    private static String describe(Throwable error) {
        return error.getClass().getName() + ": " + error.getMessage();
    }

    private Runner() {}
}

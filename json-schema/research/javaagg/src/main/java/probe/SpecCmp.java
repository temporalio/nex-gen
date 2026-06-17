package probe;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.core.JsonToken;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.JsonDeserializer;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.exc.InvalidFormatException;
import java.io.IOException;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.util.ArrayList;
import java.util.List;

import probe.ValidationException.Violation;

/**
 * Side-by-side proof of the two candidate shapes for the spec-strict
 * integer primitive, both driven from inside a tree-walking collecting
 * deserializer (so both can aggregate).
 */
public class SpecCmp {

  static final long CAP = (1L << 53) - 1;

  // ---------- Option A: node-based static helper (the Go parseSpecInteger parallel) ----------
  // No throw. Reads a JsonNode, pushes a Violation on failure, returns null.
  static Long specLongA(JsonNode n, String path, List<Violation> errs) {
    if (!n.isNumber()) {
      errs.add(new Violation(path, "expected integer, got " + n.getNodeType()));
      return null;
    }
    BigDecimal d = n.decimalValue();
    if (d.stripTrailingZeros().scale() > 0) {
      errs.add(new Violation(path, "not an integer: " + n.asText()));
      return null;
    }
    if (d.abs().compareTo(BigDecimal.valueOf(CAP)) > 0) {
      errs.add(new Violation(path, "exceeds +/-(2^53-1) cap"));
      return null;
    }
    return d.longValueExact();
  }

  // ---------- Option B: retain the JsonDeserializer<Long>, drive it over a sub-parser ----------
  // The original token-based, *throwing* deserializer. Same code a per-field
  // @JsonDeserialize would have used.
  static final class SpecLongDeserializer extends JsonDeserializer<Long> {
    @Override
    public Long deserialize(JsonParser p, DeserializationContext ctxt) throws IOException {
      JsonToken t = p.currentToken();
      if (t == JsonToken.VALUE_NUMBER_INT) {
        BigInteger bi = p.getBigIntegerValue();
        if (bi.abs().compareTo(BigInteger.valueOf(CAP)) > 0)
          throw new InvalidFormatException(p, "exceeds +/-(2^53-1) cap", p.getText(), Long.class);
        return bi.longValueExact();
      }
      if (t == JsonToken.VALUE_NUMBER_FLOAT) {
        BigDecimal d = p.getDecimalValue();
        if (d.stripTrailingZeros().scale() > 0 || d.abs().compareTo(BigDecimal.valueOf(CAP)) > 0)
          throw new InvalidFormatException(p, "not an integer / cap", p.getText(), Long.class);
        return d.longValueExact();
      }
      throw new InvalidFormatException(p, "expected number token, got " + t, p.getText(), Long.class);
    }
  }

  static final SpecLongDeserializer DESER = new SpecLongDeserializer();

  // Adapter the collecting deserializer must write to bridge node -> parser -> catch -> Violation.
  static Long specLongB(JsonNode n, String path, List<Violation> errs,
                        JsonParser parent, DeserializationContext ctxt) {
    try (JsonParser sub = n.traverse(parent.getCodec())) {
      sub.nextToken();                       // advance onto the value token
      return DESER.deserialize(sub, ctxt);
    } catch (JsonProcessingException e) {
      errs.add(new Violation(path, e.getOriginalMessage()));   // throw -> Violation, per field
      return null;
    } catch (IOException e) {
      errs.add(new Violation(path, e.getMessage()));
      return null;
    }
  }

  public static void main(String[] args) throws Exception {
    ObjectMapper m = new ObjectMapper();
    String[] inputs = {"1", "1.0", "1e2", "1.5", "9007199254740993", "\"1\"", "true"};
    System.out.printf("%-22s %-28s %-28s%n", "input", "Option A (node helper)", "Option B (deser+subparser)");
    for (String in : inputs) {
      JsonNode n = m.readTree(in);
      List<Violation> ea = new ArrayList<>(), eb = new ArrayList<>();
      Long a = specLongA(n, "v", ea);
      // Option B needs a parent parser to borrow a codec from:
      JsonParser parent = m.getFactory().createParser("{}");
      parent.setCodec(m);
      Long b = specLongB(n, "v", eb, parent, null);
      System.out.printf("%-22s %-28s %-28s%n", in,
          ea.isEmpty() ? "OK " + a : "ERR " + ea.get(0).reason,
          eb.isEmpty() ? "OK " + b : "ERR " + eb.get(0).reason);
    }
  }
}

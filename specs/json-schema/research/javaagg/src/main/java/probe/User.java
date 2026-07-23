package probe;

import com.fasterxml.jackson.core.JsonGenerator;
import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.JsonDeserializer;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.JsonSerializer;
import com.fasterxml.jackson.databind.SerializerProvider;
import com.fasterxml.jackson.databind.annotation.JsonDeserialize;
import com.fasterxml.jackson.databind.annotation.JsonSerialize;
import java.io.IOException;
import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.List;

/**
 * A generated POJO. The aggregation mechanism is baked into the type via
 * class-level annotations that point at the model's OWN nested
 * (de)serializer classes — `User.Deserializer` / `User.Serializer`. They
 * travel with the model regardless of which ObjectMapper the Temporal
 * converter owns, and being nested they can't collide across models
 * (every model has its own `.Serializer` / `.Deserializer`).
 *
 *   id   : integer, REQUIRED, spec-strict (1.0 ok, 1.5 rejected), |x| <= 2^53-1
 *   name : string,  REQUIRED, non-null
 *   age  : integer, OPTIONAL, if present must be >= 0
 */
@JsonDeserialize(using = User.Deserializer.class)
@JsonSerialize(using = User.Serializer.class)
public class User {
  public long id;
  public String name;
  public Integer age;

  public User() {}

  public User(long id, String name, Integer age) {
    this.id = id;
    this.name = name;
    this.age = age;
  }

  @Override public String toString() {
    return "User{id=" + id + ", name=" + name + ", age=" + age + "}";
  }

  /** Two-stage lenient-tree-then-validate bind; the Go shadow-layout analog. */
  public static final class Deserializer extends JsonDeserializer<User> {
    private static final long CAP = (1L << 53) - 1;

    @Override
    public User deserialize(JsonParser p, DeserializationContext ctxt) throws IOException {
      JsonNode root = p.readValueAsTree();
      List<ValidationException.Violation> errs = new ArrayList<>();

      long id = 0;
      JsonNode idN = root.get("id");
      if (idN == null || idN.isNull()) {
        errs.add(new ValidationException.Violation("id", "required"));
      } else if (!idN.isNumber()) {
        errs.add(new ValidationException.Violation("id", "expected integer, got " + idN.getNodeType()));
      } else {
        BigDecimal d = idN.decimalValue();
        if (d.stripTrailingZeros().scale() > 0) {
          errs.add(new ValidationException.Violation("id", "not an integer: " + idN.asText()));
        } else if (d.abs().compareTo(BigDecimal.valueOf(CAP)) > 0) {
          errs.add(new ValidationException.Violation("id", "exceeds +/-(2^53-1) cap"));
        } else {
          id = d.longValueExact();
        }
      }

      String name = null;
      JsonNode nameN = root.get("name");
      if (nameN == null || nameN.isNull()) {
        errs.add(new ValidationException.Violation("name", "required"));
      } else if (!nameN.isTextual()) {
        errs.add(new ValidationException.Violation("name", "expected string, got " + nameN.getNodeType()));
      } else {
        name = nameN.asText();
      }

      Integer age = null;
      JsonNode ageN = root.get("age");
      if (ageN != null && !ageN.isNull()) {
        if (!ageN.isIntegralNumber()) {
          errs.add(new ValidationException.Violation("age", "expected integer"));
        } else if (ageN.asInt() < 0) {
          errs.add(new ValidationException.Violation("age", "must be >= 0, got " + ageN.asInt()));
        } else {
          age = ageN.asInt();
        }
      }

      if (!errs.isEmpty()) {
        throw new ValidationException(p, errs);
      }
      return new User(id, name, age);
    }
  }

  /** Serialize-side mirror (P17 / Java §6): validate-then-write. */
  public static final class Serializer extends JsonSerializer<User> {
    @Override
    public void serialize(User u, JsonGenerator gen, SerializerProvider sp) throws IOException {
      List<ValidationException.Violation> errs = new ArrayList<>();
      if (u.name == null) {
        errs.add(new ValidationException.Violation("name", "required (null in memory)"));
      }
      if (u.age != null && u.age < 0) {
        errs.add(new ValidationException.Violation("age", "must be >= 0, got " + u.age));
      }
      if (!errs.isEmpty()) {
        throw new ValidationException(gen, errs);
      }
      gen.writeStartObject();
      gen.writeNumberField("id", u.id);
      gen.writeStringField("name", u.name);
      if (u.age != null) {
        gen.writeNumberField("age", u.age);
      }
      gen.writeEndObject();
    }
  }
}

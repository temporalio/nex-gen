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
import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * An OPEN struct. A SECOND model with its own nested `OpenUser.Serializer`
 * / `OpenUser.Deserializer` — demonstrating the nested naming can't
 * collide across models. The catch-all map is populated/emitted by the
 * nested (de)serializer, NOT @JsonAnySetter/@JsonAnyGetter.
 */
@JsonDeserialize(using = OpenUser.Deserializer.class)
@JsonSerialize(using = OpenUser.Serializer.class)
public class OpenUser {
  public long id;
  public Map<String, Object> additionalProperties = new LinkedHashMap<>();

  public OpenUser() {}

  public OpenUser(long id, Map<String, Object> extras) {
    this.id = id;
    this.additionalProperties = extras;
  }

  @Override public String toString() {
    return "OpenUser{id=" + id + ", additionalProperties=" + additionalProperties + "}";
  }

  public static final class Deserializer extends JsonDeserializer<OpenUser> {
    private static final String DECLARED = "id";

    @Override
    public OpenUser deserialize(JsonParser p, DeserializationContext ctxt) throws IOException {
      JsonNode root = p.readValueAsTree();
      List<ValidationException.Violation> errs = new ArrayList<>();

      long id = 0;
      JsonNode idN = root.get("id");
      if (idN == null || idN.isNull()) {
        errs.add(new ValidationException.Violation("id", "required"));
      } else if (!idN.isIntegralNumber()) {
        errs.add(new ValidationException.Violation("id", "expected integer"));
      } else {
        id = idN.asLong();
      }

      Map<String, Object> extras = new LinkedHashMap<>();
      Iterator<Map.Entry<String, JsonNode>> it = root.fields();
      while (it.hasNext()) {
        Map.Entry<String, JsonNode> e = it.next();
        if (!DECLARED.equals(e.getKey())) {
          extras.put(e.getKey(), ctxt.readTreeAsValue(e.getValue(), Object.class));
        }
      }

      if (!errs.isEmpty()) {
        throw new ValidationException(p, errs);
      }
      return new OpenUser(id, extras);
    }
  }

  public static final class Serializer extends JsonSerializer<OpenUser> {
    @Override
    public void serialize(OpenUser u, JsonGenerator gen, SerializerProvider sp) throws IOException {
      gen.writeStartObject();
      gen.writeNumberField("id", u.id);
      if (u.additionalProperties != null) {
        for (Map.Entry<String, Object> e : u.additionalProperties.entrySet()) {
          gen.writeObjectField(e.getKey(), e.getValue());
        }
      }
      gen.writeEndObject();
    }
  }
}

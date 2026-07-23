package probe;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.core.JsonToken;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.DeserializationFeature;
import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.JsonDeserializer;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.deser.DeserializationProblemHandler;
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;

/**
 * Does a mapper-level DeserializationProblemHandler actually SEE our
 * spec/constraint violations? Plain Jackson binding into a POJO with NO
 * custom deserializer, with a recording handler attached. We watch which
 * hooks fire (if any) for the cases P8 must aggregate.
 */
public class HandlerProbe {

  public static class H {
    public long id;        // required, spec-strict integer
    public String name;    // required, non-null
  }

  static final List<String> fired = new ArrayList<>();

  static ObjectMapper mapperWithHandler() {
    ObjectMapper m = new ObjectMapper();
    m.addHandler(new DeserializationProblemHandler() {
      @Override public Object handleWeirdNumberValue(DeserializationContext c, Class<?> t, Number v, String msg) {
        fired.add("handleWeirdNumberValue(" + v + " -> " + t.getSimpleName() + ")");
        return 0L;
      }
      @Override public Object handleWeirdStringValue(DeserializationContext c, Class<?> t, String v, String msg) {
        fired.add("handleWeirdStringValue(\"" + v + "\" -> " + t.getSimpleName() + ")");
        return 0L;
      }
      @Override public Object handleUnexpectedToken(DeserializationContext c, JavaType t, JsonToken tok, JsonParser p, String msg) {
        fired.add("handleUnexpectedToken(" + tok + " -> " + t + ")");
        return null;
      }
      @Override public boolean handleUnknownProperty(DeserializationContext c, JsonParser p, JsonDeserializer<?> d, Object bean, String prop) throws IOException {
        fired.add("handleUnknownProperty(" + prop + ")");
        p.skipChildren();
        return true;
      }
    });
    return m;
  }

  public static void main(String[] args) throws Exception {
    String[] cases = {
        "{\"id\": 1.5, \"name\": \"ada\"}",                 // not-an-integer per spec
        "{\"id\": 9007199254740993, \"name\": \"ada\"}",    // exceeds 2^53-1 cap (still a valid long)
        "{\"name\": \"ada\"}",                              // required id MISSING
        "{\"id\": 1}",                                      // required name MISSING
        "{\"id\": 1, \"name\": \"ada\", \"extra\": true}",  // closed-struct extra key
        "{\"id\": \"abc\", \"name\": \"ada\"}",             // genuinely un-coercible
    };
    for (String json : cases) {
      fired.clear();
      ObjectMapper m = mapperWithHandler();
      // default FAIL_ON_UNKNOWN_PROPERTIES = true, so unknown-prop hook can fire
      System.out.println("in : " + json);
      try {
        H h = m.readValue(json, H.class);
        System.out.println("  bound : id=" + h.id + " name=" + h.name);
      } catch (Exception e) {
        System.out.println("  threw : " + e.getClass().getSimpleName());
      }
      System.out.println("  hooks fired : " + (fired.isEmpty() ? "(none)" : fired));
      System.out.println();
    }
  }
}

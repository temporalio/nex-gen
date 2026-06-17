package probe;

import io.temporal.api.common.v1.Payload;
import io.temporal.common.converter.DataConverter;
import io.temporal.common.converter.DefaultDataConverter;
import java.nio.charset.StandardCharsets;
import java.util.Optional;

/**
 * Drives the aggregation mechanism through the *default* Temporal Java
 * data converter (the one a Nexus/Workflow handler gets with no setup),
 * to prove the POJO-baked hook survives a converter whose ObjectMapper
 * we do not own.
 */
public class Main {

  static final DataConverter DC = DefaultDataConverter.STANDARD_INSTANCE;

  public static void main(String[] args) {
    System.out.println("Temporal default DataConverter: " + DC.getClass().getName());

    section("1. DESERIALIZE: three independent errors in one payload");
    // id non-integral (1.5), name missing, age negative -> expect 3 violations
    fromJson("{\"id\": 1.5, \"age\": -3}");

    section("2. DESERIALIZE: happy path, 1.0 accepted as integer (spec)");
    fromJson("{\"id\": 1.0, \"name\": \"ada\", \"age\": 30}");

    section("3. DESERIALIZE: wrong types aggregated (string id, number name)");
    fromJson("{\"id\": \"abc\", \"name\": 42}");

    section("4. SERIALIZE: round-trip a valid model through the converter");
    toJson(new User(7, "grace", null));

    section("5. SERIALIZE: invalid in-memory model fails loudly (P17/Java §6)");
    toJson(new User(7, null, -5));

    section("6. OPEN STRUCT: extras routed into catch-all by the collecting "
        + "deserializer (not @JsonAnySetter), round-trip + aggregate together");
    openRoundTrip("{\"id\": 5, \"x\": \"hi\", \"y\": [1,2], \"z\": {\"k\": true}}");
    System.out.println("  -- and a declared-field error still aggregates:");
    openFrom("{\"id\": 1.5, \"x\": \"hi\"}");
  }

  static void openRoundTrip(String json) {
    System.out.println("  in : " + json);
    Payload payload = jsonPayload(json);
    try {
      OpenUser u = DC.fromPayload(payload, OpenUser.class, OpenUser.class);
      System.out.println("  ok : " + u);
      String out = DC.toPayload(u).get().getData().toStringUtf8();
      System.out.println("  out: " + out + "   (extras preserved: "
          + (out.contains("\"x\"") && out.contains("\"y\"") && out.contains("\"z\"")) + ")");
    } catch (Exception e) {
      report(e);
    }
  }

  static void openFrom(String json) {
    System.out.println("  in : " + json);
    try {
      OpenUser u = DC.fromPayload(jsonPayload(json), OpenUser.class, OpenUser.class);
      System.out.println("  ok : " + u);
    } catch (Exception e) {
      report(e);
    }
  }

  static Payload jsonPayload(String json) {
    return Payload.newBuilder()
        .putMetadata("encoding", com.google.protobuf.ByteString.copyFromUtf8("json/plain"))
        .setData(com.google.protobuf.ByteString.copyFrom(json, StandardCharsets.UTF_8))
        .build();
  }

  static void fromJson(String json) {
    System.out.println("  in : " + json);
    Payload payload =
        Payload.newBuilder()
            .putMetadata("encoding", com.google.protobuf.ByteString.copyFromUtf8("json/plain"))
            .setData(com.google.protobuf.ByteString.copyFrom(json, StandardCharsets.UTF_8))
            .build();
    try {
      User u = DC.fromPayload(payload, User.class, User.class);
      System.out.println("  ok : " + u);
    } catch (Exception e) {
      report(e);
    }
  }

  static void toJson(User u) {
    System.out.println("  in : " + u);
    try {
      Optional<Payload> p = DC.toPayload(u);
      System.out.println("  ok : " + p.get().getData().toStringUtf8());
    } catch (Exception e) {
      report(e);
    }
  }

  /** Walk the cause chain to recover the aggregated violations. */
  static void report(Throwable e) {
    System.out.println("  threw: " + e.getClass().getName());
    Throwable t = e;
    while (t != null && !(t instanceof ValidationException)) t = t.getCause();
    if (t instanceof ValidationException) {
      ValidationException ve = (ValidationException) t;
      System.out.println("  -> ValidationException survived as cause, "
          + ve.getViolations().size() + " violation(s):");
      for (ValidationException.Violation v : ve.getViolations()) {
        System.out.println("       - " + v);
      }
    } else {
      System.out.println("  -> ValidationException NOT in cause chain: " + e.getMessage());
    }
  }

  static void section(String s) { System.out.println("\n=== " + s + " ==="); }
}

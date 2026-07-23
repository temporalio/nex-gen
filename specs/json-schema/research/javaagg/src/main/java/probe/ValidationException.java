package probe;

import com.fasterxml.jackson.databind.JsonMappingException;
import java.io.Closeable;
import java.util.ArrayList;
import java.util.List;

/**
 * Single-shot aggregation primitive (P8 analog for Java).
 *
 * Carries every {@link Violation} collected in one bind/validate pass.
 * Extends {@link JsonMappingException} (an IOException) so it propagates
 * out of a Jackson (de)serializer verbatim — Jackson does not wrap it —
 * and the Temporal converter then wraps it as the *cause* of a
 * DataConverterException. The Nexus handler walks the cause chain,
 * pulls the violations, and emits one BAD_REQUEST HandlerError.
 */
public class ValidationException extends JsonMappingException {

  public static final class Violation {
    public final String path;
    public final String reason;
    public Violation(String path, String reason) {
      this.path = path;
      this.reason = reason;
    }
    @Override public String toString() { return path + ": " + reason; }
  }

  private final List<Violation> violations;

  public ValidationException(Closeable processor, List<Violation> violations) {
    super(processor, render(violations));
    this.violations = violations;
  }

  public List<Violation> getViolations() { return violations; }

  private static String render(List<Violation> vs) {
    List<String> parts = new ArrayList<>();
    for (Violation v : vs) parts.add(v.toString());
    return vs.size() + " validation error(s): " + String.join("; ", parts);
  }
}

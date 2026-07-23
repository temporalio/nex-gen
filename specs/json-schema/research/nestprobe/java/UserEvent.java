import com.fasterxml.jackson.annotation.*;
public final class UserEvent {
    public static final class Kind {              // NESTED value class (UserEvent.Kind)
        public static final Kind USER = new Kind("user");
        private final String value;
        private Kind(String v) { this.value = v; }
        @JsonCreator public static Kind fromString(String v) {
            return "user".equals(v) ? USER : new Kind(v);
        }
        @JsonValue public String getValue() { return value; }
        public boolean isUnrecognized() { return this != USER; }
    }
    private final Kind kind;
    @JsonCreator public UserEvent(@JsonProperty("kind") Kind kind) { this.kind = kind; }
    public Kind getKind() { return kind; }
}

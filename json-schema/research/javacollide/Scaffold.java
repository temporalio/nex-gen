public final class Scaffold {
    public static final Scaffold VALUE = new Scaffold("v"); // enum value "value" -> VALUE
    private final String value;                              // instance field (lowercase)
    private Scaffold(String v) { this.value = v; }
    public String getValue() { return value; }
    public boolean isUnrecognized() { return false; }
}

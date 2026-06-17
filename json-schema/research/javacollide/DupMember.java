public final class DupMember {
    public static final DupMember USER = new DupMember("user");
    public static final DupMember USER = new DupMember("USER"); // "user" and "USER" both -> USER
    private final String value;
    private DupMember(String v) { this.value = v; }
}

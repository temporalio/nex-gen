import com.fasterxml.jackson.databind.ObjectMapper;
public class Main {
    public static void main(String[] a) throws Exception {
        ObjectMapper om = new ObjectMapper();
        UserEvent e = om.readValue("{\"kind\":\"user\"}", UserEvent.class);
        System.out.println("nested-class field: " + (e.getKind() == UserEvent.Kind.USER));
        System.out.println("roundtrip: " + om.writeValueAsString(e));
        System.out.println("top-level UserEventKind coexists: " + new UserEventKind().tag);
    }
}

package jsonschema;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.google.protobuf.ByteString;
import io.temporal.api.common.v1.Payload;
import io.temporal.common.converter.DataConverter;
import io.temporal.common.converter.DefaultDataConverter;
import io.temporal.failure.ApplicationFailure;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.LinkedHashMap;
import java.util.Map;
import org.junit.jupiter.api.Test;

import json_schema.definitions.chat.Labels;
import json_schema.definitions.chat.Message;
import json_schema.definitions.chat.Room;
import json_schema.definitions.chat.SendMessageInput;
import json_schema.definitions.chat.SendMessageOutput;
import json_schema.definitions.kb.content.block.Block;
import json_schema.definitions.kb.content.block.BlockStyle;
import json_schema.definitions.kb.content.page.Page;
import json_schema.definitions.kb.kb.GetCategoryTreeInput;
import json_schema.definitions.kb.kb.GetPageInput;
import json_schema.definitions.kb.kb.PutBlockOutput;
import json_schema.definitions.kb.tree.category.Category;

/**
 * Exercises the generated Java models through Temporal's default data
 * converter: every wire fixture deserializes into a POJO and re-serializes
 * back to the same JSON, and reference cases (constants, cross-file cycles,
 * open objects, typed maps) round-trip.
 *
 * <p>Equality is checked semantically (parsed JSON trees), mirroring the Python
 * suite's {@code json.loads(encoded) == load_fixture(name)}: the wire bytes from
 * {@code toPayload} are compact while the fixtures are pretty-printed, and Java
 * conservatively omits explicit nulls on optional+nullable fields (see
 * nullability.md). Fixtures carrying such nulls are therefore verified by
 * deserialization only.
 */
final class JsonSchemaRoundTripTest {

    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final DataConverter CONVERTER = DefaultDataConverter.newDefaultInstance();

    private static Path fixtureDir(String suite) {
        return Paths.get(System.getProperty("user.dir"), "..", "wire", "json_schema", suite)
                .normalize();
    }

    private static byte[] fixtureBytes(String suite, String name) throws IOException {
        return Files.readAllBytes(fixtureDir(suite).resolve(name));
    }

    private static Payload jsonPayload(byte[] data) {
        return Payload.newBuilder()
                .putMetadata("encoding", ByteString.copyFromUtf8("json/plain"))
                .setData(ByteString.copyFrom(data))
                .build();
    }

    private static <T> T decode(String suite, String name, Class<T> type) throws IOException {
        return CONVERTER.fromPayload(jsonPayload(fixtureBytes(suite, name)), type, type);
    }

    /** Deserialize a fixture, re-serialize the POJO, and assert JSON-tree equality. */
    private static <T> T roundTrip(String suite, String name, Class<T> type) throws IOException {
        T value = decode(suite, name, type);
        Payload encoded = CONVERTER.toPayload(value).orElseThrow(AssertionError::new);
        JsonNode got = MAPPER.readTree(encoded.getData().toByteArray());
        JsonNode want = MAPPER.readTree(fixtureBytes(suite, name));
        assertEquals(want, got, name);
        return value;
    }

    @Test
    void chatFixturesRoundTrip() throws IOException {
        Message minimal = roundTrip("chat", "message-minimal.json", Message.class);
        assertEquals("text", minimal.getKind().getValue());
        assertEquals("hi", minimal.getBody());
        assertNull(minimal.getReplyToId());
        assertNull(minimal.getPriority());
        assertEquals(0L, minimal.getPriorityOrDefault());
        assertEquals("text", Message.Kind.TEXT.getValue());

        // message-full carries replyToId: null (optional+nullable), which Java
        // collapses on the way out, so only deserialization is checked.
        Message full = decode("chat", "message-full.json", Message.class);
        assertNull(full.getReplyToId());
        assertNotNull(full.getPriority());
        assertEquals(7L, full.getPriority());

        Room room = roundTrip("chat", "room-open.json", Room.class);
        assertEquals("r1", room.getRoomId());
        assertNull(room.getTopic());
        assertNotNull(room.getMembers());
        assertEquals(1, room.getMembers().size());
        assertTrue(room.getAdditionalProperties().containsKey("x-extra"));
        assertEquals(42, room.getAdditionalProperties().get("x-extra").intValue());

        Labels labels = roundTrip("chat", "labels.json", Labels.class);
        assertEquals("prod", labels.getAdditionalProperties().get("env"));
        assertEquals("core", labels.getAdditionalProperties().get("team"));

        SendMessageInput input = roundTrip("chat", "send-message-input.json", SendMessageInput.class);
        assertEquals("r1", input.getRoomId());
        assertEquals("hi", input.getMessage().getBody());

        SendMessageOutput output = roundTrip("chat", "send-message-output.json", SendMessageOutput.class);
        assertEquals("m1", output.getMessageId());
    }

    @Test
    void kbFixturesRoundTrip() throws IOException {
        // page.json/block.json carry page: null (optional+nullable) which Java
        // collapses on serialize, so these are verified by deserialization.
        Page page = decode("kb", "page.json", Page.class);
        assertEquals("page-1", page.getPageId());
        assertNotNull(page.getBlocks());
        assertEquals("block-1", page.getBlocks().get(0).getBlockId());
        assertNull(page.getBlocks().get(0).getPage());
        assertNotNull(page.getBlocks().get(0).getStyle());
        assertTrue(page.getBlocks().get(0).getStyle().getBold());
        assertEquals("nexgen", page.getMeta().getAuthor());

        Block block = decode("kb", "block.json", Block.class);
        assertEquals("block-1", block.getBlockId());
        assertEquals(0L, block.getOrder());
        assertNull(block.getPage());
        assertNotNull(block.getStyle());
        assertTrue(block.getStyle().getBold());

        Category category = roundTrip("kb", "category-tree.json", Category.class);
        assertEquals("root", category.getId());
        assertNotNull(category.getChildren());
        assertEquals("child", category.getChildren().get(0).getId());

        GetPageInput getPage = roundTrip("kb", "get-page-input.json", GetPageInput.class);
        assertEquals("page-1", getPage.getPageId());

        GetCategoryTreeInput getTree =
                roundTrip("kb", "get-category-tree-input.json", GetCategoryTreeInput.class);
        assertEquals("root", getTree.getRootId());

        PutBlockOutput putBlock = roundTrip("kb", "put-block-output.json", PutBlockOutput.class);
        assertEquals("block-1", putBlock.getBlockId());
        assertEquals(7L, putBlock.getRevision());
    }

    @Test
    void constructedPojoSerializesToCanonicalWire() throws IOException {
        Map<String, String> values = new LinkedHashMap<>();
        values.put("env", "prod");
        values.put("team", "core");
        Labels labels = new Labels(values);
        Payload encoded = CONVERTER.toPayload(labels).orElseThrow(AssertionError::new);
        JsonNode got = MAPPER.readTree(encoded.getData().toByteArray());
        JsonNode want = MAPPER.readTree(fixtureBytes("chat", "labels.json"));
        assertEquals(want, got);
    }

    @Test
    void invalidValuesAggregateViolations() {
        // A wrong type on a closed object must surface as a validation failure.
        RuntimeException error = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{\"bold\":\"yes\"}".getBytes(java.nio.charset.StandardCharsets.UTF_8)),
                        BlockStyle.class,
                        BlockStyle.class));
        assertTrue(messageChain(error).contains("expected boolean"), messageChain(error));

        // An unknown key on a closed object is rejected.
        RuntimeException unknown = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{\"messageId\":\"m1\",\"nope\":1}".getBytes(java.nio.charset.StandardCharsets.UTF_8)),
                        SendMessageOutput.class,
                        SendMessageOutput.class));
        assertTrue(messageChain(unknown).contains("unknown field"), messageChain(unknown));

        RuntimeException escapedPaths = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{\"roomId\":\"r1\",\"message\":{"
                                + "\"kind\":\"text\",\"body\":\"hi\",\"a.b\":1},"
                                + "\"[0]\":1,\"quote\\\"slash\\\\\":1}")
                                .getBytes(java.nio.charset.StandardCharsets.UTF_8)),
                        SendMessageInput.class,
                        SendMessageInput.class));
        String escapedText = messageChain(escapedPaths);
        assertTrue(escapedText.contains("[\"[0]\"]"), escapedText);
        assertTrue(escapedText.contains("[\"quote\\\"slash\\\\\"]"), escapedText);
        assertTrue(escapedText.contains("message[\"a.b\"]"), escapedText);

        // A fractional value for an integer field is rejected.
        RuntimeException fractional = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{\"blockId\":\"b\",\"revision\":1.5}".getBytes(java.nio.charset.StandardCharsets.UTF_8)),
                        PutBlockOutput.class,
                        PutBlockOutput.class));
        assertFalse(messageChain(fractional).isEmpty());
        assertTrue(messageChain(fractional).contains("not an integer"), messageChain(fractional));
    }

    private static String messageChain(Throwable error) {
        StringBuilder builder = new StringBuilder();
        for (Throwable current = error; current != null; current = current.getCause()) {
            if (current instanceof ApplicationFailure) {
                ApplicationFailure failure = (ApplicationFailure) current;
                if ("PayloadValidationError".equals(failure.getType())
                        && failure.getDetails().getSize() > 0) {
                    builder.append(failure.getDetails().get(0, java.util.List.class)).append('\n');
                }
            }
            if (current.getMessage() != null) {
                builder.append(current.getMessage()).append('\n');
            }
        }
        return builder.toString();
    }
}

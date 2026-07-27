package jsonschema;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
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
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import org.junit.jupiter.api.Test;

import json_schema.definitions.showcase.Address;
import json_schema.definitions.showcase.Attributes;
import json_schema.definitions.showcase.Circle;
import json_schema.definitions.showcase.ContactJava;
import json_schema.definitions.showcase.Labels;
import json_schema.definitions.showcase.Settings;
import json_schema.definitions.showcase.Showcase;
import json_schema.definitions.showcase.Square;
import json_schema.definitions.showcase.Widget;

/**
 * Round-trips the showcase wire fixtures through Temporal's default data
 * converter. The showcase schema is a single pure JSON Schema file (no service)
 * exercising the whole supported keyword subset.
 *
 * <p>Equality is checked on parsed JSON trees (the {@code toPayload} bytes are
 * compact while the fixtures are pretty-printed). Java conservatively omits
 * explicit nulls on optional+nullable fields (see nullability.md), so
 * showcase-nulls.json (which carries {@code middleName: null}) is verified by
 * deserialization only.
 */
final class JsonSchemaShowcaseRoundTripTest {

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

    private static <T> T decode(String name, Class<T> type) throws IOException {
        return CONVERTER.fromPayload(jsonPayload(fixtureBytes("showcase", name)), type, type);
    }

    private static <T> T roundTrip(String name, Class<T> type) throws IOException {
        T value = decode(name, type);
        Payload encoded = CONVERTER.toPayload(value).orElseThrow(AssertionError::new);
        JsonNode got = MAPPER.readTree(encoded.getData().toByteArray());
        JsonNode want = MAPPER.readTree(fixtureBytes("showcase", name));
        assertEquals(want, got, name);
        return value;
    }

    @Test
    void showcaseFixturesRoundTrip() throws IOException {
        Showcase minimal = roundTrip("showcase-minimal.json", Showcase.class);
        assertEquals("showcase", minimal.getKind());
        assertEquals("showcase", Showcase.KIND);
        // Closed value-set fields (const on integer/boolean, enum on string/
        // integer/number) round-trip to their in-memory constants.
        assertEquals(1L, minimal.getRevision());
        assertEquals(1L, Showcase.REVISION_JAVA);
        assertTrue(minimal.getEnabled());
        assertEquals("active", minimal.getStatus());
        assertEquals(Showcase.ACTIVE_JAVA, minimal.getStatus());
        assertEquals(1L, minimal.getTier());
        assertEquals(1.5, minimal.getScale());
        assertEquals("Widget", minimal.getName());
        assertEquals(3L, minimal.getCount());
        assertTrue(minimal.getActive());
        assertNull(minimal.getRetries());
        assertEquals(3L, minimal.getRetriesOrDefault());
        // Scalar defaults of each kind: null field (unset on the wire), surfaced
        // on read via the generated getter default (materialize-on-read).
        assertNull(minimal.getGreeting());
        assertEquals("hello", minimal.getGreetingOrDefault());
        assertNull(minimal.getDebug());
        assertFalse(minimal.getDebugOrDefault());
        assertEquals("tools", minimal.getCategory());

        Showcase full = roundTrip("showcase-full.json", Showcase.class);
        assertEquals(42L, full.getCount());
        assertNotNull(full.getRetries());
        assertEquals(5L, full.getRetries());
        assertEquals("Q", full.getMiddleName());
        assertNotNull(full.getTags());
        assertEquals(2, full.getTags().size());
        assertEquals(java.util.Arrays.asList("alpha", "beta"), full.getAliases());
        assertEquals(java.util.Arrays.asList("admin", "user"), full.getRoles());
        assertNotNull(full.getAddress());
        assertEquals("1 Main St", full.getAddress().getStreet());
        assertTrue(full.getAddress().getAdditionalProperties().containsKey("region"));
        assertNotNull(full.getLabels());
        assertEquals("prod", full.getLabels().getValues().get("env"));
        assertNotNull(full.getSettings());
        assertEquals(14L, full.getSettings().getFontSize());

        // showcase-nulls carries middleName: null (optional+nullable), which Java
        // collapses on serialize, so only deserialization is checked.
        Showcase nulls = decode("showcase-nulls.json", Showcase.class);
        assertNull(nulls.getMiddleName());
        assertNull(nulls.getCategory());
        assertFalse(nulls.getActive());

        Address address = roundTrip("address-open.json", Address.class);
        assertEquals("1 Main St", address.getStreet());
        assertTrue(address.getAdditionalProperties().containsKey("x-extra"));

        Labels labels = roundTrip("labels.json", Labels.class);
        assertEquals("prod", labels.getValues().get("env"));
        assertEquals("core", labels.getValues().get("team"));

        Settings settings = roundTrip("settings.json", Settings.class);
        assertEquals("dark", settings.getTheme());
        assertEquals(14L, settings.getFontSize());

        // Numeric-constrained fields round-trip (integer bounds/multipleOf and a
        // number field with minimum + multipleOf).
        Showcase metrics = roundTrip("showcase-metrics.json", Showcase.class);
        assertEquals(5L, metrics.getPriority());
        assertEquals(2L, metrics.getLevel());
        assertEquals(15.0, metrics.getRatio());
        assertEquals(9L, metrics.getStep());

        // String-length-constrained fields round-trip. The astral crux: "a😀b"
        // is 3 code points but 6 UTF-8 bytes / 4 UTF-16 units; it passes code
        // (maxLength:5) only because length is counted in code points.
        Showcase strings = roundTrip("showcase-strings.json", Showcase.class);
        assertEquals("a😀b", strings.getCode());
        assertEquals("buddy", strings.getNickname());
    }

    @Test
    void invalidValuesAreRejected() {
        // Wrong const value on the root object.
        RuntimeException badConst = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(
                                ("{\"kind\":\"nope\",\"name\":\"w\",\"count\":1,\"active\":true,\"category\":null}")
                                        .getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertFalse(messageChain(badConst).isEmpty());

        // Unknown key on a closed object.
        RuntimeException unknown = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{\"theme\":\"dark\",\"nope\":1}".getBytes(StandardCharsets.UTF_8)),
                        Settings.class,
                        Settings.class));
        assertTrue(messageChain(unknown).contains("unknown field"), messageChain(unknown));

        // Closed value-set (const/enum) rejections with informative reasons.
        String closedBase =
                "\"kind\":\"showcase\",\"revision\":1,\"enabled\":true,\"status\":\"active\","
                        + "\"tier\":1,\"scale\":1.5,\"name\":\"w\",\"count\":1,\"active\":true,"
                        + "\"category\":\"tools\"";
        RuntimeException badRevision = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + closedBase + ",\"revision\":2}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badRevision).contains("must equal 1"), messageChain(badRevision));

        RuntimeException badStatus = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + closedBase + ",\"status\":\"archived\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(
                messageChain(badStatus).contains("must be one of [\"active\", \"inactive\", \"pending\"], got archived"),
                messageChain(badStatus));

        RuntimeException badTier = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + closedBase + ",\"tier\":9}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badTier).contains("must be one of [1, 2, 3], got 9"), messageChain(badTier));

        RuntimeException badScale = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + closedBase + ",\"scale\":3.5}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badScale).contains("must be one of [1.5, 2.5], got 3.5"), messageChain(badScale));

        // Numeric constraints fire at runtime with informative reasons.
        String base = "\"kind\":\"showcase\",\"name\":\"w\",\"count\":1,\"active\":true,\"category\":\"tools\"";
        RuntimeException tooBig = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"priority\":99}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(tooBig).contains("must be <= 10, got 99"), messageChain(tooBig));

        RuntimeException notMultiple = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"step\":7}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(notMultiple).contains("must be a multiple of 3, got 7"), messageChain(notMultiple));

        RuntimeException numberOffGrid = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"ratio\":7}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(numberOffGrid).contains("must be a multiple of 5, got 7"), messageChain(numberOffGrid));

        // String-length constraints fire at runtime, counted in code points.
        RuntimeException tooShort = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"code\":\"a\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(tooShort).contains("must have length >= 2, got 1"), messageChain(tooShort));

        RuntimeException tooLong = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"code\":\"abcdef\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(tooLong).contains("must have length <= 5, got 6"), messageChain(tooLong));

        // Astral: 6 emoji = 6 code points (24 bytes); rejected by code-point count.
        RuntimeException astral = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"code\":\"😀😀😀😀😀😀\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(astral).contains("must have length <= 5, got 6"), messageChain(astral));

        // Array constraints fire at runtime with informative reasons.
        RuntimeException tooFewItems = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"tags\":[]}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(tooFewItems).contains("must have at least 1 items, got 0"), messageChain(tooFewItems));

        RuntimeException tooManyItems = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"tags\":[\"a\",\"b\",\"c\",\"d\",\"e\",\"f\"]}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(tooManyItems).contains("must have at most 5 items, got 6"), messageChain(tooManyItems));

        RuntimeException dupItems = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"aliases\":[\"x\",\"x\"]}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(dupItems).contains("duplicate items: element at index 1 equals index 0"), messageChain(dupItems));

        RuntimeException missingContains = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"roles\":[\"user\"]}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(missingContains).contains("too few matching items: at least 1, got 0"), messageChain(missingContains));

        RuntimeException tooManyContains = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"roles\":[\"admin\",\"admin\",\"admin\"]}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(tooManyContains).contains("too many matching items: at most 2, got 3"), messageChain(tooManyContains));
    }

    @Test
    void patternConstraintsRoundTripAndReject() throws IOException {
        // sku `^[A-Z]{2,4}$` and phrase `^\S+\s\S+$` round-trip.
        Showcase patterns = roundTrip("showcase-patterns.json", Showcase.class);
        assertEquals("AB", patterns.getSku());
        assertEquals("hello world", patterns.getPhrase());

        String base = "\"kind\":\"showcase\",\"name\":\"w\",\"count\":1,\"active\":true,\"category\":\"tools\"";

        // Lowercase / too-long sku.
        RuntimeException badSkuCase = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"sku\":\"ab\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badSkuCase).contains("must match pattern"), messageChain(badSkuCase));

        RuntimeException longSku = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"sku\":\"ABCDE\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(longSku).contains("must match pattern"), messageChain(longSku));

        // phrase with no whitespace separator.
        RuntimeException noSep = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"phrase\":\"helloworld\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(noSep).contains("must match pattern"), messageChain(noSep));

        // `\s` ASCII-class crux: a NBSP (U+00A0) is NOT ASCII whitespace. The
        // loader normalized `\s`/`\S` to the explicit ASCII class, so Java's
        // default-flag matcher rejects it — consistent with Go/TS/Python.
        RuntimeException nbsp = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"phrase\":\"hello world\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(nbsp).contains("must match pattern"), messageChain(nbsp));

        // `$` end-anchor crux: a trailing newline is rejected. Java `$` matches
        // before a trailing `\n`; the loader rewrote `$`→`\z` (strict end), so
        // this rejects, consistent with Go/TS/Python.
        RuntimeException trailingNewline = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"phrase\":\"hello world\\n\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(trailingNewline).contains("must match pattern"), messageChain(trailingNewline));
        // (A valid sku/phrase is exercised by the showcase-patterns.json round-trip above.)
    }

    @Test
    void formatConstraintsRoundTripAndReject() throws IOException {
        // uuid/email/hostname/uri/ipv4 round-trip (string-typed, no materialization).
        Showcase formats = roundTrip("showcase-format.json", Showcase.class);
        assertEquals("de305d54-75b4-431b-adb2-eb6b9e546013", formats.getRequestId());
        assertEquals("user@example.com", formats.getContactEmail());
        assertEquals("api.example.com", formats.getHost());
        assertEquals("https://example.com/path?q=1#frag", formats.getHomepage());
        assertEquals("192.168.0.1", formats.getGateway());

        String base = "\"kind\":\"showcase\",\"name\":\"w\",\"count\":1,\"active\":true,\"category\":\"tools\"";

        // A malformed uuid.
        RuntimeException badUuid = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"requestId\":\"not-a-uuid\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badUuid).contains("must be a valid uuid, got not-a-uuid"), messageChain(badUuid));

        // Single-label email domain (user@localhost) is rejected.
        RuntimeException badEmail = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"contactEmail\":\"user@localhost\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badEmail).contains("must be a valid email, got user@localhost"), messageChain(badEmail));

        // ipv4 octet out of range.
        RuntimeException badIpv4 = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"gateway\":\"256.0.0.1\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badIpv4).contains("must be a valid ipv4, got 256.0.0.1"), messageChain(badIpv4));

        // uri with a double-`::` IPv6 IP-literal host (spliced ipv6 grammar rejects).
        RuntimeException badUri = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"homepage\":\"http://[1::2::3]\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badUri).contains("must be a valid uri"), messageChain(badUri));

        // An over-long hostname (> 253 code points) is rejected by the length guard.
        StringBuilder longHost = new StringBuilder();
        for (int i = 0; i < 64; i++) {
            if (i > 0) {
                longHost.append('.');
            }
            longHost.append("abc");
        }
        RuntimeException badHost = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"host\":\"" + longHost + "\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badHost).contains("must be a valid hostname"), messageChain(badHost));
    }

    @Test
    void contentEncodingRoundTripAndReject() throws IOException {
        // blob (base64) and urlBlob (base64url) round-trip: a JSON string on the
        // wire, native byte[] in the model, re-encoded byte-identically. The same
        // bytes (">>>") encode to "Pj4+" (padded standard) vs "Pj4-" (unpadded
        // URL-safe).
        Showcase bytes = roundTrip("showcase-bytes.json", Showcase.class);
        assertArrayEquals(">>>".getBytes(StandardCharsets.US_ASCII), bytes.getBlob());
        assertArrayEquals(">>>".getBytes(StandardCharsets.US_ASCII), bytes.getUrlBlob());

        String base = "\"kind\":\"showcase\",\"name\":\"w\",\"count\":1,\"active\":true,\"category\":\"tools\"";

        // A base64 field using the URL-safe alphabet is rejected by the pinned regex.
        RuntimeException urlUnderBase64 = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"blob\":\"Pj4-\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(urlUnderBase64).contains("must be base64-encoded")
                        && messageChain(urlUnderBase64).contains("Pj4-"),
                messageChain(urlUnderBase64));

        // A base64 field missing padding is rejected.
        RuntimeException unpadded = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"blob\":\"aGk\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(unpadded).contains("must be base64-encoded"), messageChain(unpadded));

        // A base64url field carrying padding is rejected.
        RuntimeException paddedUnderUrl = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(("{" + base + ",\"urlBlob\":\"aGk=\"}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(paddedUnderUrl).contains("must be base64url-encoded")
                        && messageChain(paddedUnderUrl).contains("aGk="),
                messageChain(paddedUnderUrl));
    }

    @Test
    void objectConstraintsRoundTripAndReject() throws IOException {
        // Valid map and object round-trip.
        Attributes attributes = roundTrip("attributes.json", Attributes.class);
        assertEquals("a", attributes.getValues().get("host"));
        assertEquals("8080", attributes.getValues().get("port"));

        ContactJava contact = roundTrip("contact.json", ContactJava.class);
        assertEquals("1 Main St", contact.getShippingStreet());
        assertEquals("90210", contact.getShippingZip());

        // minProperties:1 on a map — an empty object is too few (one number over
        // the distinct wire keys).
        RuntimeException tooFew = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{}".getBytes(StandardCharsets.UTF_8)),
                        Attributes.class,
                        Attributes.class));
        assertTrue(messageChain(tooFew).contains("must have at least 1 properties, got 0"), messageChain(tooFew));

        // maxProperties:3 on a map.
        RuntimeException tooMany = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{\"a\":\"1\",\"b\":\"2\",\"c\":\"3\",\"d\":\"4\"}".getBytes(StandardCharsets.UTF_8)),
                        Attributes.class,
                        Attributes.class));
        assertTrue(messageChain(tooMany).contains("must have at most 3 properties, got 4"), messageChain(tooMany));

        // propertyNames maxLength:8 — an over-long key (code-point length).
        RuntimeException badKey = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{\"toolongkey\":\"1\"}".getBytes(StandardCharsets.UTF_8)),
                        Attributes.class,
                        Attributes.class));
        assertTrue(
                messageChain(badKey).contains("invalid property name \"toolongkey\": must have length <= 8, got 10"),
                messageChain(badKey));

        // dependentRequired — a shipping street present without a shipping zip.
        RuntimeException missingDep = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{\"shippingStreet\":\"1 Main St\"}".getBytes(StandardCharsets.UTF_8)),
                        ContactJava.class,
                        ContactJava.class));
        assertTrue(
                messageChain(missingDep).contains("property \"shippingZip\" is required when \"shippingStreet\" is present"),
                messageChain(missingDep));

        // minProperties:1 on a declared-property object — an empty object.
        RuntimeException emptyContact = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{}".getBytes(StandardCharsets.UTF_8)),
                        ContactJava.class,
                        ContactJava.class));
        assertTrue(messageChain(emptyContact).contains("must have at least 1 properties, got 0"), messageChain(emptyContact));
    }

    @Test
    void allOfMergedWidgetRoundTripsAndEnforcesMergedBounds() throws IOException {
        // Widget is an allOf base-type extension (WidgetBase folded in + an
        // extension branch): a flat standalone object with the union of
        // properties ({id, kind, name, size}) and required ([id, name]).
        Widget widget = roundTrip("widget.json", Widget.class);
        assertEquals("w-1", widget.getId());
        assertEquals("gadget", widget.getKind());
        assertEquals("Widget One", widget.getName());
        assertNotNull(widget.getSize());
        assertEquals(15L, widget.getSize());

        // `size` carries a bound tightened from two allOf branches to [10, 20].
        RuntimeException tooSmall = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(
                                "{\"id\":\"w-1\",\"name\":\"Widget One\",\"size\":5}"
                                        .getBytes(StandardCharsets.UTF_8)),
                        Widget.class,
                        Widget.class));
        assertTrue(messageChain(tooSmall).contains("must be >= 10, got 5"), messageChain(tooSmall));

        RuntimeException tooBig = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(
                                "{\"id\":\"w-1\",\"name\":\"Widget One\",\"size\":25}"
                                        .getBytes(StandardCharsets.UTF_8)),
                        Widget.class,
                        Widget.class));
        assertTrue(messageChain(tooBig).contains("must be <= 20, got 25"), messageChain(tooBig));

        // A missing required member contributed by the extension branch.
        RuntimeException missingName = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload("{\"id\":\"w-1\"}".getBytes(StandardCharsets.UTF_8)),
                        Widget.class,
                        Widget.class));
        assertFalse(messageChain(missingName).isEmpty());
    }

    @Test
    void oneOfSumTypesRoundTripAndReject() throws IOException {
        // Disjoint-kind union (string | integer): each branch round-trips and is
        // selected by its wire token.
        Showcase asString = roundTrip("showcase-union-string.json", Showcase.class);
        assertTrue(asString.getIdOrName() instanceof Showcase.IdOrNameString);
        assertEquals("abc", ((Showcase.IdOrNameString) asString.getIdOrName()).getValue());
        Showcase asInt = roundTrip("showcase-union-int.json", Showcase.class);
        assertTrue(asInt.getIdOrName() instanceof Showcase.IdOrNameInteger);
        assertEquals(7L, ((Showcase.IdOrNameInteger) asInt.getIdOrName()).getValue());

        // Discriminated (tagged) union (Circle | Square) selected by `kind`.
        Showcase circle = roundTrip("showcase-shape-circle.json", Showcase.class);
        assertTrue(circle.getShape() instanceof Circle);
        assertEquals(2.5, ((Circle) circle.getShape()).getRadius());
        Showcase square = roundTrip("showcase-shape-square.json", Showcase.class);
        assertTrue(square.getShape() instanceof Square);
        assertEquals(4.0, ((Square) square.getShape()).getSide());

        String base =
                "{\"kind\":\"showcase\",\"revision\":1,\"enabled\":true,\"status\":\"active\","
                        + "\"tier\":1,\"scale\":1.5,\"name\":\"w\",\"count\":1,\"active\":true,"
                        + "\"category\":\"tools\"";

        // An unmatchable wire token (boolean) names the admissible kinds.
        RuntimeException badToken = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload((base + ",\"idOrName\":true}").getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badToken).contains("string, integer"), messageChain(badToken));

        // An unknown discriminator value is rejected (closed value set, P13.1).
        RuntimeException badTag = assertThrows(RuntimeException.class, () ->
                CONVERTER.fromPayload(
                        jsonPayload(
                                (base + ",\"shape\":{\"kind\":\"triangle\"}}")
                                        .getBytes(StandardCharsets.UTF_8)),
                        Showcase.class,
                        Showcase.class));
        assertTrue(messageChain(badTag).contains("triangle"), messageChain(badTag));
    }

    /**
     * Builds a Showcase with all required members valid, varying only the members
     * under serialize-side test; every other optional member is left unset.
     */
    private static Showcase showcaseWith(
            String status,
            long revision,
            String code,
            String sku,
            String requestId,
            Long priority,
            java.util.List<String> aliases) {
        return new Showcase(
                "showcase", revision, true, status, 1L, 1.5, "w", 1L, true,
                null, code, sku, null, requestId, null, null, null, null,
                null, null, null, null, null, null, null, null, "tools",
                priority, null, null, null, null, aliases, null, null, null,
                null, null, null, null, null);
    }

    /**
     * Serialize side (P12): constructing an in-memory model with an out-of-spec
     * value and serializing it (toPayload → the generated Serializer) is rejected
     * before any wire bytes are produced, with the same informative reason as the
     * deserializer. Mirrors the Go serialize-reject assertions.
     */
    @Test
    void invalidInMemoryValuesRejectedOnSerialize() {
        // A valid baseline serializes cleanly (no false rejection).
        Showcase valid = showcaseWith("active", 1L, null, null, null, null, null);
        CONVERTER.toPayload(valid).orElseThrow(AssertionError::new);

        // Numeric bound: an in-memory value past `maximum` fails to serialize.
        RuntimeException numeric = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(showcaseWith("active", 1L, null, null, null, 42L, null)));
        assertTrue(messageChain(numeric).contains("must be <= 10, got 42"), messageChain(numeric));

        // String length: an in-memory over-long string fails to serialize.
        RuntimeException length = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(showcaseWith("active", 1L, "abcdef", null, null, null, null)));
        assertTrue(messageChain(length).contains("must have length <= 5, got 6"), messageChain(length));

        // Pattern: an in-memory off-pattern value fails to serialize.
        RuntimeException pattern = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(showcaseWith("active", 1L, null, "xyz", null, null, null)));
        assertTrue(messageChain(pattern).contains("must match pattern"), messageChain(pattern));

        // Format: an in-memory malformed uuid fails to serialize.
        RuntimeException format = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(showcaseWith("active", 1L, null, null, "nope", null, null)));
        assertTrue(messageChain(format).contains("must be a valid uuid, got nope"), messageChain(format));

        // Array: an in-memory duplicate (uniqueItems) fails to serialize.
        RuntimeException array = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(showcaseWith("active", 1L, null, null, null, null,
                        java.util.Arrays.asList("dup", "dup"))));
        assertTrue(messageChain(array).contains("duplicate items: element at index 1 equals index 0"), messageChain(array));

        // Closed value-set: a mutated enum member fails to serialize.
        RuntimeException enumMember = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(showcaseWith("archived", 1L, null, null, null, null, null)));
        assertTrue(
                messageChain(enumMember).contains("must be one of [\"active\", \"inactive\", \"pending\"], got archived"),
                messageChain(enumMember));

        // const: a mutated integer const fails to serialize.
        RuntimeException constMember = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(showcaseWith("active", 2L, null, null, null, null, null)));
        assertTrue(messageChain(constMember).contains("must equal 1"), messageChain(constMember));

        // allOf-merged bound: an in-memory `size` past the tightened maximum fails.
        RuntimeException widget = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(new Widget(
                        "w-1", null, "Widget One", 25L, new java.util.LinkedHashMap<>())));
        assertTrue(messageChain(widget).contains("must be <= 20, got 25"), messageChain(widget));

        // Object dependentRequired: a shipping street with no zip fails to serialize.
        RuntimeException dep = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(new ContactJava(
                        null, "1 Main St", null, new java.util.LinkedHashMap<>())));
        assertTrue(
                messageChain(dep).contains("property \"shippingZip\" is required when \"shippingStreet\" is present"),
                messageChain(dep));

        // Object member-count: an empty map is below minProperties:1 on serialize.
        RuntimeException emptyMap = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(new Attributes(new java.util.LinkedHashMap<>())));
        assertTrue(messageChain(emptyMap).contains("must have at least 1 properties, got 0"), messageChain(emptyMap));

        // propertyNames key-shape: an over-long key fails to serialize.
        java.util.Map<String, String> longKey = new java.util.LinkedHashMap<>();
        longKey.put("toolongkey", "1");
        RuntimeException badKey = assertThrows(RuntimeException.class, () ->
                CONVERTER.toPayload(new Attributes(longKey)));
        assertTrue(
                messageChain(badKey).contains("invalid property name \"toolongkey\": must have length <= 8, got 10"),
                messageChain(badKey));
    }

    private static String messageChain(Throwable error) {
        StringBuilder builder = new StringBuilder();
        for (Throwable current = error; current != null; current = current.getCause()) {
            if (current.getMessage() != null) {
                builder.append(current.getMessage()).append('\n');
            }
        }
        return builder.toString();
    }
}

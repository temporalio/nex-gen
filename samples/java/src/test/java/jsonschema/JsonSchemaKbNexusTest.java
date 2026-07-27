package jsonschema;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.protobuf.ByteString;
import io.nexusrpc.handler.OperationHandler;
import io.nexusrpc.handler.OperationImpl;
import io.nexusrpc.handler.ServiceImpl;
import io.temporal.api.common.v1.Payload;
import io.temporal.client.WorkflowOptions;
import io.temporal.common.converter.DataConverter;
import io.temporal.common.converter.DefaultDataConverter;
import io.temporal.testing.TestWorkflowEnvironment;
import io.temporal.worker.Worker;
import io.temporal.workflow.NexusOperationOptions;
import io.temporal.workflow.NexusServiceOptions;
import io.temporal.workflow.Workflow;
import io.temporal.workflow.WorkflowInterface;
import io.temporal.workflow.WorkflowMethod;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import org.junit.jupiter.api.Test;

import json_schema.definitions.kb.content.block.Block;
import json_schema.definitions.kb.content.page.Page;
import json_schema.definitions.kb.kb.GetCategoryTreeInput;
import json_schema.definitions.kb.kb.GetPageInput;
import json_schema.definitions.kb.kb.KnowledgeBaseService;
import json_schema.definitions.kb.kb.PutBlockOutput;
import json_schema.definitions.kb.tree.category.Category;

/**
 * Drives the generated {@link KnowledgeBaseService} Nexus service definition
 * end-to-end over a real Temporal + Nexus endpoint. Unlike {@link
 * JsonSchemaRoundTripTest} (which round-trips fixtures through the data
 * converter), the caller workflow here calls every operation through the
 * Temporal SDK's built-in Nexus service stub — there is no generated API
 * client — exercising both the generated service/operation definitions and the
 * models over the wire in both directions.
 */
final class JsonSchemaKbNexusTest {

    private static final String ENDPOINT = "knowledge-base";
    private static final String TASK_QUEUE = "kb-nexus-task-queue";
    private static final DataConverter CONVERTER = DefaultDataConverter.newDefaultInstance();

    private static <T> T fixture(String name, Class<T> type) {
        try {
            Path dir = Paths.get(System.getProperty("user.dir"), "..", "wire", "json_schema", "kb")
                    .normalize();
            byte[] data = Files.readAllBytes(dir.resolve(name));
            Payload payload = Payload.newBuilder()
                    .putMetadata("encoding", ByteString.copyFromUtf8("json/plain"))
                    .setData(ByteString.copyFrom(data))
                    .build();
            return CONVERTER.fromPayload(payload, type, type);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /** Handler implementing the generated service interface, backed by wire fixtures. */
    @ServiceImpl(service = KnowledgeBaseService.class)
    public static final class KnowledgeBaseServiceImpl {
        final List<String> calls = Collections.synchronizedList(new ArrayList<>());

        @OperationImpl
        public OperationHandler<GetPageInput, Page> getPage() {
            return OperationHandler.sync((ctx, details, input) -> {
                calls.add("GetPage");
                assertEquals("page-1", input.getPageId());
                return fixture("page.json", Page.class);
            });
        }

        @OperationImpl
        public OperationHandler<Block, PutBlockOutput> putBlock() {
            return OperationHandler.sync((ctx, details, input) -> {
                calls.add("PutBlock");
                assertEquals("block-1", input.getBlockId());
                assertNotNull(input.getStyle());
                assertTrue(input.getStyle().getBold());
                return fixture("put-block-output.json", PutBlockOutput.class);
            });
        }

        @OperationImpl
        public OperationHandler<GetCategoryTreeInput, Category> getCategoryTree() {
            return OperationHandler.sync((ctx, details, input) -> {
                calls.add("GetCategoryTree");
                assertEquals("root", input.getRootId());
                return fixture("category-tree.json", Category.class);
            });
        }
    }

    /** Result the caller workflow returns for assertion on the client side. */
    public static final class KbResult {
        public String blockId;
        public String categoryChildId;
        public String pageId;
        public long revision;

        public KbResult() {}

        public KbResult(String blockId, String categoryChildId, String pageId, long revision) {
            this.blockId = blockId;
            this.categoryChildId = categoryChildId;
            this.pageId = pageId;
            this.revision = revision;
        }
    }

    @WorkflowInterface
    public interface KnowledgeBaseCaller {
        @WorkflowMethod
        KbResult run();
    }

    public static final class KnowledgeBaseCallerImpl implements KnowledgeBaseCaller {
        private final KnowledgeBaseService kb = Workflow.newNexusServiceStub(
                KnowledgeBaseService.class,
                NexusServiceOptions.newBuilder()
                        .setEndpoint(ENDPOINT)
                        .setOperationOptions(NexusOperationOptions.newBuilder()
                                .setScheduleToCloseTimeout(Duration.ofSeconds(10))
                                .build())
                        .build());

        @Override
        public KbResult run() {
            Page page = kb.getPage(new GetPageInput("page-1"));
            List<Block> blocks = page.getBlocks();
            if (blocks == null || blocks.isEmpty()) {
                throw new IllegalStateException("expected page block");
            }
            PutBlockOutput putBlockOutput = kb.putBlock(blocks.get(0));
            Category category = kb.getCategoryTree(new GetCategoryTreeInput("root"));
            String categoryChildId =
                    category.getChildren() != null && !category.getChildren().isEmpty()
                            ? category.getChildren().get(0).getId()
                            : null;
            return new KbResult(
                    putBlockOutput.getBlockId(),
                    categoryChildId,
                    page.getPageId(),
                    putBlockOutput.getRevision());
        }
    }

    @Test
    void kbOperationsUseRealNexusClient() {
        TestWorkflowEnvironment env = TestWorkflowEnvironment.newInstance();
        try {
            Worker worker = env.newWorker(TASK_QUEUE);
            worker.registerWorkflowImplementationTypes(KnowledgeBaseCallerImpl.class);
            KnowledgeBaseServiceImpl handler = new KnowledgeBaseServiceImpl();
            worker.registerNexusServiceImplementation(handler);
            env.createNexusEndpoint(ENDPOINT, TASK_QUEUE);
            env.start();

            KnowledgeBaseCaller workflow = env.getWorkflowClient().newWorkflowStub(
                    KnowledgeBaseCaller.class,
                    WorkflowOptions.newBuilder().setTaskQueue(TASK_QUEUE).build());
            KbResult result = workflow.run();

            assertEquals("block-1", result.blockId);
            assertEquals("child", result.categoryChildId);
            assertEquals("page-1", result.pageId);
            assertEquals(7L, result.revision);
            assertEquals(Arrays.asList("GetPage", "PutBlock", "GetCategoryTree"), handler.calls);
        } finally {
            env.close();
        }
    }
}

from __future__ import annotations

from collections.abc import AsyncIterator, Sequence
from typing import Any, cast

import pytest
import pytest_asyncio
import temporalio.api.common.v1
import temporalio.nexus.system
from temporalio.client import Client
from temporalio.converter import (
    DataConverter,
    DefaultPayloadConverter,
    PayloadConverter,
    TemporalIntermediatePayloadConverter,
)
from temporalio.testing import WorkflowEnvironment
from typing_extensions import override


class TemporalModelPayloadConverter(TemporalIntermediatePayloadConverter):
    _inner_payload_converter: PayloadConverter

    def __init__(self, inner_payload_converter: PayloadConverter | None = None) -> None:
        super().__init__(inner_payload_converter or DefaultPayloadConverter())

    @override
    def to_payloads(
        self, values: Sequence[Any]
    ) -> list[temporalio.api.common.v1.Payload]:
        with temporalio.nexus.system.user_payload_converter_context(
            self._inner_payload_converter
        ):
            return super().to_payloads(values)

    @override
    def from_payloads(
        self,
        payloads: Sequence[temporalio.api.common.v1.Payload],
        type_hints: list[type] | None = None,
    ) -> list[Any]:
        with temporalio.nexus.system.user_payload_converter_context(
            self._inner_payload_converter
        ):
            return super().from_payloads(payloads, type_hints)


temporal_model_data_converter = DataConverter(
    payload_converter_class=TemporalModelPayloadConverter
)


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addoption(
        "-E",
        "--workflow-environment",
        default="local",
        help=(
            "Workflow environment to use: 'local' to start a dev server, "
            "or host:port for an existing Temporal server"
        ),
    )


@pytest.fixture(scope="session")
def env_type(request: pytest.FixtureRequest) -> str:
    option = cast(object, request.config.getoption("--workflow-environment"))
    if not isinstance(option, str):
        raise TypeError("--workflow-environment must be a string")
    return option


@pytest_asyncio.fixture(scope="session")
async def env(env_type: str) -> AsyncIterator[WorkflowEnvironment]:
    if env_type == "local":
        workflow_environment = await WorkflowEnvironment.start_local(  # pyright: ignore[reportUnknownMemberType]
            data_converter=temporal_model_data_converter
        )
    else:
        workflow_environment = WorkflowEnvironment.from_client(
            await Client.connect(
                env_type,
                data_converter=temporal_model_data_converter,
            )
        )

    try:
        yield workflow_environment
    finally:
        await workflow_environment.shutdown()

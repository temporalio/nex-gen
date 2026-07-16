from __future__ import annotations

from collections.abc import AsyncIterator, Sequence
from typing import Any, cast

import pytest
import pytest_asyncio
import temporalio.api.common.v1
from temporalio.client import Client
from temporalio.converter import (
    DataConverter,
    DefaultPayloadConverter,
    PayloadConverter,
)
from temporalio.converter import (
    SerializationContext,
    WithSerializationContext,
)
from temporalio.testing import WorkflowEnvironment
from typing_extensions import Self, override


class TemporalIntermediatePayloadConverter(PayloadConverter, WithSerializationContext):
    _inner_payload_converter: PayloadConverter

    def __init__(self, inner_payload_converter: PayloadConverter | None = None) -> None:
        self._inner_payload_converter = (
            inner_payload_converter or DefaultPayloadConverter()
        )

    @override
    def to_payloads(
        self, values: Sequence[Any]
    ) -> list[temporalio.api.common.v1.Payload]:
        return self._inner_payload_converter.to_payloads(
            [
                to_intermediate(payload_converter=self._inner_payload_converter)
                if (
                    to_intermediate := getattr(value, "_temporal_to_intermediate", None)
                )
                is not None
                else value
                for value in values
            ]
        )

    @override
    def from_payloads(
        self,
        payloads: Sequence[temporalio.api.common.v1.Payload],
        type_hints: list[type] | None = None,
    ) -> list[Any]:
        type_hints = type_hints or []
        inner_type_hints = (
            [
                None
                if getattr(type_hint, "_temporal_from_intermediate", None) is not None
                else type_hint
                for type_hint in type_hints
            ]
            if type_hints
            else None
        )
        intermediate_values = self._inner_payload_converter.from_payloads(
            payloads,
            cast("list[type] | None", inner_type_hints),
        )
        return [
            from_intermediate(
                intermediate_value, payload_converter=self._inner_payload_converter
            )
            if (
                from_intermediate := getattr(
                    type_hint, "_temporal_from_intermediate", None
                )
            )
            is not None
            else intermediate_value
            for intermediate_value, type_hint in zip(
                intermediate_values, type_hints or [None] * len(intermediate_values)
            )
        ]

    @override
    def with_context(self, context: SerializationContext) -> Self:
        if not isinstance(self._inner_payload_converter, WithSerializationContext):
            return self
        inner_payload_converter = self._inner_payload_converter.with_context(context)
        if inner_payload_converter is self._inner_payload_converter:
            return self
        return type(self)(inner_payload_converter)


class TemporalIntermediateDefaultPayloadConverter(TemporalIntermediatePayloadConverter):
    def __init__(self) -> None:
        super().__init__(DefaultPayloadConverter())


temporal_intermediate_data_converter = DataConverter(
    payload_converter_class=TemporalIntermediateDefaultPayloadConverter
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
            data_converter=temporal_intermediate_data_converter
        )
    else:
        workflow_environment = WorkflowEnvironment.from_client(
            await Client.connect(
                env_type,
                data_converter=temporal_intermediate_data_converter,
            )
        )

    try:
        yield workflow_environment
    finally:
        await workflow_environment.shutdown()

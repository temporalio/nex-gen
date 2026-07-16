from __future__ import annotations

import contextlib
import contextvars
from collections.abc import AsyncIterator, Iterator, Sequence
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
)
from temporalio.testing import WorkflowEnvironment
from typing_extensions import override


_user_payload_converter: contextvars.ContextVar[PayloadConverter | None] = (
    contextvars.ContextVar("temporal-system-nexus-user-payload-converter", default=None)
)


@contextlib.contextmanager
def user_payload_converter_context(
    payload_converter: PayloadConverter,
) -> Iterator[None]:
    token = _user_payload_converter.set(payload_converter)
    try:
        yield
    finally:
        _user_payload_converter.reset(token)


def current_user_payload_converter() -> PayloadConverter:
    payload_converter = _user_payload_converter.get()
    if payload_converter is None:
        raise RuntimeError("System Nexus user payload converter context is not active")
    return payload_converter


def _install_temporal_nexus_system_shim() -> None:
    setattr(
        temporalio.nexus.system,
        "current_user_payload_converter",
        current_user_payload_converter,
    )
    setattr(
        temporalio.nexus.system,
        "user_payload_converter_context",
        user_payload_converter_context,
    )


_install_temporal_nexus_system_shim()


class TemporalModelPayloadConverter(PayloadConverter):
    _inner_payload_converter: PayloadConverter

    def __init__(self, inner_payload_converter: PayloadConverter | None = None) -> None:
        self._inner_payload_converter = (
            inner_payload_converter or DefaultPayloadConverter()
        )

    @override
    def to_payloads(
        self, values: Sequence[Any]
    ) -> list[temporalio.api.common.v1.Payload]:
        intermediate_values: list[Any] = []
        with user_payload_converter_context(self._inner_payload_converter):
            for value in values:
                to_intermediate = getattr(value, "_temporal_to_intermediate", None)
                if to_intermediate is not None:
                    value = to_intermediate()
                intermediate_values.append(value)
        return self._inner_payload_converter.to_payloads(intermediate_values)

    @override
    def from_payloads(
        self,
        payloads: Sequence[temporalio.api.common.v1.Payload],
        type_hints: list[type] | None = None,
    ) -> list[Any]:
        if type_hints is None:
            return self._inner_payload_converter.from_payloads(payloads, None)

        normalized_type_hints: list[type | None] = list(type_hints)
        if len(normalized_type_hints) < len(payloads):
            normalized_type_hints.extend([None] * (len(payloads) - len(type_hints)))

        inner_type_hints = [
            None
            if getattr(type_hint, "_temporal_from_intermediate", None) is not None
            else type_hint
            for type_hint in normalized_type_hints
        ]
        with user_payload_converter_context(self._inner_payload_converter):
            values = self._inner_payload_converter.from_payloads(
                payloads, cast(list[type], inner_type_hints)
            )
            return [
                from_intermediate(value)
                if (
                    from_intermediate := getattr(
                        type_hint, "_temporal_from_intermediate", None
                    )
                )
                is not None
                else value
                for value, type_hint in zip(values, normalized_type_hints)
            ]


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
        workflow_environment = await WorkflowEnvironment.start_local(
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

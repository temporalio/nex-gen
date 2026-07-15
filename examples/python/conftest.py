from __future__ import annotations

from collections.abc import AsyncIterator
from typing import Any, ClassVar, cast

import pytest
import pytest_asyncio
import temporalio.api.common.v1
from temporalio.client import Client
from temporalio.converter import (
    CompositePayloadConverter,
    DataConverter,
    DefaultPayloadConverter,
    EncodingPayloadConverter,
    PayloadConverter,
)
from temporalio.converter import (
    SerializationContext,
    WithSerializationContext,
)
from temporalio.testing import WorkflowEnvironment
from typing_extensions import Self, override


class TemporalWirePayloadConverter(EncodingPayloadConverter, WithSerializationContext):
    _inner_encoding_metadata_key: ClassVar[str] = "temporal-wire-encoding"
    _inner_metadata_prefix: ClassVar[str] = "temporal-wire-metadata-"
    _inner_payload_converter: PayloadConverter

    def __init__(self, inner_payload_converter: PayloadConverter | None = None) -> None:
        self._inner_payload_converter = (
            inner_payload_converter
            or CompositePayloadConverter(*_default_inner_encoding_payload_converters())
        )

    @property
    @override
    def encoding(self) -> str:
        return "binary/temporal-wire"

    @override
    def to_payload(self, value: Any) -> temporalio.api.common.v1.Payload | None:
        to_wire = getattr(value, "_temporal_to_wire", None)
        if to_wire is None:
            return None

        wire_value = to_wire(payload_converter=self._inner_payload_converter)
        wire_payload = self._inner_payload_converter.to_payload(wire_value)
        inner_encoding = wire_payload.metadata.get("encoding")
        if inner_encoding is None:
            raise RuntimeError("Temporal wire payload missing inner encoding")
        return temporalio.api.common.v1.Payload(
            metadata={
                "encoding": self.encoding.encode(),
                self._inner_encoding_metadata_key: inner_encoding,
                **{
                    self._inner_metadata_prefix + key: metadata_value
                    for key, metadata_value in wire_payload.metadata.items()
                    if key != "encoding"
                },
            },
            data=wire_payload.data,
        )

    @override
    def from_payload(
        self,
        payload: temporalio.api.common.v1.Payload,
        type_hint: type | None = None,
    ) -> Any:
        wire_type_method = getattr(type_hint, "_temporal_wire_type", None)
        from_wire = getattr(type_hint, "_temporal_from_wire", None)
        if wire_type_method is None or from_wire is None:
            raise RuntimeError(
                f"Payload with encoding {self.encoding} requires a Temporal wire type hint"
            )

        inner_encoding = payload.metadata.get(self._inner_encoding_metadata_key)
        if inner_encoding is None:
            raise RuntimeError("Temporal wire payload missing inner encoding")
        wire_metadata = {
            key.removeprefix(self._inner_metadata_prefix): metadata_value
            for key, metadata_value in payload.metadata.items()
            if key.startswith(self._inner_metadata_prefix)
        }
        wire_metadata["encoding"] = inner_encoding
        wire_payload = temporalio.api.common.v1.Payload(
            metadata=wire_metadata,
            data=payload.data,
        )
        wire_value = self._inner_payload_converter.from_payload(
            wire_payload, wire_type_method()
        )
        return from_wire(wire_value, payload_converter=self._inner_payload_converter)

    @override
    def with_context(self, context: SerializationContext) -> Self:
        if not isinstance(self._inner_payload_converter, WithSerializationContext):
            return self
        inner_payload_converter = self._inner_payload_converter.with_context(context)
        if inner_payload_converter is self._inner_payload_converter:
            return self
        return type(self)(inner_payload_converter)


class TemporalWireDefaultPayloadConverter(CompositePayloadConverter):
    def __init__(self) -> None:
        inner_converters = _default_inner_encoding_payload_converters()
        super().__init__(
            *inner_converters[:2],
            TemporalWirePayloadConverter(CompositePayloadConverter(*inner_converters)),
            *inner_converters[2:],
        )


def _default_inner_encoding_payload_converters() -> tuple[
    EncodingPayloadConverter, ...
]:
    inner_converters = getattr(
        DefaultPayloadConverter,
        "default_inner_encoding_payload_converters",
        None,
    )
    if inner_converters is None:
        inner_converters = DefaultPayloadConverter.default_encoding_payload_converters
    return tuple(
        converter
        for converter in inner_converters
        if converter.encoding != "binary/temporal-wire"
    )


temporal_wire_data_converter = DataConverter(
    payload_converter_class=TemporalWireDefaultPayloadConverter
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
            data_converter=temporal_wire_data_converter
        )
    else:
        workflow_environment = WorkflowEnvironment.from_client(
            await Client.connect(
                env_type,
                data_converter=temporal_wire_data_converter,
            )
        )

    try:
        yield workflow_environment
    finally:
        await workflow_environment.shutdown()

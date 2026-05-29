from __future__ import annotations

import base64
import builtins
import collections.abc
import dataclasses
import enum
import json
import types
import typing

from temporalio.api.common.v1 import Payload
from temporalio.converter import (
    CompositePayloadConverter,
    DataConverter,
    DefaultPayloadConverter,
    EncodingPayloadConverter,
    JSONPlainPayloadConverter,
)
from typing_extensions import TypeAlias, override

NEXUS_ENCODING = "json/nexus"
NEXUS_TYPE_METADATA_KEY = "nexusType"
JsonValue: TypeAlias = (
    None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)


def _type_registry() -> dict[str, type[object]]:
    registry_obj = getattr(builtins, "_nex_gen_type_registry", None)
    if not isinstance(registry_obj, dict):
        registry: dict[str, type[object]] = {}
        setattr(builtins, "_nex_gen_type_registry", registry)
        return registry
    return typing.cast(dict[str, type[object]], registry_obj)


def _class_registry() -> dict[type[object], str]:
    registry_obj = getattr(builtins, "_nex_gen_class_registry", None)
    if not isinstance(registry_obj, dict):
        registry: dict[type[object], str] = {}
        setattr(builtins, "_nex_gen_class_registry", registry)
        return registry
    return typing.cast(dict[type[object], str], registry_obj)


def register_nexus_type(target: type[object], type_id: str) -> None:
    _type_registry()[type_id] = target
    _class_registry()[target] = type_id


def _wire_name(name: str) -> str:
    return name.replace("_", "-")


def _is_union_type(hint: object) -> bool:
    origin = typing.get_origin(hint)
    return origin is types.UnionType or str(origin) == "typing.Union"


def _type_args(hint: object) -> tuple[object, ...]:
    return typing.cast(tuple[object, ...], typing.get_args(hint))


def _type_hints(value: type[object]) -> collections.abc.Mapping[str, object]:
    return typing.cast(
        collections.abc.Mapping[str, object], typing.get_type_hints(value)
    )


def _without_none(hint: object) -> object:
    if not _is_union_type(hint):
        return hint
    args = tuple(arg for arg in _type_args(hint) if arg is not type(None))
    if len(args) == 1:
        return args[0]
    return hint


def _tuple_literal_tag(hint: object) -> str | None:
    if typing.get_origin(hint) is not tuple:
        return None
    args = _type_args(hint)
    if not args:
        return None
    first = args[0]
    if typing.get_origin(first) is typing.Literal:
        literal_args = _type_args(first)
        if len(literal_args) == 1 and isinstance(literal_args[0], str):
            return literal_args[0]
    return None


def _variant_tuple_hint(hint: object, tag: str) -> object | None:
    if _tuple_literal_tag(hint) == tag:
        return hint
    if _is_union_type(hint):
        for arg in _type_args(hint):
            if _tuple_literal_tag(arg) == tag:
                return arg
    return None


def _to_json(value: object, hint: object | None = None) -> JsonValue:
    if value is None:
        return None
    if isinstance(value, bytes):
        return base64.b64encode(value).decode()
    if isinstance(value, (enum.IntEnum, enum.IntFlag)):
        return int(value)
    if dataclasses.is_dataclass(value):
        hints = _type_hints(type(value))
        return {
            _wire_name(field.name): _to_json(
                typing.cast(object, getattr(value, field.name)),
                hints.get(field.name),
            )
            for field in dataclasses.fields(value)
        }
    if isinstance(value, tuple):
        tuple_value = typing.cast(tuple[object, ...], value)
        tag = _tuple_literal_tag(_without_none(hint)) if hint is not None else None
        first = tuple_value[0] if tuple_value else None
        if tag is None and isinstance(first, str):
            tag = first
        if tag is not None:
            if not isinstance(first, str):
                raise TypeError("variant tuples must start with a string tag")
            if len(tuple_value) == 1:
                return {"tag": first}
            payload_hint = None
            tuple_hint = (
                _variant_tuple_hint(_without_none(hint), first) if hint else None
            )
            if tuple_hint is not None:
                args = _type_args(tuple_hint)
                if len(args) > 1:
                    payload_hint = args[1]
            return {"tag": first, "value": _to_json(tuple_value[1], payload_hint)}
        item_hints = _type_args(_without_none(hint)) if hint is not None else ()
        return [
            _to_json(item, item_hints[index] if index < len(item_hints) else None)
            for index, item in enumerate(tuple_value)
        ]
    if isinstance(value, list):
        list_value = typing.cast(list[object], value)
        item_hint = None
        if hint is not None and typing.get_origin(_without_none(hint)) is list:
            args = _type_args(_without_none(hint))
            item_hint = args[0] if args else None
        return [_to_json(item, item_hint) for item in list_value]
    if isinstance(value, dict):
        value_hint = None
        if hint is not None and typing.get_origin(_without_none(hint)) is dict:
            args = _type_args(_without_none(hint))
            value_hint = args[1] if len(args) > 1 else None
        mapping = typing.cast(collections.abc.Mapping[object, object], value)
        return {str(key): _to_json(item, value_hint) for key, item in mapping.items()}
    if isinstance(value, (bool, int, float, str)):
        return value
    raise TypeError(f"cannot encode value of type {type(value).__name__} as json/nexus")


def _from_json(value: JsonValue, hint: object) -> object:
    resolved_hint = _without_none(hint)
    if value is None:
        return None
    if resolved_hint is bytes:
        if not isinstance(value, str):
            raise TypeError("bytes values must be encoded as base64 strings")
        return base64.b64decode(value)
    if isinstance(resolved_hint, type) and issubclass(
        resolved_hint, (enum.IntEnum, enum.IntFlag)
    ):
        return resolved_hint(value)
    if isinstance(resolved_hint, type) and dataclasses.is_dataclass(resolved_hint):
        if not isinstance(value, dict):
            raise TypeError("record values must be encoded as objects")
        hints = _type_hints(resolved_hint)
        fields: dict[str, object] = {}
        for field in dataclasses.fields(resolved_hint):
            wire_name = _wire_name(field.name)
            if wire_name in value:
                fields[field.name] = _from_json(value[wire_name], hints[field.name])
        constructor = typing.cast(collections.abc.Callable[..., object], resolved_hint)
        return constructor(**fields)
    hint_obj = typing.cast(object, resolved_hint)
    if _is_union_type(hint_obj):
        if isinstance(value, dict) and isinstance(value.get("tag"), str):
            tag = typing.cast(str, value["tag"])
            tuple_hint = _variant_tuple_hint(hint_obj, tag)
            if tuple_hint is not None:
                args = _type_args(tuple_hint)
                if len(args) == 1:
                    return (tag,)
                return (tag, _from_json(value.get("value"), args[1]))
        args = _type_args(hint_obj)
        return _from_json(value, args[0])
    origin = typing.get_origin(hint_obj)
    if origin is tuple:
        if isinstance(value, dict) and isinstance(value.get("tag"), str):
            args = _type_args(hint_obj)
            tag = typing.cast(str, value["tag"])
            if len(args) == 1:
                return (tag,)
            return (tag, _from_json(value.get("value"), args[1]))
        if not isinstance(value, list):
            raise TypeError("tuple values must be encoded as arrays")
        args = _type_args(hint_obj)
        return tuple(_from_json(item, args[index]) for index, item in enumerate(value))
    if origin is list:
        if not isinstance(value, list):
            raise TypeError("list values must be encoded as arrays")
        args = _type_args(hint_obj)
        item_hint = args[0] if args else object
        return [_from_json(item, item_hint) for item in value]
    if origin is dict:
        if not isinstance(value, dict):
            raise TypeError("map values must be encoded as objects")
        args = _type_args(hint_obj)
        value_hint = args[1] if len(args) > 1 else object
        return {key: _from_json(item, value_hint) for key, item in value.items()}
    return value


class NexusPayloadConverter(EncodingPayloadConverter):
    @property
    @override
    def encoding(self) -> str:
        return NEXUS_ENCODING

    @override
    def to_payload(self, value: object) -> Payload | None:
        type_id = _class_registry().get(type(value))
        if type_id is None:
            return None

        return Payload(
            metadata={
                "encoding": NEXUS_ENCODING.encode(),
                NEXUS_TYPE_METADATA_KEY: type_id.encode(),
            },
            data=json.dumps(
                _to_json(value, type(value)),
                separators=(",", ":"),
                sort_keys=True,
            ).encode(),
        )

    @override
    def from_payload(
        self,
        payload: Payload,
        type_hint: type | None = None,
    ) -> object:
        del type_hint
        type_id = payload.metadata.get(NEXUS_TYPE_METADATA_KEY)
        if type_id is None:
            raise RuntimeError("json/nexus payload is missing nexusType metadata")
        type_id_text = type_id.decode()
        try:
            target = _type_registry()[type_id_text]
        except KeyError as err:
            raise RuntimeError(
                f"unknown nex-gen type {type_id_text!r}; import the generated module before decoding"
            ) from err
        return _from_json(typing.cast(JsonValue, json.loads(payload.data)), target)


class NexusCompositePayloadConverter(CompositePayloadConverter):
    def __init__(self) -> None:
        default_converters = DefaultPayloadConverter.default_encoding_payload_converters
        converters: list[EncodingPayloadConverter] = []
        inserted = False
        for converter in default_converters:
            if isinstance(converter, JSONPlainPayloadConverter) and not inserted:
                converters.append(NexusPayloadConverter())
                inserted = True
            converters.append(converter)
        if not inserted:
            converters.append(NexusPayloadConverter())
        super().__init__(*converters)


nexus_data_converter = DataConverter(
    payload_converter_class=NexusCompositePayloadConverter,
)

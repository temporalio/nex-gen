from __future__ import annotations

from contextlib import AbstractContextManager

from temporalio.converter import PayloadConverter

def current_user_payload_converter() -> PayloadConverter: ...
def user_payload_converter_context(
    payload_converter: PayloadConverter,
) -> AbstractContextManager[None, bool | None]: ...

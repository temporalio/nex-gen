from __future__ import annotations
from typing import Union, Literal, ClassVar
from pydantic import BaseModel, TypeAdapter

class UserEvent(BaseModel):
    # attempt: nest the const open-union type alias inside the model
    Kind: ClassVar = Union[Literal["user"], str]
    kind: "UserEvent.Kind"   # field typed by the nested alias

try:
    UserEvent.model_rebuild()
    m = UserEvent(kind="user")
    print("OK constructed:", m, "| kind field type resolved")
    print("roundtrip:", m.model_dump_json())
except Exception as e:
    print("FAILED:", type(e).__name__, str(e)[:300])

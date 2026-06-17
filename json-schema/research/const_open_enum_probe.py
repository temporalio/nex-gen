from typing import Literal, Union, get_type_hints
from pydantic import BaseModel, model_validator, model_serializer
import pydantic_core

# Python open-enum representation for a const, parallel to TS `"user" | (string & {})`
# and Go `type EventKind = string`.
EventKindUser = Literal["user"]
EventKind = Union[EventKindUser, str]          # open: any str assignable -> P9.1 forward-compat

CONST = "user"

class Event(BaseModel):
    kind: EventKind
    data: str | None = None

    @model_validator(mode="before")
    @classmethod
    def _const(cls, d):
        if isinstance(d, dict):
            if "kind" not in d:
                d["kind"] = CONST               # auto-fill -> lands in model_fields_set
            elif d["kind"] != CONST:
                raise ValueError(f"const: expected {CONST!r}, got {d['kind']!r}")
        return d

    @model_serializer(mode="wrap")
    def _ser(self, h):
        full = h(self)
        return {k: v for k, v in full.items() if k in self.model_fields_set}

print("annotation Pydantic sees for 'kind':", Event.model_fields["kind"].annotation)

print("\n1. construct w/o kind -> auto-filled, set, emitted")
m = Event(data="x")
print("   fields_set:", m.model_fields_set, "| json:", pydantic_core.to_json(m).decode())

print("\n2. WRONG value must be rejected (not silently accepted by the str arm)")
try:
    Event(kind="admin")
    print("   ERROR: accepted 'admin'!")
except Exception as e:
    print("   rejected OK:", type(e).__name__)

print("\n3. deserialize correct value")
m3 = Event.model_validate_json('{"kind":"user","data":"y"}')
print("   json:", pydantic_core.to_json(m3).decode())

print("\n4. does Pydantic accept an arbitrary future str through the union arm? (before our check)")
#   (proves the TYPE is open; our validator is what closes it to the const)
class OpenOnly(BaseModel):
    kind: EventKind
print("   OpenOnly(kind='user_v2') ->", OpenOnly(kind="user_v2").kind, "(type open; validator absent)")

import pydantic, sys
print("\nversions: pydantic", pydantic.VERSION, "| python", sys.version.split()[0])

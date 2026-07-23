from pydantic import BaseModel, model_validator, model_serializer
from typing import Any

# Hypothesis: a key injected by a mode="before" validator lands in model_fields_set,
# so it survives an omit-unset serializer with NO const-specific keep-set entry.

class UserEvent(BaseModel):
    kind: str          # plain primitive, NO Pydantic default
    data: str | None = None

    @model_validator(mode="before")
    @classmethod
    def _const_kind(cls, d: Any):
        if isinstance(d, dict):
            if "kind" not in d:
                d["kind"] = "user"          # auto-fill required+const
            elif d["kind"] != "user":
                raise ValueError(f"const: expected 'user', got {d['kind']!r}")
        return d

    # Generic omit-unset serializer keyed ONLY on model_fields_set (NO const_fields union)
    @model_serializer(mode="wrap")
    def _ser(self, handler):
        full = handler(self)
        return {k: v for k, v in full.items() if k in self.model_fields_set}


print("== 1. construct WITHOUT kind (auto-filled by before-validator) ==")
m = UserEvent(data="x")
print("   model_fields_set:", m.model_fields_set)
print("   'kind' in set? ->", "kind" in m.model_fields_set)
print("   model_dump():", m.model_dump())

import pydantic_core
print("   to_json (Temporal default path):", pydantic_core.to_json(m).decode())

print("\n== 2. construct WITHOUT kind and WITHOUT data (only kind auto-filled) ==")
m2 = UserEvent()
print("   model_fields_set:", m2.model_fields_set)
print("   to_json:", pydantic_core.to_json(m2).decode())

print("\n== 3. deserialize from wire with correct kind ==")
m3 = UserEvent.model_validate_json('{"kind":"user","data":"y"}')
print("   to_json:", pydantic_core.to_json(m3).decode())

print("\n== 4. deserialize with WRONG kind (must reject) ==")
try:
    UserEvent.model_validate_json('{"kind":"admin"}')
    print("   ERROR: did not reject!")
except Exception as e:
    print("   rejected OK:", type(e).__name__)

print("\n== 5. optional 'data' stays omit-unset (not auto-filled, not in set) ==")
print("   m2 (no data) json has 'data'? ->", "data" in pydantic_core.to_json(m2).decode())

import pydantic, sys
print("\nversions: pydantic", pydantic.VERSION, "| python", sys.version.split()[0])

"""Probe: is our P11/P17 Python serialize design compatible with the
*default* Temporal pydantic_data_converter?

The default converter serializes via plain pydantic_core.to_json(value)
(exclude_unset=False, no validation). So we cannot rely on calling
model_dump(exclude_unset=True) ourselves. Question: can we bake the
omit-unset + const-force (+ serialize validation) into the model via
@model_serializer so it survives the converter's own to_json call?
"""
from typing import Any, ClassVar, Optional

from pydantic import BaseModel, ConfigDict, model_serializer, model_validator
from temporalio.contrib.pydantic import pydantic_data_converter, PydanticPayloadConverter
from temporalio.contrib.pydantic import ToJsonOptions

conv = pydantic_data_converter.payload_converter  # CompositePayloadConverter
def to_bytes(v):
    return conv.to_payload(v).data
def from_bytes(data, T):
    import temporalio.api.common.v1 as c
    p = c.Payload(metadata={"encoding": b"json/plain"}, data=data)
    return conv.from_payload(p, T)

print("=" * 70)
print("STEP 0: does the DEFAULT converter even invoke @model_serializer?")
print("=" * 70)

class Sentinel(BaseModel):
    x: int = 1
    @model_serializer(mode="wrap")
    def _ser(self, handler):
        d = handler(self)
        d["__serializer_ran__"] = True
        return d

print("default-converter bytes:", to_bytes(Sentinel(x=5)))


print()
print("=" * 70)
print("STEP 1: bake omit-unset + const-force into the model")
print("=" * 70)

class Nested(BaseModel):
    model_config = ConfigDict(strict=True)
    a: str
    note: Optional[str] = "n-default"          # default → omit-unset
    _const_fields: ClassVar[frozenset] = frozenset()

    @model_serializer(mode="wrap")
    def _ser(self, handler):
        full = handler(self)
        keep = set(self.model_fields_set) | self._const_fields
        return {k: v for k, v in full.items() if k in keep}

class User(BaseModel):
    model_config = ConfigDict(strict=True)
    kind: str = "user"                          # const discriminator
    name: str                                   # required
    nickname: Optional[str] = "anon"            # default → omit-unset
    child: Optional[Nested] = None

    _const_fields: ClassVar[frozenset] = frozenset({"kind"})

    @model_validator(mode="before")
    @classmethod
    def _force_const(cls, data):
        # auto-populate + enforce const (the discriminator must be on the wire)
        if isinstance(data, dict):
            if "kind" in data and data["kind"] != "user":
                raise ValueError(f"const kind must be 'user', got {data['kind']!r}")
            data = {**data, "kind": "user"}
        return data

    @model_serializer(mode="wrap")
    def _ser(self, handler):
        full = handler(self)
        keep = set(self.model_fields_set) | self._const_fields
        return {k: v for k, v in full.items() if k in keep}

# a) construct minimal; nickname unset, kind auto-populated
u = User(name="roey")
print("repr fields_set:", u.model_fields_set)
print("read nickname (should surface default):", u.nickname)
b = to_bytes(u)
print("payload bytes:", b)
assert b'"nickname"' not in b, "FAIL: unset default leaked onto the wire"
assert b'"kind":"user"' in b, "FAIL: const discriminator dropped"
assert b'"child"' not in b, "FAIL: unset optional leaked"
print("PASS: default omitted, const force-emitted, unset optional omitted")

# b) explicitly set nickname to the SAME value as the default → must pin
u2 = User(name="roey", nickname="anon")
b2 = to_bytes(u2)
print("explicit-set-to-default bytes:", b2)
assert b'"nickname":"anon"' in b2, "FAIL: explicit-set-to-default was dropped (deep-equals?)"
print("PASS: explicit-set-to-default pinned (no deep-equals)")

# c) nested model omit-unset recurses
u3 = User(name="roey", child=Nested(a="hi"))
b3 = to_bytes(u3)
print("nested bytes:", b3)
assert b'"note"' not in b3, "FAIL: nested unset default leaked"
assert b'"a":"hi"' in b3
print("PASS: nested omit-unset recurses through to_json")


print()
print("=" * 70)
print("STEP 2: deserialize through converter runs our validators")
print("=" * 70)

# good payload round-trips
back = from_bytes(b, User)
print("round-tripped:", back, "| fields_set:", back.model_fields_set)
assert back.kind == "user"
# the wire had no nickname → still unset after decode → re-serialize stays absent
rb = to_bytes(back)
print("re-serialized:", rb)
assert b'"nickname"' not in rb, "FAIL: round-trip echoed a default"
print("PASS: absent stays absent across full round-trip (P12 faithful)")

# bad const rejected on deserialize
try:
    from_bytes(b'{"kind":"admin","name":"x"}', User)
    print("FAIL: bad const accepted on deserialize")
except Exception as e:
    print("PASS: bad const rejected on deserialize ->", type(e).__name__)

# strict-mode violation rejected on deserialize
try:
    from_bytes(b'{"name":123}', User)
    print("FAIL: strict violation accepted")
except Exception as e:
    print("PASS: strict type violation rejected ->", type(e).__name__)


print()
print("=" * 70)
print("STEP 3: model_construct / mutation bypass — does serialize catch it?")
print("=" * 70)

bad = User.model_construct(name=123, kind="WRONG")  # bypasses validation
try:
    bb = to_bytes(bad)
    print("model_construct serialized WITHOUT validation:", bb)
    print("NOTE: plain to_json does NOT re-validate (expected) -> need a guard")
except Exception as e:
    print("serialize raised ->", type(e).__name__, e)


print()
print("=" * 70)
print("STEP 4: confirm the ToJsonOptions(exclude_unset=True) path is NOT")
print("        what we depend on (it works, but it's non-default + global)")
print("=" * 70)
opt_conv = PydanticPayloadConverter(ToJsonOptions(exclude_unset=True))
# a PLAIN model with no @model_serializer, under the exclude_unset converter:
class Plain(BaseModel):
    kind: str = "user"
    name: str
    nickname: Optional[str] = "anon"
pb = opt_conv.to_payload(Plain(name="x")).data
print("exclude_unset-converter on plain model:", pb)
print("NOTE: this drops the const 'kind' too (the discriminator bug) ->",
      b'"kind"' not in pb)

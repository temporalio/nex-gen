# Numeric-constraint probes for multipleOf / maximum / minimum / exclusive*.
# Run:  python -m venv /tmp/v && /tmp/v/bin/pip install pydantic && /tmp/v/bin/python pyd_numeric_probe.py
# Verified against pydantic 2.13.4 (pydantic-core Rust), Python 3.13.
#
# Findings that drive the numeric-constraint specs:
#  1. Pydantic's native float `multiple_of` is TOLERANT and DIVERGES from IEEE
#     fmod for FRACTIONAL divisors: it ACCEPTS 0.3 / 1.1 / 0.2 against
#     multiple_of=0.1, where Go math.Mod / Java % / JS % / Python math.fmod all
#     REJECT them (0.3 % 0.1 == 0.09999999999999998). => fractional multipleOf
#     cannot agree cross-language (P1) => reject at load.
#  2. For INTEGER divisors Pydantic's multiple_of AGREES with fmod value-for-value
#     on both int and float fields (10.0/6.0 ok, 7.5 reject, 1e300 ok for mo=2).
#  3. Pydantic REFUSES to build `le=5.5` (fractional bound) on an `int` field
#     ("'le' must be coercible to an integer"); `le=5.0` builds. => integer-field
#     bounds must be integer-valued; fractional bound on an integer field -> reject.
#  4. Numeric constraints compose over the type-spec SpecInt BeforeValidator via
#     annotated_types.Ge/Le/Gt/Lt/MultipleOf: 2.0/10.0 normalize then get bounded.

from typing import Annotated
from pydantic import BaseModel, Field, BeforeValidator, ValidationError, ConfigDict
from annotated_types import Ge, Le, Gt, Lt, MultipleOf


def _parse_spec_integer(v):
    if isinstance(v, bool):
        raise ValueError("bool not integer")
    if isinstance(v, float):
        if v != int(v):
            raise ValueError("fractional")
        v = int(v)
    if isinstance(v, int) and abs(v) > 9007199254740991:
        raise ValueError("cap")
    return v


SpecInt = Annotated[int, BeforeValidator(_parse_spec_integer)]


def t(M, val):
    try:
        M(v=val)
        return "OK"
    except ValidationError as e:
        return "REJECT(" + e.errors()[0]["type"] + ")"


def build(typ, **field):
    try:
        class M(BaseModel):
            model_config = ConfigDict(strict=True)
            v: typ = Field(**field)  # type: ignore
        return "builds"
    except Exception as e:
        return "SchemaError: " + str(e).splitlines()[-1].strip()


# (1)/(2) multipleOf: integer divisor agrees with fmod; fractional diverges.
class MI(BaseModel):
    model_config = ConfigDict(strict=True)
    v: int = Field(multiple_of=2)

class MF(BaseModel):
    model_config = ConfigDict(strict=True)
    v: float = Field(multiple_of=2)

class MFrac(BaseModel):
    model_config = ConfigDict(strict=True)
    v: float = Field(multiple_of=0.1)

print("== multipleOf ==")
for name, M, vals in [("int mo=2", MI, [10, 9]),
                      ("float mo=2", MF, [10.0, 6.0, 7.5, 1e300]),
                      ("float mo=0.1 (TOLERANT, != fmod)", MFrac, [0.3, 1.1, 0.2, 0.1])]:
    for val in vals:
        print(f"  {name}: v={val!r} -> {t(M, val)}")

# (3) fractional bound on int field fails to build; 5.0 ok.
print("== bound build on int field ==")
print("  int le=5.0 :", build(int, le=5.0))
print("  int le=5.5 :", build(int, le=5.5))
print("  float le=5.5:", build(float, le=5.5))

# (4) constraints compose over SpecInt BeforeValidator.
class MC(BaseModel):
    model_config = ConfigDict(strict=True)
    v: Annotated[SpecInt, Ge(0), Le(10), MultipleOf(2)]

print("== SpecInt + Ge/Le/MultipleOf ==")
for val in [2, 2.0, 4.0, 3, 12, -2, 1e1, 5.0]:
    print(f"  v={val!r:>6} -> {t(MC, val)}")

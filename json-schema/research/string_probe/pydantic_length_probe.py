# Probe: does Pydantic v2's string length constraint count Unicode CODE POINTS
# (matching Python len and the other three targets), or bytes / UTF-16 units?
#
# pydantic-core runs the check in Rust, where str.len() is BYTES and
# .chars().count() is code points — so this must be verified, not assumed
# (same "verify Pydantic empirically" rule the numeric probes follow).
#
# Result (pydantic 2.13.4): CODE POINTS. A single astral emoji (1 code point,
# 4 UTF-8 bytes, 2 UTF-16 units) passes max_length=1; two emoji (2 code
# points) fail. Confirms the features/maxLength + features/minLength Python row.
#
# Run:  python -m venv v && ./v/bin/pip install pydantic && ./v/bin/python pydantic_length_probe.py
import pydantic
from pydantic import BaseModel, StringConstraints
from typing import Annotated

print("pydantic", pydantic.VERSION)


class Max1(BaseModel):
    s: Annotated[str, StringConstraints(max_length=1)]


class Min1(BaseModel):
    s: Annotated[str, StringConstraints(min_length=1)]


def accepts(model, s):
    try:
        model(s=s)
        return True
    except Exception:
        return False


# (string, code_points, utf8_bytes, utf16_units)
cases = [
    ("a", 1, 1, 1),
    ("\U0001F600", 1, 4, 2),  # emoji: 1 code point but 4 bytes / 2 UTF-16 units
    ("é", 1, 2, 1),      # é NFC
    ("Ā", 1, 2, 1),      # Ā
]
for s, cp, b, u16 in cases:
    print(f"  {s!r:12} cp={cp} utf8={b} utf16={u16}  max_length=1 accepts: {accepts(Max1, s)}")

print("  two emoji (cp=2)  max_length=1 accepts:", accepts(Max1, "\U0001F600\U0001F600"), "(expect False)")
print("  ''      (cp=0)    min_length=1 accepts:", accepts(Min1, ""), "(expect False)")
print("  emoji   (cp=1)    min_length=1 accepts:", accepts(Min1, "\U0001F600"), "(expect True)")

# Verdict: all cp=1 rows accept regardless of byte/UTF-16 length, the cp=2 row
# rejects -> Pydantic counts code points. No custom AfterValidator needed.

from pydantic import BaseModel, ConfigDict, Field, ValidationError
from typing import Literal


class User(BaseModel):
    model_config = ConfigDict(strict=True)
    name: str = Field(min_length=3)
    nickname: str = "anon"            # default
    version: Literal["v1"] = "v1"     # const-ish


print("== A. strict construction already validates (can't build invalid) ==")
try:
    User(name="ab")
except ValidationError as e:
    print("  rejected:", [(er["loc"], er["type"]) for er in e.errors()])

print("\n== B. default materialized on READ, but omitted on serialize (no deep-equals) ==")
u = User(name="alice")
print("  u.nickname read :", repr(u.nickname))          # 'anon' default surfaced
print("  exclude_unset   :", u.model_dump(exclude_unset=True))
print("  plain dump      :", u.model_dump())

print("\n== C. explicitly setting the default PINS it (set-ness tracked) ==")
u2 = User(name="alice", nickname="anon")
print("  exclude_unset   :", u2.model_dump(exclude_unset=True))

print("\n== D. const+exclude_unset would WRONGLY drop the discriminator ==")
print("  version in exclude_unset dump?:", "version" in u.model_dump(exclude_unset=True))
print("  -> const must be force-emitted, NOT treated as omit-unset")

print("\n== E. model_construct bypasses validation; serialize-time revalidation catches ==")
bad = User.model_construct(name="ab", version="v9")
print("  bypass built:", bad.model_dump())
try:
    User.model_validate(bad.model_dump())
except ValidationError as e:
    print("  revalidate rejected:", [(er["loc"], er["type"]) for er in e.errors()])

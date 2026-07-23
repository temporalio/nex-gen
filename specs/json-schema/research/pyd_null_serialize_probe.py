from pydantic import BaseModel, ConfigDict, Field, ValidationError
from typing import Optional


class M(BaseModel):
    model_config = ConfigDict(strict=True)
    req_nn: str                  # required, non-nullable
    opt_nn: str = "d"            # optional, non-nullable  -> explicit null must reject
    opt_n: Optional[str] = None  # optional, nullable
    req_n: Optional[str]         # required, nullable (Optional + no default => required)


def dump(label, m):
    print(f"  {label}: fields_set={sorted(m.model_fields_set)}  "
          f"exclude_unset={m.model_dump(exclude_unset=True)}")


print("== required+nullable must EMIT null; optional fields OMITTED when unset ==")
m = M(req_nn="x", req_n=None)
dump("constructed req_n=None, opts unset", m)

print("\n== does fields_set distinguish wire `null` from wire-absent? (faithful round-trip?) ==")
m_null = M.model_validate({"req_nn": "x", "req_n": None, "opt_n": None})
dump("wire opt_n: null ", m_null)
m_absent = M.model_validate({"req_nn": "x", "req_n": None})
dump("wire opt_n absent", m_absent)
print("  -> 'opt_n' set on null-input:", "opt_n" in m_null.model_fields_set,
      "| set on absent-input:", "opt_n" in m_absent.model_fields_set)

print("\n== optional+non-nullable: explicit null rejected in strict mode ==")
try:
    M(req_nn="x", req_n=None, opt_nn=None)
except ValidationError as e:
    print("  rejected:", [(er["loc"], er["type"]) for er in e.errors()])

print("\n== required+nullable absent on wire -> required violation ==")
try:
    M.model_validate({"req_nn": "x"})
except ValidationError as e:
    print("  rejected:", [(er["loc"], er["type"]) for er in e.errors()])

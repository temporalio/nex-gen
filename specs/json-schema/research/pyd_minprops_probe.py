from pydantic import BaseModel, ConfigDict

class M(BaseModel):
    model_config = ConfigDict(extra='allow')
    a: int
    b: int = 99          # has default
    c: int | None = None # optional w/ default

# Wire sends only 'a' and an extra 'x'
m = M.model_validate({"a": 1, "x": 7})
print("model_fields_set:", m.model_fields_set)        # which declared fields came from the wire?
print("__pydantic_extra__:", m.__pydantic_extra__)    # extras
print("full dict (defaults populated):", m.model_dump())
print("naive in-memory count:", len(m.model_dump()))  # over-counts via defaults
wire_count = len(m.model_fields_set) + len(m.__pydantic_extra__ or {})
print("fields_set+extras count:", wire_count)

# Does 'b' (defaulted, not on wire) appear in fields_set? Should NOT.
print("b in fields_set?", "b" in m.model_fields_set)

# Now send b explicitly -> should appear in fields_set
m2 = M.model_validate({"a": 1, "b": 2})
print("explicit b in fields_set?", "b" in m2.model_fields_set, "| set:", m2.model_fields_set)

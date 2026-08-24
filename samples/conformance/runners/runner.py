"""Generic cross-language conformance runner for generated **Python** packages.

Reads a plan written by `tests/json_schema_conformance_manifest.rs` (see
`tests/toolchain/mod.rs` for the protocol), drives every probe through the
generated model's registered ``TransferTypeConverter``, and writes one JSON
verdict document. The runner is deliberately generic: it discovers the model
class and its converter by name, so a new conformance case needs no runner
change.

Usage: ``python runner.py <plan.json> <result.json> <generated-root>``
"""

from __future__ import annotations

import importlib
import datetime
import json
import math
import re
import sys
from pathlib import Path
from typing import Any


def snake_case(name: str) -> str:
    """The Python attribute the generator derives from a JSON property name.

    Conformance schemas are restricted to lowerCamel ASCII property names, which
    makes this a total function; anything else fails loudly at lookup time.
    """
    step = re.sub(r"(.)([A-Z][a-z]+)", r"\1_\2", name)
    return re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", step).lower()


def converter_for(cls: type) -> Any:
    return getattr(cls, "__temporal_transfer_type_converter")


def violations_of(error: BaseException) -> list[dict[str, str]] | None:
    """`[{path, reason}]` if `error` is a generated ValidationError, else None."""
    found = getattr(error, "violations", None)
    if found is None or type(error).__name__ != "ValidationError":
        return None
    return [{"path": v.path, "reason": v.reason} for v in found]


def _member_name(obj: Any, prop: str) -> str:
    name = snake_case(prop)
    if not hasattr(obj, name):
        raise AssertionError(
            f"{type(obj).__name__} has no member {name!r} for {prop!r}"
        )
    return name


_SEGMENT = re.compile(r"([A-Za-z0-9]+)((?:\[\d+\])*)")


def steps_of(path: str) -> list[tuple[str, Any]]:
    """`a.b[0][1]` -> [('field','a'), ('field','b'), ('index',0), ('index',1)]."""
    out: list[tuple[str, Any]] = []
    for segment in path.split("."):
        match = _SEGMENT.fullmatch(segment)
        if match is None:
            raise AssertionError(f"unparsable mutation path segment {segment!r}")
        out.append(("field", match.group(1)))
        out.extend(("index", int(i)) for i in re.findall(r"\[(\d+)\]", match.group(2)))
    return out


def _read(owner: Any, step: tuple[str, Any]) -> Any:
    return (
        getattr(owner, _member_name(owner, step[1]))
        if step[0] == "field"
        else owner[step[1]]
    )


def _write(owner: Any, step: tuple[str, Any], value: Any) -> None:
    if step[0] == "field":
        setattr(owner, _member_name(owner, step[1]), value)
    else:
        owner[step[1]] = value


_SPECIAL_NUMBERS = {"nan": math.nan, "inf": math.inf, "-inf": -math.inf}


def _number(spec: str) -> float:
    return _SPECIAL_NUMBERS[spec] if spec in _SPECIAL_NUMBERS else float(spec)


def _typed_map(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    found = getattr(value, "additional_properties", None)
    if isinstance(found, dict):
        return found
    raise AssertionError(f"{type(value).__name__} is not a typed map")


def apply_mutation(model: Any, mutation: dict[str, Any]) -> None:
    steps = steps_of(mutation["path"])
    owner = model
    for step in steps[:-1]:
        owner = _read(owner, step)
    last = steps[-1]
    if "duplicate_element" in mutation:
        sequence = _read(owner, last)
        sequence.append(sequence[int(mutation["duplicate_element"])])
        return

    if "remove_array_element" in mutation:
        del _read(owner, last)[int(mutation["remove_array_element"])]
        return
    if "put_map_entry" in mutation:
        entry = mutation["put_map_entry"]
        _typed_map(_read(owner, last))[entry["key"]] = entry["value"]
        return
    if "remove_map_entry" in mutation:
        del _typed_map(_read(owner, last))[mutation["remove_map_entry"]]
        return
    if "set_integer" in mutation:
        value: Any = int(mutation["set_integer"])
    elif "set_number" in mutation:
        value = _number(mutation["set_number"])
    elif "set_string" in mutation:
        value = mutation["set_string"]
    elif "set_null" in mutation:
        value = None
    elif "set_absent" in mutation:
        value = None
    elif "set_bytes" in mutation:
        value = bytes(mutation["set_bytes"])
    elif "set_duration" in mutation:
        duration = mutation["set_duration"]
        value = datetime.timedelta(
            seconds=int(duration["seconds"]),
            microseconds=int(duration["nanoseconds"]) // 1_000,
        )
    else:
        raise AssertionError(f"unknown mutation: {mutation!r}")
    _write(owner, last, value)


def wire_of(transfer: Any) -> str:
    """Strict JSON. A non-finite number here means the serializer let one through."""
    return json.dumps(transfer, sort_keys=True, separators=(",", ":"), allow_nan=False)


def run_probe(cls: type, probe: dict[str, Any]) -> dict[str, Any]:
    converter = converter_for(cls)
    try:
        transfer_in = json.loads(probe["wire"])
    except ValueError as error:
        return {"outcome": "error", "message": f"probe wire is not JSON: {error}"}
    try:
        model = converter.from_transfer_type(transfer_in, cls)
    except BaseException as error:  # noqa: BLE001 - the verdict is the point
        found = violations_of(error)
        if found is None:
            return {"outcome": "error", "message": f"{type(error).__name__}: {error}"}
        return {"outcome": "parse_rejected", "violations": found}
    if probe["kind"] == "parse":
        return {"outcome": "accepted"}
    try:
        for mutation in probe.get("mutations", ()):
            apply_mutation(model, mutation)
    except BaseException as error:  # noqa: BLE001
        return {"outcome": "error", "message": f"mutation failed: {error}"}
    try:
        transfer_out = converter.to_transfer_type(model)
    except BaseException as error:  # noqa: BLE001
        found = violations_of(error)
        if found is None:
            return {"outcome": "error", "message": f"{type(error).__name__}: {error}"}
        return {"outcome": "serialize_rejected", "violations": found}
    try:
        return {"outcome": "accepted", "wire": wire_of(transfer_out)}
    except ValueError as error:
        return {
            "outcome": "accepted",
            "wire": None,
            "note": f"output is not JSON: {error}",
        }


def main() -> int:
    plan_path, result_path, generated_root = sys.argv[1:4]
    sys.path.insert(0, generated_root)
    plan = json.loads(Path(plan_path).read_text(encoding="utf-8"))
    results: dict[str, Any] = {}
    for case in plan["cases"]:
        probes: dict[str, Any] = {}
        results[case["id"]] = probes
        try:
            module = importlib.import_module(case["dir"])
            cls = getattr(module, case["model"])
        except BaseException as error:  # noqa: BLE001
            message = f"import failed: {type(error).__name__}: {error}"
            for probe in case["probes"]:
                probes[probe["id"]] = {"outcome": "error", "message": message}
            continue
        for probe in case["probes"]:
            probes[probe["id"]] = run_probe(cls, probe)
    Path(result_path).write_text(json.dumps(results, indent=1), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

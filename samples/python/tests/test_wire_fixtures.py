"""The byte-level round-trip sweep over *every* canonical wire fixture.

`samples/wire/json_schema/` is the cross-language contract: the Go, TypeScript and
Java suites read the same files. P1 says a value one language accepts round-trips
through any other unchanged, so the statement this file makes is deliberately
blunt — decode each fixture through the **default** data converter, re-encode, and
compare the payload's own **bytes** against the canonicalized fixture.

Bytes, not a parsed value, because a parsed comparison is blind to the two things
that matter most here: the wire form of a number (`1` vs `1.0` — `1 == 1.0` in
Python, which is how an `integer` `const` that kept its wire `float` went
unnoticed) and the exact escaping of a string.

Canonicalization normalizes only insignificant whitespace and member order (the
SDK's payload converter writes compact, key-sorted JSON while the fixture files are
formatted for humans) — see `canonical_json_bytes`.

The per-suite files assert what each fixture *means*; this one asserts that no
fixture escapes the byte comparison. The model table is exhaustive by test, so a
newly added fixture fails here until it is declared.
"""

from __future__ import annotations

import typing

import chat
import kb
import showcase
import temporal

from tests.json_converter_helper import (
    COLLAPSED_NULL_MEMBERS,
    NON_CANONICAL_FIXTURES,
    canonical_fixture_bytes,
    canonical_json_bytes,
    decode,
    decode_fixture,
    encode_bytes,
    fixture_bytes,
    fixture_dir,
    load_fixture,
    roundtrip_fixture,
)

#: Every fixture in `samples/wire/json_schema/`, with the model it round-trips
#: through. Exhaustive: `test_every_wire_fixture_is_declared` fails if a file is
#: added, removed or renamed without updating this table.
WIRE_FIXTURES: dict[tuple[str, str], type[typing.Any]] = {
    ("chat", "labels.json"): chat.Labels,
    ("chat", "message-full.json"): chat.Message,
    ("chat", "message-minimal.json"): chat.Message,
    ("chat", "room-open.json"): chat.Room,
    ("chat", "send-message-input.json"): chat.SendMessageInput,
    ("chat", "send-message-output.json"): chat.SendMessageOutput,
    ("kb", "block.json"): kb.Block,
    ("kb", "category-tree.json"): kb.Category,
    ("kb", "get-category-tree-input.json"): kb.GetCategoryTreeInput,
    ("kb", "get-page-input.json"): kb.GetPageInput,
    ("kb", "page.json"): kb.Page,
    ("kb", "put-block-output.json"): kb.PutBlockOutput,
    ("showcase", "address-open.json"): showcase.Address,
    ("showcase", "attributes.json"): showcase.Attributes,
    ("showcase", "contact.json"): showcase.ContactPy,
    ("showcase", "extras.json"): showcase.Extras,
    ("showcase", "labels.json"): showcase.Labels,
    ("showcase", "settings.json"): showcase.Settings,
    ("showcase", "showcase-bytes.json"): showcase.Showcase,
    ("showcase", "showcase-detail-object.json"): showcase.Showcase,
    ("showcase", "showcase-detail-string.json"): showcase.Showcase,
    ("showcase", "showcase-element-unions.json"): showcase.Showcase,
    ("showcase", "showcase-format.json"): showcase.Showcase,
    ("showcase", "showcase-freeform-string.json"): showcase.Showcase,
    ("showcase", "showcase-freeform.json"): showcase.Showcase,
    ("showcase", "showcase-full.json"): showcase.Showcase,
    ("showcase", "showcase-inline-shapes.json"): showcase.Showcase,
    ("showcase", "showcase-measurements-array.json"): showcase.Showcase,
    ("showcase", "showcase-measurements-string.json"): showcase.Showcase,
    ("showcase", "showcase-metrics.json"): showcase.Showcase,
    ("showcase", "showcase-minimal.json"): showcase.Showcase,
    ("showcase", "showcase-note-link.json"): showcase.Showcase,
    ("showcase", "showcase-note-text.json"): showcase.Showcase,
    ("showcase", "showcase-nulls.json"): showcase.Showcase,
    ("showcase", "showcase-patterns.json"): showcase.Showcase,
    ("showcase", "showcase-shape-circle.json"): showcase.Showcase,
    ("showcase", "showcase-shape-or-name-square.json"): showcase.Showcase,
    ("showcase", "showcase-shape-or-name-string.json"): showcase.Showcase,
    ("showcase", "showcase-shape-square.json"): showcase.Showcase,
    ("showcase", "showcase-strings.json"): showcase.Showcase,
    ("showcase", "showcase-union-int.json"): showcase.Showcase,
    ("showcase", "showcase-union-string.json"): showcase.Showcase,
    ("showcase", "widget.json"): showcase.Widget,
    ("temporal", "temporal-canonicalize.json"): temporal.Temporal,
    ("temporal", "temporal-full.json"): temporal.Temporal,
    ("temporal", "temporal-minimal.json"): temporal.Temporal,
    ("temporal", "temporal-nulls.json"): temporal.Temporal,
}

SUITES = ("chat", "kb", "showcase", "temporal")


def test_every_wire_fixture_is_declared() -> None:
    """The table covers the fixture tree exactly — no file left unswept."""
    on_disk = {
        (suite, path.name)
        for suite in SUITES
        for path in fixture_dir(suite).iterdir()
        if path.suffix == ".json"
    }
    assert on_disk == set(WIRE_FIXTURES)

    # Both exception lists are keyed by real fixtures, so a stale or misspelled
    # entry cannot sit there silently weakening the sweep.
    assert set(COLLAPSED_NULL_MEMBERS) <= on_disk
    assert NON_CANONICAL_FIXTURES <= on_disk


def test_every_wire_fixture_roundtrips_byte_identically() -> None:
    """P1 on bytes, for every fixture but the documented exceptions."""
    for (suite, name), model_type in sorted(WIRE_FIXTURES.items()):
        if (suite, name) in NON_CANONICAL_FIXTURES:
            # Deliberately non-canonical input; its expected bytes are asserted by
            # `test_temporal.test_temporal_canonicalization`.
            continue
        _ = roundtrip_fixture(model_type, suite, name)


def test_collapsed_members_are_the_only_difference() -> None:
    """Every exception entry earns its place, and drops nothing more.

    Two directions at once: the fixture with its declared members intact must
    *fail* the byte comparison (so an entry cannot be added for a fixture that
    already round-trips, silently weakening the sweep), and with them dropped it
    must pass exactly (so an entry cannot drop a member Python does re-emit).
    """
    for (suite, name), dropped in sorted(COLLAPSED_NULL_MEMBERS.items()):
        assert dropped, f"{suite}/{name} declares no collapsed member"
        model = decode_fixture(WIRE_FIXTURES[suite, name], suite, name)
        assert encode_bytes(model) != canonical_json_bytes(load_fixture(suite, name))
        assert encode_bytes(model) == canonical_fixture_bytes(suite, name)


def test_re_encoding_is_idempotent() -> None:
    """A model decoded from re-emitted bytes re-emits those same bytes.

    The loop a peer language exercises by replying with what it received: the
    encoder's own output must be a fixed point of decode-then-encode, so no
    normalization keeps drifting on each hop.
    """
    for (suite, name), model_type in sorted(WIRE_FIXTURES.items()):
        first = encode_bytes(decode(model_type, fixture_bytes(suite, name)))
        assert encode_bytes(decode(model_type, first)) == first

// Probe: MATERIALIZE model (B) in JS/TS, modelling all THREE --js-temporal-repr
// modes as separate harness engines:
//
//   js-string   (default) -- every temporal is the generator-serialized STRING
//                (built from the parsed value; offset + full precision preserved,
//                the same bytes Go/Java emit). Lossless.
//   js-temporal (--js-temporal-repr=temporal) -- Temporal.ZonedDateTime for
//                date-time, Temporal.PlainDate for date; `time` STAYS a string
//                (Temporal has no offset-bearing time-only type). Lossless.
//   js-date     (--js-temporal-repr=date, legacy) -- date-time only, via `Date`:
//                a UTC instant at millisecond resolution (offset folded to UTC,
//                sub-ms dropped). LOSSY, expected to diverge. date/time unsupported.
//
// Temporal is NOT a Node global here (v25), so we use the @js-temporal/polyfill
// installed in this dir.  node runner.mjs corpus.json
//
// FINDING: Temporal.ZonedDateTime.from does NOT reject leap `:60` -- it SILENTLY
// CLAMPS :60->:59 (even with {overflow:'reject'}), exactly like Ruby. The spec's
// "every native parser rejects :60" is therefore inaccurate for JS. In production
// the :60-rejecting materialized grammar rejects it at VALIDATION, before any
// parse; we model that here with an explicit guard so the leap row is a SKIP
// rather than a silently-clamped materialization.
import { readFileSync } from "node:fs";
import { Temporal } from "@js-temporal/polyfill";

const emit = (o) => console.log(JSON.stringify(o));

// Models the materialized node's :60-rejecting grammar (the validator gate).
function rejectLeap(wire) {
  if (/:60(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?$/i.test(wire))
    throw new Error("leap second :60 rejected by materialized grammar");
}

// "+00:00" / "-00:00" -> "Z"; leaves other offsets untouched.
const normZ = (s) => s.replace(/[+-]00:00$/, "Z");

// Build a ZonedDateTime in the WIRE's own offset zone (offset + nanosecond
// preserved). The bracket zone is the numeric offset (Z -> +00:00).
function zdtFrom(wire) {
  const w = wire.toUpperCase();
  const m = w.match(/(Z|[+-]\d{2}:\d{2})$/);
  const zone = !m || m[1] === "Z" ? "+00:00" : m[1];
  return Temporal.ZonedDateTime.from(`${w}[${zone}]`, { offset: "use" });
}

// Generator-owned serialize for a ZonedDateTime: offset kept, +00:00/-00:00 -> Z,
// fractional seconds auto-trimmed (Temporal's default 'auto').
const serializeZdt = (z) => normZ(z.toString({ timeZoneName: "never" }));

// Generator-owned serialize for a time-of-day STRING (used by js-string AND
// js-temporal, which both keep `time` as a string). Parses the validated wire
// and re-emits: trailing fractional zeros trimmed, +00:00/-00:00 -> Z, offset
// preserved when present, omitted when absent.
function serializeTimeString(wire) {
  rejectLeap(wire);
  const w = wire.toUpperCase();
  const m = w.match(/^(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(Z|[+-]\d{2}:\d{2})?$/);
  if (!m) throw new Error("unparseable time");
  const [, hh, mm, ss, frac, off] = m;
  let out = `${hh}:${mm}:${ss}`;
  if (frac) {
    const t = frac.replace(/0+$/, "");
    if (t) out += "." + t;
  }
  if (off) out += off === "Z" ? "Z" : normZ(off);
  return out;
}

// --- js-string: generator-serialized string for every temporal ---------------
function stringDateTime(wire) { rejectLeap(wire); return serializeZdt(zdtFrom(wire)); }
function stringDate(wire) { return Temporal.PlainDate.from(wire).toString(); }
const stringTime = serializeTimeString;

// --- js-temporal: ZonedDateTime / PlainDate; `time` stays a string ------------
const temporalDateTime = stringDateTime; // ZonedDateTime, serialized identically
const temporalDate = stringDate;         // PlainDate
const temporalTime = serializeTimeString; // stays a string

// --- js-date: legacy Date, date-time ONLY (UTC instant, ms) -------------------
function dateDateTime(wire) {
  rejectLeap(wire); // Date would yield Invalid Date on :60 anyway
  const d = new Date(wire.toUpperCase());
  if (Number.isNaN(d.getTime())) throw new Error("Date -> NaN");
  return d.toISOString(); // always YYYY-MM-DDTHH:MM:SS.sssZ (UTC, 3 frac)
}
function dateUnsupported() {
  throw new Error("UNSUPPORTED: js-date only materializes date-time");
}

function run(engine, rows, fmt, fn) {
  for (const r of rows) {
    try {
      emit({ id: r.id, engine, format: fmt, canonical: fn(r.wire), err: "" });
    } catch (e) {
      emit({ id: r.id, engine, format: fmt, canonical: "", err: String(e.message || e) });
    }
  }
}

const c = JSON.parse(readFileSync(process.argv[2], "utf8"));

run("js-string", c["date-time"], "date-time", stringDateTime);
run("js-string", c["date"], "date", stringDate);
run("js-string", c["time"], "time", stringTime);

run("js-temporal", c["date-time"], "date-time", temporalDateTime);
run("js-temporal", c["date"], "date", temporalDate);
run("js-temporal", c["time"], "time", temporalTime);

run("js-date", c["date-time"], "date-time", dateDateTime);
run("js-date", c["date"], "date", dateUnsupported);
run("js-date", c["time"], "time", dateUnsupported);

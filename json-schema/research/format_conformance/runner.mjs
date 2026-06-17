// JS/TS runner (node) for the JSON-Schema `format` conformance corpus.
//
// Implements the SPEC'S PINNED CHECK: a pinned anchored regex (with the `u`
// flag, mandatory per the `pattern` design) compiled once at module level, plus
// the shared integer-arithmetic calendar predicate for the temporal formats.
// This is the OWNED check -- we do NOT delegate to `Date`/`new Date()` as the
// source of truth. As a SECONDARY column we record what the native `Date`
// parser accepts, purely to document divergence.
//
// Reads corpus.json (argv[2] or ./corpus.json) and emits JSON Lines to stdout:
//   {"id","engine":"js","valid":bool,"native":bool}
//
// Run: node runner.mjs [corpus.json]
import { readFileSync } from "node:fs";

// ---- pinned patterns (anchored, `u` flag, compiled once) --------------------

const OCTET = "(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])";
const H16 = "[0-9a-fA-F]{1,4}";
const V4 = `(${OCTET}\\.${OCTET}\\.${OCTET}\\.${OCTET})`;
const LS32 = `(${H16}:${H16}|${V4})`;
const IPV6 =
  "^(" +
  `(${H16}:){6}${LS32}|` +
  `::(${H16}:){5}${LS32}|` +
  `(${H16})?::(${H16}:){4}${LS32}|` +
  `((${H16}:){0,1}${H16})?::(${H16}:){3}${LS32}|` +
  `((${H16}:){0,2}${H16})?::(${H16}:){2}${LS32}|` +
  `((${H16}:){0,3}${H16})?::(${H16}:)${LS32}|` +
  `((${H16}:){0,4}${H16})?::${LS32}|` +
  `((${H16}:){0,5}${H16})?::${H16}|` +
  `((${H16}:){0,6}${H16})?::` +
  ")$";

const UUID_RE = new RegExp(
  "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
  "u",
);
const IPV4_RE = new RegExp(`^${OCTET}\\.${OCTET}\\.${OCTET}\\.${OCTET}$`, "u");
const IPV6_RE = new RegExp(IPV6, "u");
const DATE_RE = new RegExp("^([0-9]{4})-([0-9]{2})-([0-9]{2})$", "u");
const TIME_RE = new RegExp(
  "^([0-9]{2}):([0-9]{2}):([0-9]{2})(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})?$",
  "u",
);
const DATE_TIME_RE = new RegExp(
  "^([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$",
  "u",
);

// ---- shared calendar predicate (integer arithmetic only) --------------------

const isLeap = (y) => (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0;

function daysInMonth(y, m) {
  if ([1, 3, 5, 7, 8, 10, 12].includes(m)) return 31;
  if ([4, 6, 9, 11].includes(m)) return 30;
  if (m === 2) return isLeap(y) ? 29 : 28;
  return 0;
}

const validCalendarDate = (y, m, d) =>
  m >= 1 && m <= 12 && d >= 1 && d <= daysInMonth(y, m);

const validTimeFields = (hh, mm, ss) => hh <= 23 && mm <= 59 && ss <= 60; // :60 leap accepted

function validOffset(off) {
  if (!off || off === "Z" || off === "z") return true;
  const oh = parseInt(off.slice(1, 3), 10);
  const om = parseInt(off.slice(4, 6), 10);
  return oh <= 23 && om <= 59;
}

const N = (s) => parseInt(s, 10);

// ---- pinned per-format check ------------------------------------------------

function pinnedValid(format, v) {
  switch (format) {
    case "uuid":
      return UUID_RE.test(v);
    case "ipv4":
      return IPV4_RE.test(v);
    case "ipv6":
      return IPV6_RE.test(v);
    case "date": {
      const g = DATE_RE.exec(v);
      return g ? validCalendarDate(N(g[1]), N(g[2]), N(g[3])) : false;
    }
    case "time": {
      const g = TIME_RE.exec(v);
      return g
        ? validTimeFields(N(g[1]), N(g[2]), N(g[3])) && validOffset(g[5])
        : false;
    }
    case "date-time": {
      const g = DATE_TIME_RE.exec(v);
      return g
        ? validCalendarDate(N(g[1]), N(g[2]), N(g[3])) &&
            validTimeFields(N(g[4]), N(g[5]), N(g[6])) &&
            validOffset(g[8])
        : false;
    }
  }
  return false;
}

// ---- SECONDARY: native Date parser (documentation only) ---------------------

function nativeValid(format, v) {
  switch (format) {
    case "uuid":
    case "ipv4":
    case "ipv6":
      return false; // no native address parser in the JS stdlib
    case "date":
    case "date-time": {
      const t = Date.parse(v);
      return !Number.isNaN(t);
    }
    case "time": {
      // Date has no time-only parse; anchor to an epoch date.
      const t = Date.parse("1970-01-01T" + v);
      return !Number.isNaN(t);
    }
  }
  return false;
}

const path = process.argv[2] ?? "corpus.json";
const corpus = JSON.parse(readFileSync(path, "utf8"));

for (const p of corpus.pairs) {
  process.stdout.write(
    JSON.stringify({
      id: p.id,
      engine: "js",
      valid: pinnedValid(p.format, p.value),
      native: nativeValid(p.format, p.value),
    }) + "\n",
  );
}

// TypeScript/JS materialization probe for the `duration` format.
//
// Q1: JS has NO stdlib duration type of ANY kind. No Duration, no timedelta,
//     no ISO-8601 duration parser or serializer. (Temporal.Duration is the
//     TC39 Temporal proposal - Stage 3, NOT in the Node/browser stdlib we
//     target; using it would be a runtime dependency / non-portable.)
// Q2: design B custom object {years,months,...} + hand-written serializer is
//     the only way to materialize in TS; it round-trips the full grammar.
// Q3: design C native: there is NO native fixed-duration type to narrow into,
//     so TS could only ever do B or keep string.
//
// Run: cd ts_full && node full.mjs

console.log("=== Q1: JS stdlib duration facilities ===");
console.log("  globalThis.Duration:", typeof globalThis.Duration);         // undefined
console.log("  typeof Temporal:", typeof globalThis.Temporal);             // undefined in stdlib
console.log("  Date can represent an INSTANT, not a DURATION.");
console.log("  => No stdlib fixed-duration type; no ISO duration parse/format.\n");

// ---- design B: custom object + canonical serializer (mirrors Go/Java struct) ----
function parseISO(s) {
  const d = { years:0, months:0, weeks:0, days:0, hours:0, minutes:0, seconds:0, week:false };
  let body = s.slice(1);
  if (body.startsWith("T")) { parseTime(body.slice(1), d); return d; }
  if (body.endsWith("W")) { d.week = true; d.weeks = parseInt(body.slice(0, -1), 10); return d; }
  let datePart = body;
  const ti = body.indexOf("T");
  if (ti >= 0) { datePart = body.slice(0, ti); parseTime(body.slice(ti + 1), d); }
  let num = "";
  for (const c of datePart) {
    if (c >= "0" && c <= "9") { num += c; continue; }
    const v = parseInt(num, 10);
    if (c === "Y") d.years = v; else if (c === "M") d.months = v; else if (c === "D") d.days = v;
    num = "";
  }
  return d;
}
function parseTime(t, d) {
  let num = "";
  for (const c of t) {
    if (c >= "0" && c <= "9") { num += c; continue; }
    const v = parseInt(num, 10);
    if (c === "H") d.hours = v; else if (c === "M") d.minutes = v; else if (c === "S") d.seconds = v;
    num = "";
  }
}
function serialize(d) {
  if (d.week) return `P${d.weeks}W`;
  let date = "", tim = "";
  if (d.years) date += `${d.years}Y`;
  if (d.months) date += `${d.months}M`;
  if (d.days) date += `${d.days}D`;
  if (d.hours) tim += `${d.hours}H`;
  if (d.minutes) tim += `${d.minutes}M`;
  if (d.seconds) tim += `${d.seconds}S`;
  if (!date && !tim) return "PT0S";
  return "P" + date + (tim ? "T" + tim : "");
}

console.log("=== Q2: design B custom object round-trip (full corpus) ===");
const full = ["P3Y6M4DT12H30M5S","P1Y","P2M","P10D","P4W","P1W","P1Y6M","P1Y6M4D","P6M4D","P1YT1H","P1DT12H","P100Y200M300DT400H500M600S","P0Y"];
for (const w of full) {
  const got = serialize(parseISO(w));
  const expect = w === "P0Y" ? "PT0S" : w;
  console.log(`  ${w.padEnd(30)} -> ${got.padEnd(20)} ${got === expect}`);
}

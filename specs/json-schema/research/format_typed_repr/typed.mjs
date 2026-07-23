// Probe: TypeScript/JS STANDARD LIBRARY typed reps for the 6 formats.
// Node built-ins only (no npm dep). Run: node typed.mjs
// Backs features/format typed-repr research.

function show(label, s, fn) {
  try {
    console.log(`  ${label} ${JSON.stringify(s).padEnd(40)} -> ${fn()}`);
  } catch (e) {
    console.log(`  ${label} ${JSON.stringify(s).padEnd(40)} -> ERR ${e.message}`);
  }
}

console.log("=== JS/TS stdlib typed representations ===");

// ---- date-time : the ONLY temporal type in stdlib is Date (a UTC instant) ----
console.log("\n[date-time] type=Date (via new Date(s) / Date.parse). Date is an instant; no offset retained.");
for (const s of [
  "2021-02-28T23:59:60Z",          // leap second
  "2006-01-02T15:04:05Z",
  "2006-01-02T15:04:05+00:00",
  "2006-01-02T15:04:05-00:00",
  "2006-01-02T15:04:05.123456789Z",// ns precision
  "2006-01-02t15:04:05z",          // lowercase
  "2006-01-02T15:04:05",           // missing offset -> Date treats as LOCAL
  "2021-02-30T00:00:00Z",          // bad calendar
]) {
  const d = new Date(s);
  const bad = Number.isNaN(d.getTime());
  show("Date", s, () => bad ? "Invalid Date" : `OK toISOString=${d.toISOString()}`);
}

// ---- date : NO date-only type. Date(s) with date-only string parses as UTC midnight ----
console.log("\n[date] NO date-only stdlib type. new Date('2020-02-29') -> a full instant at UTC midnight.");
for (const s of ["2020-02-29", "2021-02-29", "2021-13-01"]) {
  const d = new Date(s);
  const bad = Number.isNaN(d.getTime());
  show("Date", s, () => bad ? "Invalid Date" : `OK toISOString=${d.toISOString()}`);
}

// ---- time : NO time-only type at all ----
console.log("\n[time] NO time-only stdlib type. new Date('12:00:00') is Invalid.");
for (const s of ["12:00:00", "23:59:60Z"]) {
  const d = new Date(s);
  show("Date", s, () => Number.isNaN(d.getTime()) ? "Invalid Date" : d.toISOString());
}

// ---- uuid : NO UUID class. crypto.randomUUID GENERATES but there's no parse/validate type ----
console.log("\n[uuid] NO UUID type. node:crypto.randomUUID() only GENERATES; there is no parse-into-typed-value API.");
try {
  const { randomUUID } = await import("node:crypto");
  console.log("  crypto.randomUUID() sample:", randomUUID(), "(returns a plain string, not a typed object)");
} catch (e) { console.log("  crypto err", e.message); }

// ---- ipv4 / ipv6 : node:net has isIP/isIPv4/isIPv6 (validators) but returns NO typed object ----
console.log("\n[ipv4/ipv6] node:net.isIP(s) VALIDATES (returns 4/6/0) but yields NO typed address object.");
const net = await import("node:net");
for (const s of ["192.168.0.1", "256.0.0.1", "01.2.3.4", "1.2.3", "::1", "2001:DB8::1", "::ffff:192.168.0.1"]) {
  console.log(`  net.isIP(${JSON.stringify(s).padEnd(24)}) = ${net.isIP(s)}  isIPv4=${net.isIPv4(s)} isIPv6=${net.isIPv6(s)}`);
}
console.log("  (note: isIP is only in Node runtime, NOT in browsers / the ECMAScript stdlib.)");

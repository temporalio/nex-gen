// Emit canonical re-serialization JSON for the corpus (JS/Node).
// JS has NO native duration type, so BOTH groups use design B (custom object).
// For `timeonly` we compute the same total-based canonical by hand.
import { readFileSync } from "node:fs";

function parseISO(s) {
  const d = { y:0, mo:0, w:0, d:0, h:0, mi:0, s:0, week:false };
  let body = s.slice(1);
  if (body.startsWith("T")) { pt(body.slice(1), d); return d; }
  if (body.endsWith("W")) { d.week = true; d.w = parseInt(body.slice(0,-1),10); return d; }
  let datePart = body; const ti = body.indexOf("T");
  if (ti >= 0) { datePart = body.slice(0,ti); pt(body.slice(ti+1), d); }
  let num = "";
  for (const c of datePart) {
    if (c>="0"&&c<="9") { num+=c; continue; }
    const v = parseInt(num,10);
    if (c==="Y") d.y=v; else if (c==="M") d.mo=v; else if (c==="D") d.d=v; num="";
  }
  return d;
}
function pt(t, d) {
  let num = "";
  for (const c of t) {
    if (c>="0"&&c<="9") { num+=c; continue; }
    const v = parseInt(num,10);
    if (c==="H") d.h=v; else if (c==="M") d.mi=v; else if (c==="S") d.s=v; num="";
  }
}
function serializeB(d) {
  if (d.week) return `P${d.w}W`;
  let date="", tim="";
  if (d.y) date+=`${d.y}Y`; if (d.mo) date+=`${d.mo}M`; if (d.d) date+=`${d.d}D`;
  if (d.h) tim+=`${d.h}H`; if (d.mi) tim+=`${d.mi}M`; if (d.s) tim+=`${d.s}S`;
  if (!date && !tim) return "PT0S";
  return "P"+date+(tim?"T"+tim:"");
}
function nativeCanonical(s) {
  const d = parseISO(s);
  const total = d.h*3600 + d.mi*60 + d.s;
  const h = Math.floor(total/3600), m = Math.floor((total%3600)/60), sec = total%60;
  let out = "PT";
  if (h) out+=`${h}H`; if (m) out+=`${m}M`; if (sec||(h===0&&m===0)) out+=`${sec}S`;
  return out;
}

const corpus = JSON.parse(readFileSync("corpus.json","utf8"));
const out = { full:{}, timeonly:{} };
for (const r of corpus.full) out.full[r.id] = serializeB(parseISO(r.wire));
for (const r of corpus.timeonly) out.timeonly[r.id] = nativeCanonical(r.wire);
console.log(JSON.stringify(out));

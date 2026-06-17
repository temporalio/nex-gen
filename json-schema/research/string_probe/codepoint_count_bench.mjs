// Benchmark: fastest way to enforce a code-point length bound in JS.
//
// Motivating question: is there a cheaper code-point count than the obvious
// `[...s].length`, which allocates a full array of code points? Drives the
// TypeScript validator rows in features/maxLength and features/minLength.
//
// Findings (100k-char string, 2000 iterations; see NOTES.md for the table):
//   - [...s].length  : allocates a code-point array          (baseline)
//   - for (const _ of s): no array, but iterator overhead ≈ baseline (NOT a win)
//   - surrogate scan : allocation-free single pass           (~3.5× faster)
//   - early-exit     : stop once the bound is crossed        (work bounded by
//                      the bound, not the input length — the real win)

// Reference: spread iterates by code point.
const spread = (s) => [...s].length;

// for...of also iterates by code point, no array — but the iterator is not free.
const forOf = (s) => { let n = 0; for (const _ of s) n++; return n; };

// Allocation-free full count: UTF-16 length minus one per well-formed
// surrogate pair. This is the shared `codePointLength` helper the specs emit.
export function codePointLength(s) {
  let n = s.length;
  for (let i = 0; i < s.length - 1; i++) {
    const c = s.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff) {                 // high surrogate
      const d = s.charCodeAt(i + 1);
      if (d >= 0xdc00 && d <= 0xdfff) { n--; i++; }   // low surrogate -> one code point
    }
  }
  return n;
}

// The validator form: answer "does the code-point count exceed max?" without
// counting the whole string. Stops as soon as the bound is crossed.
export function exceedsCodePointMax(s, max) {
  let n = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const d = s.charCodeAt(i + 1);
      if (d >= 0xdc00 && d <= 0xdfff) i++;            // skip the low surrogate
    }
    if (++n > max) return true;
  }
  return false;
}

// --- correctness: forOf, codePointLength must equal the spread reference ---
const cases = ["", "abc", "a\u{1F600}b", "\u{1F600}\u{1F600}",
               "é", "é", "\u{1F600}", "lone\uD800half",
               "x".repeat(1000) + "\u{1F600}"];
let ok = true;
for (const s of cases) {
  const r = spread(s);
  if (forOf(s) !== r || codePointLength(s) !== r) {
    ok = false;
    console.log("MISMATCH", JSON.stringify(s), { spread: r, forOf: forOf(s), scan: codePointLength(s) });
  }
}
console.log("correctness (forOf & codePointLength == [...s].length):", ok);
console.log("exceedsCodePointMax('a😀b', 2):", exceedsCodePointMax("a\u{1F600}b", 2), "(expect true)");
console.log("exceedsCodePointMax('a😀b', 3):", exceedsCodePointMax("a\u{1F600}b", 3), "(expect false)");

// --- perf: 100k-char string with scattered astral chars ---
const big = "hello \u{1F600} world ".repeat(5000);
const bench = (name, fn) => {
  const t = process.hrtime.bigint();
  let acc = 0;
  for (let k = 0; k < 2000; k++) acc += fn(big) ? 1 : 0;
  const ms = Number(process.hrtime.bigint() - t) / 1e6;
  console.log(`${name.padEnd(22)} ${ms.toFixed(1)}ms`);
  return acc;
};
bench("[...s].length", (s) => spread(s) >= 0);
bench("for...of", (s) => forOf(s) >= 0);
bench("codePointLength (scan)", (s) => codePointLength(s) >= 0);
bench("exceedsMax@8 (early-exit)", (s) => exceedsCodePointMax(s, 8));

# Why native email validators are unsuitable as the source of truth

The whole reason JSON Schema made `format` assertion *optional* is that native
email validators are the single most divergent corner of the ecosystem. This
file records empirical evidence (probes run 2026-07) that they (a) disagree with
each other and (b) each accept a different, much larger language than the pinned
RE2-safe subset. Using any of them would violate **P1** (identical cross-language
verdicts) and most add a **P4** dependency.

## Probe results (same 8 addresses, native validators)

| address                | pinned subset | .NET `MailAddress` | Python `email.utils.parseaddr` |
|------------------------|:-------------:|:------------------:|:------------------------------:|
| `user@example.com`     | valid         | valid              | parses (`user@example.com`)    |
| `user@localhost`       | **invalid**   | **valid**          | parses (`user@localhost`)      |
| `a@b`                  | **invalid**   | **valid**          | parses (`a@b`)                 |
| `user@[192.0.2.1]`     | **invalid**   | **valid**          | **rejects** (empty)            |
| `"a b"@x.com`          | **invalid**   | **valid**          | parses (quoted kept)           |
| `user(comment)@x.com`  | **invalid**   | **valid**          | (parses)                       |
| `usér@example.com`     | **invalid**   | **valid** (IDN)    | (parses)                       |
| `user@例え.jp`         | **invalid**   | **valid** (IDN)    | (parses)                       |
| `plain` (no `@`)       | **invalid**   | invalid            | **"parses"** to `plain`        |

`.NET MailAddress` accepts essentially everything — single-label domains,
IP-literals, quoted locals, comments, and full IDN/Unicode — i.e. it validates a
*superset* of even RFC 5321, and there is no option to narrow it. Python
`email.utils.parseaddr` never really rejects (it returns a best-effort parse;
`plain` comes back as a "valid" address with no `@`), so it is not a validator at
all.

## The others (documented, not all installed)

- **Java Bean Validation `@Email`** (Hibernate Validator): uses its own regex,
  historically permissive; accepts `user@localhost` and IDN forms, and is a
  **third-party dependency** (jakarta.validation + an implementation) — a hard
  **P4** violation for generated code that must run under the default Temporal
  converter.
- **Pydantic `EmailStr`**: delegates to the **`email-validator`** PyPI package
  (a runtime dependency — **P4**), which does full RFC-ish parsing *plus DNS-
  deliverability-oriented normalization* and lowercases/normalizes the domain
  (**changes the wire value** — a P12 hazard), and accepts IDN. Its accepted
  language is different again from `.NET`/Java.
- **Go**: `net/mail.ParseAddress` follows RFC 5322 *address* syntax (allows
  display names, comments, quoted locals) — a different and larger language than
  all of the above.
- **JS/TS**: no stdlib email validator; every library (validator.js, zod's
  `.email()`, etc.) picks its own regex — none agree.

## Conclusion

No two native validators accept the same language, several add a dependency, and
some mutate the value. The only way to get **identical** accept/reject across all
seven targets (P1) with **no new dependency** (P4) is a **generator-owned pinned
regex** — which is exactly what `corpus.json` + the seven runners verify.

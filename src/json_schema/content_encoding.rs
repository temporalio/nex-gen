//! The materialized `contentEncoding` subset (JSON Schema 2020-12 §8.3). We opt
//! into assertion + materialization for the two byte-transform encodings —
//! **`base64`** (standard alphabet, padded) and **`base64url`** (URL-safe
//! alphabet, unpadded, RFC 4648 §5) — lowering both to a language-native bytes
//! type (Go `[]byte`, Java `byte[]`, Python `bytes`, TS `Uint8Array`). Every
//! other encoding is rejected at load. See
//! `specs/json-schema/features/contentEncoding.md` for the authoritative rules.
//!
//! Like [[format]], the validity check is a **generator-owned** pinned regex
//! over the wire string (through the [[pattern]] RE2-safe gate) — not a
//! decoder's own (lenient) error behavior — so a value accepted (or rejected) by
//! one language is accepted (or rejected) by all (P1). The regex is the oracle;
//! the stdlib / generator-owned decoder runs only after it passes.

/// The two byte-transform encodings we materialize. Both lower to the same
/// native bytes type and differ only in the canonical wire codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Standard alphabet (`+`/`/`), required `=` padding (RFC 4648 §4).
    Base64,
    /// URL-safe alphabet (`-`/`_`), **no** padding (RFC 4648 §5).
    Base64Url,
}

/// The encodings we materialize, in canonical order, for the unsupported-encoding
/// fix-it.
pub const SUPPORTED_ENCODINGS: [&str; 2] = ["base64", "base64url"];

impl Encoding {
    /// The `Encoding` for a `contentEncoding` value, or `None` for any other
    /// (unsupported) encoding name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "base64" => Some(Self::Base64),
            "base64url" => Some(Self::Base64Url),
            _ => None,
        }
    }

    /// The canonical encoding name (`base64` / `base64url`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Base64 => "base64",
            Self::Base64Url => "base64url",
        }
    }

    /// The pinned, anchored (`^…$`) validity regex for this encoding. `base64`
    /// accepts canonical **padded** standard base64; `base64url` accepts
    /// canonical **unpadded** URL-safe base64. Both accept the empty string
    /// (→ zero bytes) and reject the *other* encoding's alphabet, wrong padding,
    /// embedded whitespace, and stray characters. Emitted verbatim to Go/JS and
    /// with the [[pattern]] per-target end-anchor rewrite to Python/Java.
    ///
    /// **Trailing bits are constrained** so the wire form is *canonical* — the
    /// byte-identity claim the spec makes ("only the canonical form is accepted
    /// … the wire round-trips byte-identically with no re-canonicalization
    /// step") is load-bearing for a sibling `const`/`enum`/`pattern`/`maxLength`
    /// on the same node, and every target's decoder happily ignores non-zero
    /// unused bits. A final quantum written as **two** significant characters
    /// carries one byte, so the second character's low four bits must be zero
    /// (`[AQgw]`, alphabet indices 0/16/32/48); written as **three** characters
    /// it carries two bytes, so the third character's low two bits must be zero
    /// (`[AEIMQUYcgkosw048]`, the indices divisible by four). Without this,
    /// `"aGl="`, `"AB=="`, `"//9="` and base64url `"aGl"` all match, decode
    /// fine, and re-encode to a *different* string (`"aGk="`, `"AA=="`,
    /// `"//8="`, `"aGk"`) — measured end-to-end in Go, where `{"req":"aGl="}`
    /// marshals back as `{"req":"aGk="}`.
    pub fn pattern(self) -> &'static str {
        match self {
            Self::Base64 => concat!(
                "^(?:[A-Za-z0-9+/]{4})*",
                "(?:[A-Za-z0-9+/][AQgw]==|[A-Za-z0-9+/]{2}[AEIMQUYcgkosw048]=)?$"
            ),
            Self::Base64Url => concat!(
                "^(?:[A-Za-z0-9_-]{4})*",
                "(?:[A-Za-z0-9_-][AQgw]|[A-Za-z0-9_-]{2}[AEIMQUYcgkosw048])?$"
            ),
        }
    }
}

/// The load-time classification of a `contentEncoding` value.
pub enum EncodingClass {
    /// A supported byte-transform encoding lowering to a native bytes field.
    Supported(Encoding),
    /// Any other encoding (`quoted-printable`, `7bit`, `8bit`, `binary`,
    /// `base16`, unknown) — rejected at load.
    Unsupported,
}

/// Classifies a `contentEncoding` name for the load gate.
pub fn classify(name: &str) -> EncodingClass {
    match Encoding::from_name(name) {
        Some(encoding) => EncodingClass::Supported(encoding),
        None => EncodingClass::Unsupported,
    }
}

/// Runtime-equivalent verdict for a wire string under an encoding: the pinned
/// regex the generators emit. Used at load to validate `const`/`default`/`enum`
/// string literals (the literal-vs-constraint obligation) and as the oracle for
/// the regression tests. A value that passes is well-formed canonical base64 /
/// base64url and decodes to bytes; a value that fails is rejected.
pub fn is_valid(encoding: Encoding, value: &str) -> bool {
    // The load gate proves the pinned pattern compiles; recompiling here (load
    // path only) is fine.
    regex::Regex::new(encoding.pattern())
        .expect("pinned contentEncoding pattern compiles")
        .is_match(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_patterns_pass_the_pattern_gate() {
        for name in SUPPORTED_ENCODINGS {
            let encoding = Encoding::from_name(name).expect("supported");
            crate::json_schema::pattern::gate_and_normalize(encoding.pattern()).unwrap_or_else(
                |error| panic!("{name} pinned pattern rejected by gate: {error:?}"),
            );
        }
    }

    #[test]
    fn classify_partitions_names() {
        assert!(matches!(
            classify("base64"),
            EncodingClass::Supported(Encoding::Base64)
        ));
        assert!(matches!(
            classify("base64url"),
            EncodingClass::Supported(Encoding::Base64Url)
        ));
        for name in [
            "base16",
            "quoted-printable",
            "7bit",
            "8bit",
            "binary",
            "hex",
        ] {
            assert!(
                matches!(classify(name), EncodingClass::Unsupported),
                "{name}"
            );
        }
    }

    #[test]
    fn base64_accepts_canonical_padded_rejects_url_and_unpadded() {
        // ">>>" -> "Pj4+" (padded standard).
        assert!(is_valid(Encoding::Base64, "Pj4+"));
        // "hi" -> "aGk=".
        assert!(is_valid(Encoding::Base64, "aGk="));
        // Empty string -> zero bytes.
        assert!(is_valid(Encoding::Base64, ""));
        // URL-safe alphabet is rejected under base64.
        assert!(!is_valid(Encoding::Base64, "Pj4-"));
        assert!(!is_valid(Encoding::Base64, "a-b_"));
        // Missing padding is rejected under base64.
        assert!(!is_valid(Encoding::Base64, "aGk"));
        // Embedded whitespace / stray characters are rejected.
        assert!(!is_valid(Encoding::Base64, "aG k="));
        assert!(!is_valid(Encoding::Base64, "aGk=\n"));
        assert!(!is_valid(Encoding::Base64, "aGk!"));
    }

    #[test]
    fn base64url_accepts_canonical_unpadded_rejects_std_and_padding() {
        // ">>>" -> "Pj4-" (unpadded URL-safe).
        assert!(is_valid(Encoding::Base64Url, "Pj4-"));
        // "hi" -> "aGk" (unpadded).
        assert!(is_valid(Encoding::Base64Url, "aGk"));
        assert!(is_valid(Encoding::Base64Url, ""));
        // Standard alphabet is rejected under base64url.
        assert!(!is_valid(Encoding::Base64Url, "Pj4+"));
        // Padding is rejected under base64url.
        assert!(!is_valid(Encoding::Base64Url, "aGk="));
        // A single trailing char (invalid base64 length) is rejected.
        assert!(!is_valid(Encoding::Base64Url, "aGk=A"));
        assert!(!is_valid(Encoding::Base64Url, "a"));
    }

    /// Decision **D1** / `10#4`: the unused low bits of the last significant
    /// character must be zero, so `decode(v)` → `encode(…)` is the identity and
    /// the wire round-trips byte-identically with no re-canonicalization step.
    #[test]
    fn rejects_non_canonical_trailing_bits() {
        // Three significant characters + `=`: the third carries only four
        // significant bits. "aGk=" is canonical for "hi"; "aGl=" decodes to the
        // same bytes and re-encodes as "aGk=".
        assert!(is_valid(Encoding::Base64, "aGk="));
        assert!(!is_valid(Encoding::Base64, "aGl="));
        assert!(is_valid(Encoding::Base64, "//8="));
        assert!(!is_valid(Encoding::Base64, "//9="));
        // Two significant characters + `==`: the second carries only two
        // significant bits.
        assert!(is_valid(Encoding::Base64, "AA=="));
        assert!(!is_valid(Encoding::Base64, "AB=="));
        assert!(is_valid(Encoding::Base64, "/w=="));
        assert!(!is_valid(Encoding::Base64, "/x=="));
        // Same rule, unpadded, over the URL-safe alphabet.
        assert!(is_valid(Encoding::Base64Url, "aGk"));
        assert!(!is_valid(Encoding::Base64Url, "aGl"));
        assert!(is_valid(Encoding::Base64Url, "AA"));
        assert!(!is_valid(Encoding::Base64Url, "AB"));
        // A leading full quantum does not change the rule for the final one.
        assert!(is_valid(Encoding::Base64, "YWJjaGk="));
        assert!(!is_valid(Encoding::Base64, "YWJjaGl="));
        // Every canonical padded/unpadded encoding of a byte string is accepted:
        // exhaustively over all one- and two-byte payloads.
        for high in 0u16..=255 {
            let one = [high as u8];
            assert!(is_valid(Encoding::Base64, &encode(&one, true)), "{one:?}");
            assert!(
                is_valid(Encoding::Base64Url, &encode(&one, false)),
                "{one:?}"
            );
            let two = [high as u8, (high * 7 % 256) as u8];
            assert!(is_valid(Encoding::Base64, &encode(&two, true)), "{two:?}");
            assert!(
                is_valid(Encoding::Base64Url, &encode(&two, false)),
                "{two:?}"
            );
        }
    }

    /// A minimal reference encoder (the generator does not depend on a base64
    /// crate) used to prove the tightened regex accepts every canonical form.
    fn encode(bytes: &[u8], standard: bool) -> String {
        const STD: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        const URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let alphabet = if standard { STD } else { URL };
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let mut buffer = [0u8; 3];
            buffer[..chunk.len()].copy_from_slice(chunk);
            let triple =
                u32::from(buffer[0]) << 16 | u32::from(buffer[1]) << 8 | u32::from(buffer[2]);
            let significant = chunk.len() + 1;
            for index in 0..significant {
                let shift = 18 - 6 * index;
                out.push(alphabet[((triple >> shift) & 0x3F) as usize] as char);
            }
            if standard {
                for _ in significant..4 {
                    out.push('=');
                }
            }
        }
        out
    }
}

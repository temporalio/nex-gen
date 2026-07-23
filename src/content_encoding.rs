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
    pub fn pattern(self) -> &'static str {
        match self {
            Self::Base64 => "^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$",
            Self::Base64Url => "^(?:[A-Za-z0-9_-]{4})*(?:[A-Za-z0-9_-]{2,3})?$",
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
            crate::pattern::gate_and_normalize(encoding.pattern()).unwrap_or_else(|error| {
                panic!("{name} pinned pattern rejected by gate: {error:?}")
            });
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
}

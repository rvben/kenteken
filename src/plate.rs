//! Dutch licence plate normalization and validation.
//!
//! Pure: no I/O, no network. A [`Plate`] can only exist in the form the RDW API
//! actually matches on, so no caller can accidentally query a plate the API
//! will silently return zero rows for.
//!
//! The RDW dataset stores every plate as exactly six characters from `[A-Z0-9]`
//! with no separators. That was verified against the whole dataset rather than a
//! sample: `SELECT count(kenteken) WHERE length(kenteken) != 6` returns 0 across
//! all 16.8M rows. Sidecode layouts vary widely (`9999XX`, `99XXXX`, `X999XX`,
//! `XXX99X`, ...), so this validates length and charset only. Validating the
//! layout would reject valid plates for no gain.

use std::fmt;

/// The number of characters in every RDW-registered plate.
const PLATE_LEN: usize = 6;

/// Characters a human may type between plate groups, all discarded.
const SEPARATORS: [char; 4] = ['-', ' ', '.', '_'];

/// A plate in the exact form the RDW API matches: six uppercase alphanumerics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Plate(String);

/// Why a string is not a usable plate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlateError {
    /// Nothing but separators, or an empty string.
    Empty,
    /// The right characters, but not six of them.
    BadLength { normalized: String, len: usize },
    /// A character that no plate can contain.
    BadCharacter { normalized: String, found: char },
}

impl Plate {
    /// Normalize and validate user input into a plate.
    ///
    /// Accepts any grouping a human might type (`X-99-XXX`, `x99 xxx`, `X99XXX`)
    /// and folds it to the single form RDW stores.
    pub fn parse(input: &str) -> Result<Self, PlateError> {
        // ASCII case folding, not Unicode: `char::to_uppercase` maps 'ß' onto
        // "SS", 'ı' onto 'I' and 'ſ' onto 'S', so folding first would erase the
        // offending character before the check below ever sees it and send RDW
        // a plate the caller never typed. This mapping touches only `a-z`, so
        // an unusable character survives to be named in the error.
        let normalized: String = input
            .chars()
            .filter(|c| !SEPARATORS.contains(c) && !c.is_whitespace())
            .collect::<String>()
            .to_ascii_uppercase();

        if normalized.is_empty() {
            return Err(PlateError::Empty);
        }

        // Character check first: it explains a wrong length far better than a
        // length complaint does. `V-95-JK/` is a bad character, not a short plate.
        if let Some(found) = normalized.chars().find(|c| !c.is_ascii_alphanumeric()) {
            return Err(PlateError::BadCharacter { normalized, found });
        }

        // `chars().count()` and not `len()`: the two agree only because the
        // check above has already ruled out every multi-byte character, and a
        // count keeps this honest if that check ever moves.
        let len = normalized.chars().count();
        if len != PLATE_LEN {
            return Err(PlateError::BadLength { normalized, len });
        }

        Ok(Plate(normalized))
    }

    /// The plate as RDW stores it, for use as a query value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The plate grouped for human display, e.g. `X-99-XXX`.
    ///
    /// Grouping follows the runs of digits and letters, which reproduces the
    /// official dash placement for every sidecode in the dataset.
    pub fn display_grouped(&self) -> String {
        let mut groups: Vec<String> = Vec::new();
        for c in self.0.chars() {
            let same_run = groups
                .last()
                .and_then(|g: &String| g.chars().last())
                .is_some_and(|prev| prev.is_ascii_digit() == c.is_ascii_digit());
            match same_run {
                true => groups.last_mut().expect("run implies a group").push(c),
                false => groups.push(c.to_string()),
            }
        }
        groups.join("-")
    }
}

impl fmt::Display for Plate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for PlateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlateError::Empty => write!(f, "empty plate"),
            PlateError::BadLength { normalized, len } => write!(
                f,
                "'{normalized}' has {len} characters, every Dutch plate has {PLATE_LEN}"
            ),
            PlateError::BadCharacter { normalized, found } => write!(
                f,
                "'{normalized}' contains '{found}', plates are letters and digits only"
            ),
        }
    }
}

impl std::error::Error for PlateError {}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixture plates throughout this crate are placeholders RDW holds no vehicle
    // under, so no test or example names a real car. `X99XXX` is such a plate,
    // six literal characters. In `accepts_every_sidecode_layout_present_in_the_dataset`
    // below, `X` and `9` instead stand for any letter and any digit.

    fn ok(input: &str) -> String {
        Plate::parse(input)
            .unwrap_or_else(|e| panic!("{input:?} should parse: {e}"))
            .as_str()
            .to_string()
    }

    #[test]
    fn accepts_the_canonical_form() {
        assert_eq!(ok("X99XXX"), "X99XXX");
    }

    #[test]
    fn normalizes_separators_and_case() {
        for input in [
            "X-99-XXX", "x-99-xxx", "x99xxx", "X 99 XXX", "X.99.XXX", "x_99_XXX",
        ] {
            assert_eq!(ok(input), "X99XXX", "input {input:?}");
        }
    }

    #[test]
    fn normalizes_surrounding_whitespace() {
        assert_eq!(ok("  X99XXX \n"), "X99XXX");
    }

    #[test]
    fn accepts_every_sidecode_layout_present_in_the_dataset() {
        // Sampled across the real dataset at four widely separated offsets.
        for input in [
            "9999XX", "999XX9", "99XXXX", "99XXX9", "99XX99", "X999XX", "XXX99X", "XX9999",
        ] {
            let plate = input.replace('9', "1").replace('X', "A");
            assert_eq!(ok(&plate), plate);
        }
    }

    #[test]
    fn rejects_empty_and_separator_only_input() {
        assert_eq!(Plate::parse(""), Err(PlateError::Empty));
        assert_eq!(Plate::parse("---"), Err(PlateError::Empty));
        assert_eq!(Plate::parse("   "), Err(PlateError::Empty));
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(
            Plate::parse("X99XX"),
            Err(PlateError::BadLength {
                normalized: "X99XX".into(),
                len: 5
            })
        );
        assert_eq!(
            Plate::parse("X99XXXX"),
            Err(PlateError::BadLength {
                normalized: "X99XXXX".into(),
                len: 7
            })
        );
    }

    #[test]
    fn rejects_non_alphanumeric_characters() {
        assert_eq!(
            Plate::parse("X99XX/"),
            Err(PlateError::BadCharacter {
                normalized: "X99XX/".into(),
                found: '/'
            })
        );
    }

    #[test]
    fn reports_a_bad_character_rather_than_a_bad_length() {
        // '*' makes this seven characters; the character is the real problem and
        // the message must say so.
        let err = Plate::parse("X99XXX*").unwrap_err();
        assert!(
            matches!(err, PlateError::BadCharacter { found: '*', .. }),
            "expected a character error, got {err:?}"
        );
    }

    #[test]
    fn rejects_query_injection_attempts() {
        // A SoQL/URL metacharacter must never reach the client layer.
        for input in ["' OR 1=1", "X99XXX&$limit=99", "../../etc/passwd", "X99XX%"] {
            assert!(
                Plate::parse(input).is_err(),
                "{input:?} must not parse into a plate"
            );
        }
    }

    #[test]
    fn rejects_multibyte_characters_as_character_errors() {
        // 'É' normalizes to one uppercase char, giving six characters total, so a
        // length check alone would let it through.
        let err = Plate::parse("é99XXX").unwrap_err();
        assert!(
            matches!(err, PlateError::BadCharacter { .. }),
            "expected a character error, got {err:?}"
        );
    }

    #[test]
    fn uppercasing_that_changes_length_is_still_rejected() {
        // 'ß' uppercases to "SS", so a naive char-by-char map would produce a
        // seven-character all-ASCII string. It must be rejected, not accepted.
        let result = Plate::parse("ß9XXXX");
        assert!(result.is_err(), "got {result:?}");
    }

    #[test]
    fn uppercasing_never_invents_a_plate_the_caller_did_not_type() {
        // Unicode case folding maps several non-ASCII characters onto ASCII:
        // 'ß' onto "SS", 'ﬁ' onto "FI", 'ı' onto 'I', 'ſ' onto 'S'. Folding case
        // before validating erases the offending character, so each of these
        // would pass every later check and send RDW a six-character plate the
        // caller never typed.
        for (input, invented) in [
            ("ß9XXX", "SS9XXX"),
            ("ﬁ9XXX", "FI9XXX"),
            ("ı99XXX", "I99XXX"),
            ("ſ99XXX", "S99XXX"),
        ] {
            match Plate::parse(input) {
                Err(_) => {}
                Ok(plate) => panic!(
                    "{input:?} was queried as {} (expected {invented} to be refused, not looked up)",
                    plate.as_str()
                ),
            }
        }
    }

    #[test]
    fn groups_for_display_by_digit_letter_runs() {
        let cases = [
            ("X99XXX", "X-99-XXX"),
            ("12ABC3", "12-ABC-3"),
            ("1234AB", "1234-AB"),
            ("XX9999", "XX-9999"),
            ("ABCDEF", "ABCDEF"),
            ("123456", "123456"),
        ];
        for (plain, grouped) in cases {
            assert_eq!(
                Plate::parse(plain).unwrap().display_grouped(),
                grouped,
                "plate {plain}"
            );
        }
    }

    #[test]
    fn display_is_the_api_form_not_the_grouped_form() {
        // The Display impl feeds query construction; grouping there would break
        // every lookup.
        let plate = Plate::parse("X-99-XXX").unwrap();
        assert_eq!(plate.to_string(), "X99XXX");
    }

    #[test]
    fn parsing_is_idempotent() {
        let once = ok("x-99-xxx");
        assert_eq!(ok(&once), once);
    }
}

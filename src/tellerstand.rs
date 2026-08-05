//! The RDW odometer-judgement explanations, embedded at compile time.
//!
//! The register says whether a vehicle's odometer readings are `Logisch` or
//! `Onlogisch`, and separately carries a two-character code saying *why*. The
//! codes are the interesting part: `05` means the vehicle was registered outside
//! the Netherlands, `02` that the odometer was replaced or repaired, `04` that a
//! reading came in lower than the one before it. Nine codes and 2 KB, so the
//! table ships inside the binary and a lookup costs no request.
//!
//! Refresh it with `make update-tellerstand`, which rewrites
//! `data/tellerstand.json` from RDW so the change is reviewable as a diff.
//!
//! A code this build does not know resolves to `None`, never to a guess.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The table as shipped, keyed by `code_toelichting_tellerstandoordeel`.
const EMBEDDED: &str = include_str!("../data/tellerstand.json");

/// The code RDW uses when it has registered no odometer judgement at all.
///
/// Its explanation is the bare words "Niet geregistreerd.", which is RDW's
/// placeholder rather than a reason, so it resolves to `None` like every other
/// placeholder this tool meets.
const NOT_REGISTERED: &str = "NG";

/// Parse the table once, on first use.
fn table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(EMBEDDED).expect("embedded tellerstand table is valid JSON")
    })
}

/// RDW's explanation of an odometer judgement, or `None` when there is none.
pub fn explain(code: &str) -> Option<&'static str> {
    if code == NOT_REGISTERED {
        return None;
    }
    table().get(code).map(String::as_str)
}

/// How many codes this build knows, including the placeholder.
pub fn len() -> usize {
    table().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_table_parses_and_is_populated() {
        assert!(len() >= 9, "only {} codes embedded", len());
    }

    #[test]
    fn explains_the_codes_that_say_why_no_judgement_was_given() {
        // Verified against the live RDW table. These two are the ones that
        // answer the question a reader actually has when a judgement is absent.
        assert!(
            explain("05")
                .expect("05 is known")
                .contains("buiten Nederland"),
            "got {:?}",
            explain("05")
        );
        assert!(
            explain("02").expect("02 is known").contains("vervangen"),
            "got {:?}",
            explain("02")
        );
    }

    #[test]
    fn the_not_registered_placeholder_resolves_to_none() {
        // The table's own text for NG is "Niet geregistreerd.", which is RDW's
        // way of saying it has nothing. Passing that through would put a
        // confident-looking sentence where there is no reason at all.
        assert_eq!(explain(NOT_REGISTERED), None);
        assert!(
            table().contains_key(NOT_REGISTERED),
            "the placeholder is kept in the table so a refresh diff stays honest"
        );
    }

    #[test]
    fn an_unknown_code_is_none_not_an_empty_string() {
        assert_eq!(explain("ZZ"), None);
        assert_eq!(explain(""), None);
    }

    #[test]
    fn no_entry_has_an_empty_explanation() {
        for (code, text) in table() {
            assert!(
                !text.trim().is_empty(),
                "code {code} has a blank explanation"
            );
        }
    }
}

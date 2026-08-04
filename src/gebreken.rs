//! The RDW defect-code table, embedded at compile time.
//!
//! `kenteken defects` returns codes like `AC4`; on their own they are useless.
//! The full table is 1007 codes and 54 KB, small enough to ship inside the
//! binary, which keeps `defects` to a single request and keeps it working
//! offline. Refresh it with `make update-gebreken`, which rewrites
//! `data/gebreken.json` from RDW so the change is reviewable as a diff.
//!
//! RDW revises the table occasionally, so an embedded copy can lag. A code that
//! is not in it renders as `null`, never as a guess or an empty string: an
//! unrecognised code and a code meaning "no defect" must stay distinguishable.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The table as shipped, keyed by `gebrek_identificatie`.
const EMBEDDED: &str = include_str!("../data/gebreken.json");

/// Parse the table once, on first use, so a run that never looks up a defect
/// code never pays for it.
fn table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(EMBEDDED).expect("embedded gebreken table is valid JSON")
    })
}

/// The description for a defect code, or `None` if this build does not know it.
pub fn describe(code: &str) -> Option<&'static str> {
    table().get(code).map(String::as_str)
}

/// How many codes this build knows.
pub fn len() -> usize {
    table().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_table_parses_and_is_populated() {
        assert!(len() > 900, "only {} codes embedded", len());
    }

    #[test]
    fn describes_a_known_code() {
        // Verified against the live RDW code table.
        assert_eq!(
            describe("AC4"),
            Some("Stuur- of fuseekogel met slijtage kleiner of gelijk 1,0 mm")
        );
    }

    #[test]
    fn an_unknown_code_is_none_not_an_empty_string() {
        // The distinction matters: a consumer must be able to tell "this build
        // does not know the code" from "the code means nothing".
        assert_eq!(describe("ZZZZZZ"), None);
        assert_eq!(describe(""), None);
    }

    #[test]
    fn lookup_is_case_sensitive_because_rdw_codes_are_uppercase() {
        assert!(describe("AC4").is_some());
        assert!(describe("ac4").is_none());
    }

    #[test]
    fn no_entry_has_an_empty_description() {
        // An empty description would render as a blank column that looks like a
        // successful lookup of nothing.
        for (code, desc) in table() {
            assert!(
                !desc.trim().is_empty(),
                "code {code} has a blank description"
            );
        }
    }
}

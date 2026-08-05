//! Following a recall from a plate to what it is actually about.
//!
//! RDW splits a recall across three datasets. `terugroepactie-status` says which
//! recalls apply to a plate and whether each is still open, but carries nothing
//! else: two status columns and a nine-character reference code. The defect, the
//! repair and who to call live in `terugroepactie`, and the hazard in
//! `terugroepactie-risico`, both keyed by that reference rather than by plate.
//!
//! So answering "what is wrong with this car" needs a second hop, and this
//! module is it. Every reference a run collected is resolved in one request per
//! dataset, whether that is one recall or forty, because a request per recall
//! against a free public service is not a reasonable way to ask.

use crate::error::KentekenError;
use crate::rdw::{Dataset, RdwSource, Row};
use serde_json::Value;
use std::collections::HashMap;

/// The column every recall dataset is joined on.
pub const REFERENCE: &str = "referentiecode_rdw";

/// The reference codes of some status rows, deduplicated and in a stable order.
pub fn references<'a>(rows: impl IntoIterator<Item = &'a Row>) -> Vec<String> {
    let mut found: Vec<String> = rows
        .into_iter()
        .filter_map(|row| row.get(REFERENCE))
        .filter_map(Value::as_str)
        .map(str::to_string)
        .filter(|r| !r.is_empty())
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// Whether a status row describes a recall that is still open.
///
/// `code_status` holds `O` for open and `P` for a repair the manufacturer has
/// reported. A row with neither is `None`: RDW said nothing, and reporting that
/// as repaired would be inventing the reassuring answer.
pub fn is_open(row: &Row) -> Option<bool> {
    Some(row.get("code_status").and_then(Value::as_str)? == OPEN)
}

/// The `code_status` of a recall that has not been repaired yet.
const OPEN: &str = "O";

/// Rows from the reference-keyed recall datasets, indexed for lookup.
#[derive(Debug, Default)]
pub struct Joined {
    by_dataset: HashMap<&'static str, HashMap<String, Vec<Row>>>,
}

impl Joined {
    /// Every row of `dataset` carrying this reference.
    ///
    /// An empty slice means RDW published no such row, which is a real state:
    /// a status row can name a reference the detail dataset has nothing for.
    pub fn rows(&self, dataset: &Dataset, reference: &str) -> &[Row] {
        self.by_dataset
            .get(dataset.id)
            .and_then(|by_reference| by_reference.get(reference))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The first row of `dataset` for this reference, for the datasets that hold
    /// at most one.
    pub fn row(&self, dataset: &Dataset, reference: &str) -> Option<&Row> {
        self.rows(dataset, reference).first()
    }
}

/// Fetch and index the given datasets for the given references.
///
/// The datasets are fetched concurrently, and the earliest failure in the order
/// they were asked for is the one reported, so the same inputs always produce
/// the same error regardless of how the threads were scheduled.
pub fn resolve<S>(
    source: &S,
    datasets: &[Dataset],
    references: &[String],
    concurrency: usize,
) -> Result<Joined, KentekenError>
where
    S: RdwSource + Sync,
{
    if references.is_empty() || datasets.is_empty() {
        return Ok(Joined::default());
    }

    let fetched = crate::fetch_concurrently(datasets, concurrency, |dataset| {
        source.rows_for_values(dataset, REFERENCE, references)
    })?;

    let mut joined = Joined::default();
    for (dataset, rows) in datasets.iter().zip(fetched) {
        let index = joined.by_dataset.entry(dataset.id).or_default();
        for row in rows {
            let Some(reference) = row.get(REFERENCE).and_then(Value::as_str) else {
                continue;
            };
            index.entry(reference.to_string()).or_default().push(row);
        }
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(v: Value) -> Row {
        v.as_object().expect("row is an object").clone()
    }

    #[test]
    fn references_are_deduplicated_and_ordered() {
        let rows = vec![
            row(json!({"referentiecode_rdw": "MGP230291"})),
            row(json!({"referentiecode_rdw": "MGP230085"})),
            row(json!({"referentiecode_rdw": "MGP230291"})),
        ];
        assert_eq!(references(&rows), vec!["MGP230085", "MGP230291"]);
    }

    #[test]
    fn a_row_without_a_reference_contributes_nothing() {
        // RDW omits a column it has no value for, so a status row can arrive
        // with no reference at all. An empty string is not a reference either;
        // resolving one would fetch rows belonging to no recall.
        let rows = vec![
            row(json!({"kenteken": "X99XXX"})),
            row(json!({"referentiecode_rdw": ""})),
        ];
        assert!(references(&rows).is_empty());
    }

    #[test]
    fn open_is_read_from_the_status_code_not_the_prose() {
        // The prose column says "Openstaand" or "Hersteld", but it is RDW's
        // display text; the code is the value with a defined set.
        let open = row(json!({"code_status": "O", "status": "Openstaand"}));
        let repaired = row(json!({"code_status": "P", "status": "Hersteld"}));
        assert_eq!(is_open(&open), Some(true));
        assert_eq!(is_open(&repaired), Some(false));
        // A status RDW did not fill is unknown, not repaired. Answering "no"
        // here would tell an owner their recall is done on no evidence.
        assert_eq!(is_open(&row(json!({"kenteken": "X99XXX"}))), None);
    }

    #[test]
    fn an_unresolved_reference_yields_no_rows_rather_than_a_panic() {
        let joined = Joined::default();
        assert!(
            joined
                .rows(&crate::rdw::datasets::RECALL_DETAIL, "MGP230085")
                .is_empty()
        );
        assert!(
            joined
                .row(&crate::rdw::datasets::RECALL_DETAIL, "MGP230085")
                .is_none()
        );
    }
}

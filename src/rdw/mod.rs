//! Access to the RDW open-data API.
//!
//! [`RdwSource`] is the seam: the CLI uses [`client::HttpSource`], tests use a
//! fake, so nothing in the test suite touches the network or depends on the
//! contents of a live public dataset.

pub mod client;
pub mod datasets;

pub use client::HttpSource;
pub use datasets::Dataset;

use crate::error::KentekenError;
use crate::plate::Plate;
use serde_json::{Map, Value};

/// A single RDW row, kept exactly as RDW sent it.
///
/// Rows are not deserialized into fixed structs on purpose. RDW omits a column
/// entirely when it has no value for a vehicle, and the distinction between
/// "absent" and "zero" carries real meaning here (a missing `catalogusprijs` is
/// not a free car). Keeping the raw map preserves that, and means a column RDW
/// adds later shows up in output instead of being silently dropped.
pub type Row = Map<String, Value>;

/// Somewhere rows can be fetched from.
pub trait RdwSource {
    /// Fetch every row in `dataset` whose `kenteken` equals `plate`.
    ///
    /// An empty vector means the plate genuinely has no rows in this dataset,
    /// which callers must distinguish from an error.
    fn rows_for_plate(&self, dataset: &Dataset, plate: &Plate) -> Result<Vec<Row>, KentekenError>;
}

//! Error type, the stable error `kind` set, and the exit-code contract.
//!
//! Errors are reported as a clispec structured envelope on the last line of
//! stderr: `{"error":{"kind":...,"message":...,"exit_code":...,"hint":...}}`.
//!
//! Exit codes (also declared in the schema):
//! - `1` **outcome**, not an error: some plates resolved and some did not. The
//!   results are on stdout and no envelope is written.
//! - `2` the RDW API was unreachable
//! - `3` usage error: bad arguments, an unparseable plate, an unknown dataset
//! - `4` no requested plate exists in the dataset
//! - `5` the request timed out
//! - `6` RDW rate-limited the request
//! - `7` RDW answered, but with an error
//! - `8` the result could not be written to stdout

use crate::plate::PlateError;
use thiserror::Error;

/// Exit code for the `partial` outcome: a data state, never an error, so it
/// carries no envelope.
pub const EXIT_PARTIAL: u8 = 1;

/// All failure modes of a kenteken run.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum KentekenError {
    /// Invalid command-line arguments (also used for wrapped clap errors).
    #[error("{message}")]
    Usage { message: String },

    /// A plate argument could not be normalized into a queryable plate.
    #[error("not a valid Dutch licence plate: {source}")]
    InvalidPlate {
        input: String,
        #[source]
        source: PlateError,
    },

    /// The dataset id given to `raw` is not one RDW serves.
    #[error("RDW has no dataset '{dataset}'")]
    UnknownDataset { dataset: String },

    /// Every requested plate returned zero rows.
    #[error("{}", not_found_message(.plates))]
    NotFound { plates: Vec<String> },

    /// The RDW API could not be reached.
    #[error("could not reach the RDW API: {message}")]
    Network { message: String },

    /// The request exceeded the configured timeout.
    #[error("the RDW API did not respond within {seconds}s")]
    Timeout { seconds: u64 },

    /// RDW returned HTTP 429.
    #[error("RDW rate-limited this request")]
    RateLimit,

    /// RDW answered with an error status or an unparseable body.
    #[error("RDW API error: {message}")]
    Api { message: String },

    /// The answer was fetched but could not be written out in full.
    ///
    /// A partly written result is indistinguishable from a complete one, so a
    /// failed write has to be reported rather than swallowed. A consumer that
    /// closed the pipe (`| head`) is not this error: it got what it asked for.
    #[error("could not write the result: {message}")]
    Io { message: String },
}

fn not_found_message(plates: &[String]) -> String {
    match plates {
        [one] => format!("no vehicle registered under plate {one}"),
        many => format!("no vehicle registered under any of: {}", many.join(", ")),
    }
}

impl KentekenError {
    /// Stable snake_case identifier consumers branch on (the schema `errors` set).
    pub fn kind(&self) -> &'static str {
        match self {
            KentekenError::Usage { .. } => "usage",
            KentekenError::InvalidPlate { .. } => "invalid_plate",
            KentekenError::UnknownDataset { .. } => "unknown_dataset",
            KentekenError::NotFound { .. } => "not_found",
            KentekenError::Network { .. } => "network",
            KentekenError::Timeout { .. } => "timeout",
            KentekenError::RateLimit => "rate_limit",
            KentekenError::Api { .. } => "api",
            KentekenError::Io { .. } => "io",
        }
    }

    /// Actionable remediation, when there is one.
    pub fn hint(&self) -> Option<String> {
        match self {
            KentekenError::Usage { .. } => {
                Some("see `kenteken --help` or `kenteken schema`".into())
            }
            KentekenError::InvalidPlate { .. } => {
                Some("plates are six letters and digits, e.g. X-99-XXX".into())
            }
            KentekenError::UnknownDataset { .. } => {
                Some("run `kenteken datasets` for the datasets this tool knows".into())
            }
            KentekenError::NotFound { .. } => Some(
                "RDW only holds currently and formerly registered Dutch vehicles; \
                 check the plate for typos"
                    .into(),
            ),
            KentekenError::Network { .. } => {
                Some("check network connectivity to opendata.rdw.nl".into())
            }
            KentekenError::Timeout { .. } => Some("retry, or raise --timeout".into()),
            KentekenError::RateLimit => Some(
                "wait before retrying; set RDW_APP_TOKEN to a Socrata app token for a \
                 higher limit"
                    .into(),
            ),
            KentekenError::Api { .. } => None,
            KentekenError::Io { .. } => {
                Some("check the destination: a full disk, or a file that went away".into())
            }
        }
    }

    /// Structured, kind-specific context for the envelope's `details` object.
    pub fn details(&self) -> Option<serde_json::Value> {
        match self {
            KentekenError::InvalidPlate { input, .. } => {
                Some(serde_json::json!({ "input": input }))
            }
            KentekenError::UnknownDataset { dataset } => {
                Some(serde_json::json!({ "dataset": dataset }))
            }
            KentekenError::NotFound { plates } => Some(serde_json::json!({ "plates": plates })),
            _ => None,
        }
    }

    /// Whether a consumer should expect a retry to behave differently.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            KentekenError::Network { .. }
                | KentekenError::Timeout { .. }
                | KentekenError::RateLimit
        )
    }

    /// The process exit code associated with this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            KentekenError::Network { .. } => 2,
            KentekenError::Usage { .. }
            | KentekenError::InvalidPlate { .. }
            | KentekenError::UnknownDataset { .. } => 3,
            KentekenError::NotFound { .. } => 4,
            KentekenError::Timeout { .. } => 5,
            KentekenError::RateLimit => 6,
            KentekenError::Api { .. } => 7,
            KentekenError::Io { .. } => 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every error kind and exit code, so a change to either is a deliberate
    /// edit to this table rather than a silent contract break.
    fn all_variants() -> Vec<KentekenError> {
        vec![
            KentekenError::Usage {
                message: "bad".into(),
            },
            KentekenError::InvalidPlate {
                input: "nope".into(),
                source: PlateError::Empty,
            },
            KentekenError::UnknownDataset {
                dataset: "zzzz-zzzz".into(),
            },
            KentekenError::NotFound {
                plates: vec!["X99XXX".into()],
            },
            KentekenError::Network {
                message: "dns".into(),
            },
            KentekenError::Timeout { seconds: 15 },
            KentekenError::RateLimit,
            KentekenError::Api {
                message: "boom".into(),
            },
            KentekenError::Io {
                message: "no space left on device".into(),
            },
        ]
    }

    #[test]
    fn every_kind_is_unique() {
        let mut kinds: Vec<&str> = all_variants().iter().map(|e| e.kind()).collect();
        let count = kinds.len();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), count, "two variants share a kind: {kinds:?}");
    }

    #[test]
    fn no_error_exit_code_collides_with_the_partial_outcome() {
        // clispec: outcome codes must not overlap with error exit codes, or a
        // consumer cannot tell a data state from a failure by exit code alone.
        for err in all_variants() {
            assert_ne!(
                err.exit_code(),
                EXIT_PARTIAL,
                "{} reuses the partial outcome code",
                err.kind()
            );
        }
    }

    #[test]
    fn no_error_exits_zero() {
        for err in all_variants() {
            assert_ne!(err.exit_code(), 0, "{} exits successfully", err.kind());
        }
    }

    #[test]
    fn only_transient_kinds_are_retryable() {
        for err in all_variants() {
            let expected = matches!(err.kind(), "network" | "timeout" | "rate_limit");
            assert_eq!(err.retryable(), expected, "kind {}", err.kind());
        }
    }

    #[test]
    fn not_found_reads_naturally_for_one_and_many_plates() {
        let one = KentekenError::NotFound {
            plates: vec!["X99XXX".into()],
        };
        assert_eq!(one.to_string(), "no vehicle registered under plate X99XXX");

        let many = KentekenError::NotFound {
            plates: vec!["X99XXX".into(), "AA11BB".into()],
        };
        assert_eq!(
            many.to_string(),
            "no vehicle registered under any of: X99XXX, AA11BB"
        );
    }

    #[test]
    fn invalid_plate_message_carries_the_underlying_reason() {
        let err = KentekenError::InvalidPlate {
            input: "X99XX".into(),
            source: PlateError::BadLength {
                normalized: "X99XX".into(),
                len: 5,
            },
        };
        let message = err.to_string();
        assert!(message.contains("5 characters"), "message was {message:?}");
    }

    #[test]
    fn details_carry_the_offending_input() {
        let err = KentekenError::InvalidPlate {
            input: "X99XX".into(),
            source: PlateError::Empty,
        };
        assert_eq!(err.details().unwrap()["input"], "X99XX");
    }
}

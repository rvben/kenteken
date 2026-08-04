//! The HTTP implementation of [`RdwSource`], talking to opendata.rdw.nl.
//!
//! RDW runs Socrata (SODA v2). Anonymous access works and is what this tool
//! uses by default; setting `RDW_APP_TOKEN` adds a Socrata app token, which
//! raises the shared per-IP rate limit. The token is read from the environment
//! only, never from argv, since argv is visible in `ps` and lands in shell
//! history and agent transcripts.
//!
//! No request is ever retried automatically. RDW is a free public service and a
//! CLI that quietly re-issues failed requests turns one user error into a burst.
//! Transient failures are reported with `retryable: true` in the schema so the
//! caller decides.

use super::{Dataset, RdwSource, Row};
use crate::error::KentekenError;
use crate::plate::Plate;
use std::time::Duration;

/// Base URL of the RDW Socrata endpoint.
const BASE_URL: &str = "https://opendata.rdw.nl/resource";

/// Upper bound on rows requested from RDW in one call.
///
/// Every query this tool makes is filtered to a single plate, where the largest
/// realistic result is a few dozen rows, so the cap exists to bound a
/// pathological response rather than to paginate. Socrata's own default is 1000;
/// asking explicitly is what stops that default from silently truncating.
pub const FETCH_CAP: usize = 5_000;

/// Environment variable holding an optional Socrata app token.
pub const APP_TOKEN_ENV: &str = "RDW_APP_TOKEN";

/// Fetches rows over HTTPS from the live RDW API.
pub struct HttpSource {
    client: reqwest::blocking::Client,
    base_url: String,
    app_token: Option<String>,
    timeout: Duration,
}

impl HttpSource {
    /// Build a client with the given per-request timeout.
    pub fn new(timeout: Duration) -> Result<Self, KentekenError> {
        Self::with_base_url(BASE_URL, timeout)
    }

    /// Build a client pointed at an arbitrary base URL, for tests against a
    /// local server.
    pub fn with_base_url(base_url: &str, timeout: Duration) -> Result<Self, KentekenError> {
        let token = normalize_token(std::env::var(APP_TOKEN_ENV).ok());
        Self::build(base_url, timeout, token)
    }

    /// Build a client with an explicitly supplied token, bypassing the
    /// environment. Keeps tests off process-global state.
    pub fn build(
        base_url: &str,
        timeout: Duration,
        app_token: Option<String>,
    ) -> Result<Self, KentekenError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .user_agent(concat!("kenteken/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| KentekenError::Network {
                message: e.to_string(),
            })?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            app_token,
            timeout,
        })
    }

    /// Whether an app token will be sent.
    pub fn has_app_token(&self) -> bool {
        self.app_token.is_some()
    }
}

impl RdwSource for HttpSource {
    fn rows_for_plate(&self, dataset: &Dataset, plate: &Plate) -> Result<Vec<Row>, KentekenError> {
        if !dataset.plate_keyed {
            return Err(KentekenError::Usage {
                message: format!(
                    "dataset '{}' ({}) has no kenteken column, so it cannot be queried by plate",
                    dataset.name, dataset.id
                ),
            });
        }

        // `$order` is not optional. Socrata leaves an unsorted result's order
        // undefined, and two identical requests to this endpoint were observed
        // returning the same rows in different orders, which would make
        // `--limit` an arbitrary subset and `--offset` skip or repeat rows.
        let url = format!("{}/{}.json", self.base_url, dataset.id);
        let mut request = self.client.get(&url).query(&[
            ("kenteken", plate.as_str()),
            ("$limit", &FETCH_CAP.to_string()),
            ("$order", dataset.order),
        ]);
        if let Some(token) = &self.app_token {
            request = request.header("X-App-Token", token);
        }

        let response = request.send().map_err(|e| self.transport_error(e))?;
        let status = response.status();

        if !status.is_success() {
            return Err(self.status_error(status, dataset, response.text().ok()));
        }

        // A success status with a body that is not an array of objects means the
        // endpoint is not what we think it is. Surfacing that as an api error is
        // honest; coercing it to "no rows" would report a real vehicle as
        // unregistered.
        let body = response.text().map_err(|e| self.transport_error(e))?;
        serde_json::from_str::<Vec<Row>>(&body).map_err(|e| KentekenError::Api {
            message: format!("unexpected response from dataset {}: {e}", dataset.id),
        })
    }
}

impl HttpSource {
    /// Classify a reqwest transport failure into the right error kind.
    fn transport_error(&self, e: reqwest::Error) -> KentekenError {
        if e.is_timeout() {
            KentekenError::Timeout {
                seconds: self.timeout.as_secs(),
            }
        } else {
            KentekenError::Network {
                message: e.to_string(),
            }
        }
    }

    /// Classify a non-2xx response, using the body when RDW explains itself.
    fn status_error(
        &self,
        status: reqwest::StatusCode,
        dataset: &Dataset,
        body: Option<String>,
    ) -> KentekenError {
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return KentekenError::RateLimit;
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return KentekenError::UnknownDataset {
                dataset: dataset.id.to_string(),
            };
        }
        KentekenError::Api {
            message: match body.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
                Some(body) => format!("{status}: {}", socrata_message(body)),
                None => status.to_string(),
            },
        }
    }
}

/// Treat an empty or whitespace-only token as unset.
///
/// An exported-but-blank `RDW_APP_TOKEN` would otherwise be sent as a header and
/// rejected, which surfaces as a mysterious API error rather than as the
/// misconfiguration it is.
fn normalize_token(raw: Option<String>) -> Option<String> {
    raw.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Pull the human-readable text out of a Socrata error body, falling back to the
/// raw body when it is not the shape we expect.
fn socrata_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message")?.as_str().map(str::to_string))
        .unwrap_or_else(|| truncate(body, 200))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdw::datasets;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// Serve one request, answer it with an empty row array, and hand back the
    /// request line the client actually sent.
    ///
    /// Asserting on the wire rather than on a query-building helper is the only
    /// way to know a parameter was not dropped between the two.
    fn capture_request_line(dataset: &Dataset, plate: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the client connects");
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .expect("a request line");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]")
                .expect("the response is written");
            line
        });

        let source =
            HttpSource::build(&base, Duration::from_secs(5), None).expect("the client builds");
        let rows = source
            .rows_for_plate(dataset, &Plate::parse(plate).unwrap())
            .expect("the stub answers");
        assert!(rows.is_empty(), "the stub serves no rows");
        server.join().expect("the server thread finishes")
    }

    #[test]
    fn every_query_carries_the_datasets_sort_order() {
        // Socrata leaves an unsorted result's order undefined, so without this
        // parameter --limit returns an arbitrary subset and --offset can skip or
        // repeat rows. Nothing else in the suite would notice its absence.
        let line = capture_request_line(&datasets::DEFECTS, "X99XXX");
        assert!(
            line.contains("%24order=") || line.contains("$order="),
            "no sort order on the wire: {line}"
        );
        for term in ["meld_datum_door_keuringsinstantie", "DESC"] {
            assert!(
                line.contains(term),
                "the sort order sent is not the dataset's ({term} missing): {line}"
            );
        }
    }

    #[test]
    fn every_query_is_filtered_to_the_plate_and_bounded() {
        let line = capture_request_line(&datasets::VEHICLE, "x-99-xxx");
        assert!(
            line.contains("kenteken=X99XXX"),
            "the plate is not normalized onto the wire: {line}"
        );
        assert!(
            line.contains(&FETCH_CAP.to_string()),
            "no row cap, so Socrata's own default silently truncates: {line}"
        );
    }

    #[test]
    fn socrata_message_extracts_the_message_field() {
        let body = r#"{"error": true, "message": "Unrecognized arguments [bogusfield]"}"#;
        assert_eq!(socrata_message(body), "Unrecognized arguments [bogusfield]");
    }

    #[test]
    fn socrata_message_falls_back_to_the_raw_body() {
        assert_eq!(socrata_message("<html>502</html>"), "<html>502</html>");
    }

    #[test]
    fn socrata_message_bounds_a_huge_body() {
        let body = "x".repeat(10_000);
        let message = socrata_message(&body);
        assert!(
            message.chars().count() <= 201,
            "got {} chars",
            message.chars().count()
        );
    }

    #[test]
    fn truncate_is_character_safe() {
        // Slicing by byte index here would panic on a multi-byte boundary.
        let s = "é".repeat(300);
        assert_eq!(truncate(&s, 200).chars().count(), 201);
    }

    #[test]
    fn refuses_a_dataset_with_no_kenteken_column_without_a_request() {
        // Base URL is unroutable: if this made a request the test would hang or
        // fail with a network error rather than a usage error.
        let source = HttpSource::with_base_url("http://127.0.0.1:1", Duration::from_millis(50))
            .expect("client builds");
        let plate = Plate::parse("X99XXX").unwrap();
        let err = source
            .rows_for_plate(&datasets::DEFECT_CODES, &plate)
            .unwrap_err();
        assert_eq!(err.kind(), "usage", "got {err:?}");
    }

    #[test]
    fn a_blank_or_absent_app_token_is_treated_as_unset() {
        assert_eq!(normalize_token(None), None);
        assert_eq!(normalize_token(Some(String::new())), None);
        assert_eq!(normalize_token(Some("   \n".into())), None);
    }

    #[test]
    fn a_real_app_token_is_kept_and_trimmed() {
        assert_eq!(
            normalize_token(Some("  abc123  ".into())),
            Some("abc123".into())
        );
    }

    #[test]
    fn an_explicit_token_reaches_the_client() {
        let with = HttpSource::build(
            "http://127.0.0.1:1",
            Duration::from_millis(50),
            Some("tok".into()),
        )
        .unwrap();
        assert!(with.has_app_token());

        let without =
            HttpSource::build("http://127.0.0.1:1", Duration::from_millis(50), None).unwrap();
        assert!(!without.has_app_token());
    }
}

//! kenteken: look up Dutch vehicle data by licence plate from the RDW open data
//! API.
//!
//! The whole pipeline is reachable through [`run`], which the CLI and the tests
//! both use. `run` is generic over [`RdwSource`], so tests drive it with a fake
//! and never touch the network or depend on a live public dataset.
//!
//! Two absences are kept apart everywhere, because collapsing them is the one
//! mistake this tool cannot afford:
//!
//! - **not found** - no vehicle is registered under the plate. Reported in
//!   `not_found`, and as a `not_found` error when no requested plate exists.
//! - **no rows** - the vehicle is registered, but has nothing in the dataset
//!   asked about. Reported in `no_rows`, and exits zero.
//!
//! A typo'd plate must never come back as "this car has no recorded defects".
//!
//! Every item carries a `derived` block alongside RDW's own columns: the plate
//! in readable form, whether the APK has expired, power and CO2 pulled from
//! whichever column RDW happened to fill, and RDW's placeholder strings resolved
//! to `null`. The text output is a rendering of that same block, so an agent and
//! a human are answered from one computation rather than two.

pub mod date;
pub mod error;
pub mod facts;
pub mod gebreken;
pub mod output;
pub mod plate;
pub mod rdw;
pub mod schema;

pub use error::{EXIT_PARTIAL, KentekenError};
pub use plate::{Plate, PlateError};
pub use rdw::{Dataset, HttpSource, RdwSource, Row};

use serde_json::{Map, Value, json};

/// Rendered output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    Yaml,
    Ndjson,
}

impl OutputFormat {
    /// Whether this format carries the envelope's metadata in-band.
    ///
    /// NDJSON is one object per line by design, so there is nowhere in the data
    /// stream for `total` or `not_found` to live; the caller reports them on
    /// stderr instead.
    pub fn has_envelope(self) -> bool {
        !matches!(self, OutputFormat::Ndjson)
    }

    /// Whether the rendered output states the row counts itself.
    ///
    /// JSON and YAML print the whole envelope, so `total` and `truncated` are on
    /// stdout. Text renders only the rows, so a page cut short by `--limit` would
    /// read as the complete answer unless the counts are reported on stderr.
    pub fn states_counts(self) -> bool {
        matches!(self, OutputFormat::Json | OutputFormat::Yaml)
    }
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Registration summary, enriched with fuel rows.
    Lookup { plates: Vec<Plate> },
    /// Defects found at inspection, with code descriptions resolved.
    Defects { plates: Vec<Plate> },
    /// Fuel and emissions rows.
    Fuel { plates: Vec<Plate> },
    /// Unmodified rows from any known dataset.
    Raw {
        dataset: Dataset,
        plates: Vec<Plate>,
    },
    /// The dataset registry. Needs no network.
    Datasets,
}

impl Command {
    fn plates(&self) -> &[Plate] {
        match self {
            Command::Lookup { plates }
            | Command::Defects { plates }
            | Command::Fuel { plates }
            | Command::Raw { plates, .. } => plates,
            Command::Datasets => &[],
        }
    }

    /// The dataset whose rows become the items, if any.
    fn target(&self) -> Option<Dataset> {
        match self {
            Command::Lookup { .. } => Some(rdw::datasets::VEHICLE),
            Command::Defects { .. } => Some(rdw::datasets::DEFECTS),
            Command::Fuel { .. } => Some(rdw::datasets::FUEL),
            Command::Raw { dataset, .. } => Some(*dataset),
            Command::Datasets => None,
        }
    }
}

/// One invocation, fully resolved.
#[derive(Debug, Clone)]
pub struct Request {
    pub command: Command,
    pub format: OutputFormat,
    /// Whether the destination can render ANSI escapes.
    pub style: output::Style,
    pub limit: usize,
    pub offset: usize,
    /// Field names to keep, or `None` for every field.
    pub fields: Option<Vec<String>>,
    /// Maximum requests in flight against RDW.
    pub concurrency: usize,
}

/// What `run` produced: text for stdout, plus how the process should exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub stdout: String,
    /// Plates that are not in the vehicle register at all.
    pub not_found: Vec<String>,
    /// Plates that are registered but have no rows in the queried dataset.
    pub no_rows: Vec<String>,
    /// Rows on this page, which is what `stdout` shows.
    pub shown: usize,
    /// Rows the request matched, before `--limit` and `--offset` applied.
    pub total: usize,
    /// Whether rows remain after this page.
    pub truncated: bool,
    /// `0`, or [`EXIT_PARTIAL`] when some plates resolved and some did not.
    pub exit_code: u8,
}

/// Default rows returned before `--limit` has to be raised.
pub const DEFAULT_LIMIT: usize = 100;

/// Hard ceiling on concurrent requests to RDW, regardless of `--concurrency`.
///
/// RDW is a free public service. The cap is deliberately low: the measured cost
/// of a fan-out is dominated by one round trip, not by width, so there is
/// nothing to gain from hammering it.
pub const MAX_CONCURRENCY: usize = 8;

/// Run one request end to end.
pub fn run<S>(source: &S, request: &Request) -> Result<Outcome, KentekenError>
where
    S: RdwSource + Sync,
{
    let Gathered {
        items,
        not_found,
        no_rows,
    } = match &request.command {
        Command::Datasets => Gathered {
            items: dataset_items(),
            not_found: Vec::new(),
            no_rows: Vec::new(),
        },
        _ => gather(source, request)?,
    };

    if !request.command.plates().is_empty() && items.is_empty() && no_rows.is_empty() {
        return Err(KentekenError::NotFound { plates: not_found });
    }

    let projected = project(items, request.fields.as_deref())?;
    let total = projected.len();
    let page: Vec<Value> = projected
        .into_iter()
        .skip(request.offset)
        .take(request.limit)
        .collect();
    // Truncation means rows remain *after* this page. Comparing the page length
    // to the total would call a final page truncated whenever an offset skipped
    // anything, sending a caller after a page that does not exist. The saturating
    // add keeps an absurd `--offset` from overflowing.
    let shown = page.len();
    let truncated = request.offset.saturating_add(shown) < total;

    let envelope = json!({
        "items": page,
        "total": total,
        "limit": request.limit,
        "offset": request.offset,
        "truncated": truncated,
        "not_found": not_found,
        "no_rows": no_rows,
    });

    let exit_code = if not_found.is_empty() {
        0
    } else {
        EXIT_PARTIAL
    };

    Ok(Outcome {
        stdout: output::render(&envelope, &request.command, request.format, request.style),
        not_found,
        no_rows,
        shown,
        total,
        truncated,
        exit_code,
    })
}

/// Rows for the requested plates, with each plate classified.
struct Gathered {
    items: Vec<Value>,
    /// Plates with no trace in RDW at all.
    not_found: Vec<String>,
    /// Plates that exist but have nothing in the queried dataset.
    no_rows: Vec<String>,
}

/// Fetch every row the command needs, and classify each plate.
fn gather<S>(source: &S, request: &Request) -> Result<Gathered, KentekenError>
where
    S: RdwSource + Sync,
{
    let plates = request.command.plates();
    let target = request
        .command
        .target()
        .expect("non-dataset command has a target");
    let enrich = matches!(request.command, Command::Lookup { .. });
    // Read the clock once for the whole run, so two rows of one answer cannot
    // straddle midnight and disagree about whether an APK has expired.
    let today = date::today();

    // Every command except `lookup` also reads the vehicle register, purely to
    // tell a misspelled plate from a registered vehicle with nothing to report.
    // Without it, `defects XX99XX` on a typo would answer "no defects found",
    // which reads as a clean bill of health for a car that does not exist.
    let probes_register = target != rdw::datasets::VEHICLE;

    let mut tasks: Vec<(usize, Dataset)> = Vec::new();
    for (i, _) in plates.iter().enumerate() {
        tasks.push((i, target));
        if probes_register {
            tasks.push((i, rdw::datasets::VEHICLE));
        }
        if enrich {
            tasks.push((i, rdw::datasets::FUEL));
        }
    }

    let fetched = fetch_all(source, plates, &tasks, request.concurrency)?;

    let mut items = Vec::new();
    let mut not_found = Vec::new();
    let mut no_rows = Vec::new();

    for (i, plate) in plates.iter().enumerate() {
        let target_rows = rows_of(&fetched, &tasks, i, target);
        // A row in the target dataset proves the plate exists even if the
        // register lookup came back empty, which can happen for a vehicle that
        // has been deregistered.
        let registered = !target_rows.is_empty()
            || (probes_register
                && !rows_of(&fetched, &tasks, i, rdw::datasets::VEHICLE).is_empty());

        if !registered {
            not_found.push(plate.to_string());
            continue;
        }
        if target_rows.is_empty() {
            no_rows.push(plate.to_string());
            continue;
        }

        for row in target_rows {
            items.push(decorate(row, &request.command, &fetched, &tasks, i, today));
        }
    }

    Ok(Gathered {
        items,
        not_found,
        no_rows,
    })
}

/// Add whatever the command layers on top of a raw RDW row.
///
/// Everything computed goes under `derived`, next to RDW's untouched columns,
/// which keeps the two apart: an agent reading `vervaldatum_apk` gets exactly
/// what RDW sent, and one reading `derived.apk_expired` gets this tool's answer.
/// `raw` gets neither, because it promises rows as RDW returned them.
fn decorate(
    row: &Row,
    command: &Command,
    fetched: &[Result<Vec<Row>, KentekenError>],
    tasks: &[(usize, Dataset)],
    plate_index: usize,
    today: Option<date::Date>,
) -> Value {
    let mut row = row.clone();
    match command {
        Command::Lookup { .. } => {
            let fuel = rows_of(fetched, tasks, plate_index, rdw::datasets::FUEL);
            row.insert("fuel".into(), json!(fuel));
            let item = Value::Object(row.clone());
            row.insert("derived".into(), facts::vehicle(&item, today));
        }
        Command::Defects { .. } => {
            // `null` and not a placeholder string: an unrecognised code must stay
            // visibly unresolved rather than look like a described defect.
            let description = row
                .get("gebrek_identificatie")
                .and_then(Value::as_str)
                .and_then(gebreken::describe);
            row.insert("gebrek_omschrijving".into(), json!(description));
            let item = Value::Object(row.clone());
            row.insert("derived".into(), facts::defect(&item));
        }
        Command::Fuel { .. } => {
            let item = Value::Object(row.clone());
            row.insert("derived".into(), facts::fuel(&item));
        }
        Command::Raw { .. } | Command::Datasets => {}
    }
    Value::Object(row)
}

/// Rows fetched for one (plate, dataset) pair.
fn rows_of<'a>(
    fetched: &'a [Result<Vec<Row>, KentekenError>],
    tasks: &[(usize, Dataset)],
    plate_index: usize,
    dataset: Dataset,
) -> &'a [Row] {
    tasks
        .iter()
        .position(|(i, d)| *i == plate_index && *d == dataset)
        .and_then(|slot| fetched.get(slot))
        .and_then(|r| r.as_ref().ok())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Run every task across a bounded set of threads, preserving task order.
///
/// The first failure aborts the whole run. A network error must never be
/// downgraded into "this plate is not registered".
fn fetch_all<S>(
    source: &S,
    plates: &[Plate],
    tasks: &[(usize, Dataset)],
    concurrency: usize,
) -> Result<Vec<Result<Vec<Row>, KentekenError>>, KentekenError>
where
    S: RdwSource + Sync,
{
    let mut results: Vec<Result<Vec<Row>, KentekenError>> =
        tasks.iter().map(|_| Ok(Vec::new())).collect();
    if tasks.is_empty() {
        return Ok(results);
    }

    let width = concurrency.clamp(1, MAX_CONCURRENCY).min(tasks.len());
    let chunk = tasks.len().div_ceil(width);

    std::thread::scope(|scope| {
        for (task_chunk, result_chunk) in tasks.chunks(chunk).zip(results.chunks_mut(chunk)) {
            scope.spawn(move || {
                for ((plate_index, dataset), slot) in task_chunk.iter().zip(result_chunk.iter_mut())
                {
                    *slot = source.rows_for_plate(dataset, &plates[*plate_index]);
                }
            });
        }
    });

    // Report the earliest failure in task order, so the same inputs always
    // produce the same error regardless of thread scheduling.
    if let Some(slot) = results.iter().position(Result::is_err) {
        return Err(results
            .swap_remove(slot)
            .expect_err("the slot holds an error"));
    }
    Ok(results)
}

/// The dataset registry as items, for `kenteken datasets`.
fn dataset_items() -> Vec<Value> {
    rdw::datasets::KNOWN.iter().map(|d| json!(d)).collect()
}

/// Keep only the requested fields.
///
/// A requested field that appears in no row at all is a usage error, never a
/// silently empty column: `--fields bogus` returning `{}` with exit 0 is
/// indistinguishable from a genuine empty result.
fn project(items: Vec<Value>, fields: Option<&[String]>) -> Result<Vec<Value>, KentekenError> {
    let Some(fields) = fields else {
        return Ok(items);
    };
    if items.is_empty() {
        return Ok(items);
    }

    let unknown: Vec<&str> = fields
        .iter()
        .filter(|f| !items.iter().any(|item| item.get(f.as_str()).is_some()))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        return Err(KentekenError::Usage {
            message: format!(
                "no such field{}: {}",
                if unknown.len() == 1 { "" } else { "s" },
                unknown.join(", ")
            ),
        });
    }

    Ok(items
        .into_iter()
        .map(|item| {
            let mut kept = Map::new();
            for field in fields {
                // Present as an explicit null when this particular row lacks the
                // column, so a projection has one stable shape.
                kept.insert(
                    field.clone(),
                    item.get(field.as_str()).cloned().unwrap_or(Value::Null),
                );
            }
            Value::Object(kept)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An in-memory source: no network, no live dataset.
    #[derive(Default)]
    struct FakeSource {
        rows: HashMap<(String, String), Vec<Row>>,
        fail_with: Option<KentekenError>,
    }

    impl FakeSource {
        fn with(mut self, dataset: Dataset, plate: &str, rows: Vec<Value>) -> Self {
            let rows = rows
                .into_iter()
                .map(|v| v.as_object().expect("row is an object").clone())
                .collect();
            self.rows.insert((dataset.id.into(), plate.into()), rows);
            self
        }

        fn failing(err: KentekenError) -> Self {
            Self {
                rows: HashMap::new(),
                fail_with: Some(err),
            }
        }
    }

    impl RdwSource for FakeSource {
        fn rows_for_plate(
            &self,
            dataset: &Dataset,
            plate: &Plate,
        ) -> Result<Vec<Row>, KentekenError> {
            if let Some(err) = &self.fail_with {
                return Err(match err {
                    KentekenError::Timeout { seconds } => {
                        KentekenError::Timeout { seconds: *seconds }
                    }
                    _ => KentekenError::RateLimit,
                });
            }
            Ok(self
                .rows
                .get(&(dataset.id.to_string(), plate.to_string()))
                .cloned()
                .unwrap_or_default())
        }
    }

    fn plate(s: &str) -> Plate {
        Plate::parse(s).expect("test plate parses")
    }

    fn request(command: Command) -> Request {
        Request {
            command,
            format: OutputFormat::Json,
            style: output::Style::Plain,
            limit: DEFAULT_LIMIT,
            offset: 0,
            fields: None,
            concurrency: 4,
        }
    }

    fn envelope(outcome: &Outcome) -> Value {
        serde_json::from_str(&outcome.stdout).expect("stdout is JSON")
    }

    fn source_with_vehicle() -> FakeSource {
        FakeSource::default().with(
            rdw::datasets::VEHICLE,
            "X99XXX",
            vec![json!({"kenteken": "X99XXX", "merk": "IVECO"})],
        )
    }

    #[test]
    fn lookup_returns_the_vehicle_row() {
        let source = source_with_vehicle();
        let outcome = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("X-99-XXX")],
            }),
        )
        .unwrap();

        let v = envelope(&outcome);
        assert_eq!(v["total"], 1);
        assert_eq!(v["items"][0]["merk"], "IVECO");
        assert_eq!(outcome.exit_code, 0);
    }

    #[test]
    fn lookup_enriches_with_fuel_rows() {
        let source = source_with_vehicle().with(
            rdw::datasets::FUEL,
            "X99XXX",
            vec![json!({"brandstof_omschrijving": "Diesel"})],
        );
        let outcome = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap();
        let v = envelope(&outcome);
        assert_eq!(v["items"][0]["fuel"][0]["brandstof_omschrijving"], "Diesel");
    }

    #[test]
    fn lookup_of_a_vehicle_without_fuel_rows_yields_an_empty_array_not_a_missing_key() {
        let source = source_with_vehicle();
        let outcome = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap();
        let v = envelope(&outcome);
        assert_eq!(v["items"][0]["fuel"], json!([]));
    }

    #[test]
    fn an_unregistered_plate_is_a_not_found_error_not_an_empty_result() {
        let source = FakeSource::default();
        let err = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("XX99XX")],
            }),
        )
        .unwrap_err();
        assert_eq!(err.kind(), "not_found");
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn a_registered_plate_with_no_defects_is_not_reported_as_not_found() {
        // The whole point of probing the register: this car exists and is clean.
        // Reporting it as "not found" would be as wrong as reporting a typo'd
        // plate as clean.
        let source = source_with_vehicle();
        let outcome = run(
            &source,
            &request(Command::Defects {
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap();

        let v = envelope(&outcome);
        assert_eq!(v["items"], json!([]));
        assert_eq!(v["no_rows"], json!(["X99XXX"]));
        assert_eq!(v["not_found"], json!([]));
        assert_eq!(outcome.exit_code, 0);
    }

    #[test]
    fn a_typo_plate_queried_for_defects_is_not_found_rather_than_clean() {
        let source = source_with_vehicle();
        let err = run(
            &source,
            &request(Command::Defects {
                plates: vec![plate("XX99XX")],
            }),
        )
        .unwrap_err();
        assert_eq!(err.kind(), "not_found");
    }

    #[test]
    fn defects_resolve_code_descriptions() {
        let source = source_with_vehicle().with(
            rdw::datasets::DEFECTS,
            "X99XXX",
            vec![json!({"kenteken": "X99XXX", "gebrek_identificatie": "AC4"})],
        );
        let outcome = run(
            &source,
            &request(Command::Defects {
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap();
        let v = envelope(&outcome);
        assert_eq!(
            v["items"][0]["gebrek_omschrijving"],
            "Stuur- of fuseekogel met slijtage kleiner of gelijk 1,0 mm"
        );
    }

    #[test]
    fn an_unknown_defect_code_resolves_to_null_not_a_placeholder() {
        let source = source_with_vehicle().with(
            rdw::datasets::DEFECTS,
            "X99XXX",
            vec![json!({"gebrek_identificatie": "ZZZ9"})],
        );
        let outcome = run(
            &source,
            &request(Command::Defects {
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap();
        let v = envelope(&outcome);
        assert_eq!(v["items"][0]["gebrek_omschrijving"], Value::Null);
        // The code itself survives, so the row stays actionable.
        assert_eq!(v["items"][0]["gebrek_identificatie"], "ZZZ9");
    }

    #[test]
    fn a_deregistered_plate_with_target_rows_still_returns_them() {
        // No vehicle row, but defect rows exist: the plate demonstrably exists.
        let source = FakeSource::default().with(
            rdw::datasets::DEFECTS,
            "X99XXX",
            vec![json!({"gebrek_identificatie": "AC4"})],
        );
        let outcome = run(
            &source,
            &request(Command::Defects {
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap();
        let v = envelope(&outcome);
        assert_eq!(v["total"], 1);
        assert_eq!(v["not_found"], json!([]));
    }

    #[test]
    fn a_partly_resolved_batch_exits_with_the_partial_outcome() {
        let source = source_with_vehicle();
        let outcome = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("X99XXX"), plate("XX99XX")],
            }),
        )
        .unwrap();

        let v = envelope(&outcome);
        assert_eq!(v["total"], 1);
        assert_eq!(v["not_found"], json!(["XX99XX"]));
        assert_eq!(
            outcome.exit_code, EXIT_PARTIAL,
            "a silently dropped plate must change the exit code"
        );
    }

    #[test]
    fn a_fully_resolved_batch_exits_zero() {
        let source = source_with_vehicle().with(
            rdw::datasets::VEHICLE,
            "AA11BB",
            vec![json!({"kenteken": "AA11BB"})],
        );
        let outcome = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("X99XXX"), plate("AA11BB")],
            }),
        )
        .unwrap();
        assert_eq!(outcome.exit_code, 0);
        assert_eq!(envelope(&outcome)["total"], 2);
    }

    #[test]
    fn a_transport_failure_is_never_downgraded_to_not_found() {
        let source = FakeSource::failing(KentekenError::Timeout { seconds: 15 });
        let err = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap_err();
        assert_eq!(err.kind(), "timeout", "got {err:?}");
    }

    #[test]
    fn results_keep_plate_order_regardless_of_concurrency() {
        let mut source = FakeSource::default();
        let plates: Vec<Plate> = (10..26)
            .map(|n| {
                let p = format!("AA{n:02}BB");
                source = std::mem::take(&mut source).with(
                    rdw::datasets::VEHICLE,
                    &p,
                    vec![json!({"kenteken": p})],
                );
                plate(&p)
            })
            .collect();

        for concurrency in [1, 3, 8, 64] {
            let mut req = request(Command::Lookup {
                plates: plates.clone(),
            });
            req.concurrency = concurrency;
            let v = envelope(&run(&source, &req).unwrap());
            let got: Vec<&str> = v["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["kenteken"].as_str().unwrap())
                .collect();
            let want: Vec<String> = plates.iter().map(|p| p.to_string()).collect();
            assert_eq!(got, want, "order broke at concurrency {concurrency}");
        }
    }

    #[test]
    fn limit_and_offset_page_the_items_and_report_truncation() {
        let source = source_with_vehicle().with(
            rdw::datasets::DEFECTS,
            "X99XXX",
            (0..5)
                .map(|i| json!({"gebrek_identificatie": format!("C{i}")}))
                .collect(),
        );
        let mut req = request(Command::Defects {
            plates: vec![plate("X99XXX")],
        });
        req.limit = 2;
        req.offset = 1;

        let v = envelope(&run(&source, &req).unwrap());
        assert_eq!(v["total"], 5, "total counts all rows, not the page");
        assert_eq!(v["limit"], 2);
        assert_eq!(v["offset"], 1);
        assert_eq!(v["truncated"], true);
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        assert_eq!(v["items"][0]["gebrek_identificatie"], "C1");
    }

    #[test]
    fn the_last_page_is_not_marked_truncated_just_because_an_offset_skipped_rows() {
        // Rows 5 of 5: nothing follows this page, so a caller told `truncated`
        // would fetch an empty page and could read that as data having vanished.
        let source = source_with_vehicle().with(
            rdw::datasets::DEFECTS,
            "X99XXX",
            (0..5)
                .map(|i| json!({"gebrek_identificatie": format!("C{i}")}))
                .collect(),
        );
        let mut req = request(Command::Defects {
            plates: vec![plate("X99XXX")],
        });
        req.limit = 100;
        req.offset = 4;

        let outcome = run(&source, &req).unwrap();
        let v = envelope(&outcome);
        assert_eq!(v["total"], 5);
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            v["truncated"], false,
            "the final page carries every remaining row"
        );
        assert!(!outcome.truncated);
    }

    #[test]
    fn an_offset_past_the_end_is_an_empty_page_not_a_truncated_one() {
        let source = source_with_vehicle().with(
            rdw::datasets::DEFECTS,
            "X99XXX",
            (0..5)
                .map(|i| json!({"gebrek_identificatie": format!("C{i}")}))
                .collect(),
        );
        let mut req = request(Command::Defects {
            plates: vec![plate("X99XXX")],
        });
        req.offset = 9;

        let v = envelope(&run(&source, &req).unwrap());
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert_eq!(v["total"], 5, "the total still reports what exists");
        assert_eq!(v["truncated"], false, "there is nothing after this page");
    }

    #[test]
    fn a_page_with_rows_after_it_is_marked_truncated_at_every_offset() {
        let source = source_with_vehicle().with(
            rdw::datasets::DEFECTS,
            "X99XXX",
            (0..5)
                .map(|i| json!({"gebrek_identificatie": format!("C{i}")}))
                .collect(),
        );
        // The negative control for the two tests above: while rows remain, the
        // flag must stay true, or "not truncated" would mean nothing.
        for offset in 0..4 {
            let mut req = request(Command::Defects {
                plates: vec![plate("X99XXX")],
            });
            req.limit = 1;
            req.offset = offset;
            let outcome = run(&source, &req).unwrap();
            assert!(outcome.truncated, "offset {offset} still has rows after it");
            assert_eq!(outcome.shown, 1);
            assert_eq!(outcome.total, 5);
        }
    }

    #[test]
    fn an_unbounded_page_is_not_marked_truncated() {
        let source = source_with_vehicle();
        let v = envelope(
            &run(
                &source,
                &request(Command::Lookup {
                    plates: vec![plate("X99XXX")],
                }),
            )
            .unwrap(),
        );
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn fields_selects_columns() {
        let source = FakeSource::default().with(
            rdw::datasets::VEHICLE,
            "X99XXX",
            vec![json!({"kenteken": "X99XXX", "merk": "IVECO", "bruto_bpm": 21860})],
        );
        let mut req = request(Command::Lookup {
            plates: vec![plate("X99XXX")],
        });
        req.fields = Some(vec!["kenteken".into(), "merk".into()]);

        let v = envelope(&run(&source, &req).unwrap());
        let item = v["items"][0].as_object().unwrap();
        assert_eq!(item.len(), 2);
        assert_eq!(item["merk"], "IVECO");
        assert!(!item.contains_key("bruto_bpm"));
    }

    #[test]
    fn an_unknown_field_is_an_error_not_an_empty_object() {
        // `--fields bogus` must not return `{}` with exit 0; that is
        // indistinguishable from a genuine empty result.
        let source = source_with_vehicle();
        let mut req = request(Command::Lookup {
            plates: vec![plate("X99XXX")],
        });
        req.fields = Some(vec!["kenteken".into(), "bogus".into()]);

        let err = run(&source, &req).unwrap_err();
        assert_eq!(err.kind(), "usage");
        assert!(err.to_string().contains("bogus"), "{err}");
    }

    #[test]
    fn a_field_missing_from_only_some_rows_projects_as_null() {
        let source = source_with_vehicle().with(
            rdw::datasets::DEFECTS,
            "X99XXX",
            vec![
                json!({"gebrek_identificatie": "AC4", "aantal_gebreken_geconstateerd": "2"}),
                json!({"gebrek_identificatie": "AC5"}),
            ],
        );
        let mut req = request(Command::Defects {
            plates: vec![plate("X99XXX")],
        });
        req.fields = Some(vec!["aantal_gebreken_geconstateerd".into()]);

        let v = envelope(&run(&source, &req).unwrap());
        assert_eq!(v["items"][0]["aantal_gebreken_geconstateerd"], "2");
        assert_eq!(v["items"][1]["aantal_gebreken_geconstateerd"], Value::Null);
    }

    #[test]
    fn datasets_needs_no_source_and_lists_the_registry() {
        let source = FakeSource::failing(KentekenError::RateLimit);
        let outcome = run(&source, &request(Command::Datasets)).unwrap();
        let v = envelope(&outcome);
        assert_eq!(
            v["total"].as_u64().unwrap() as usize,
            rdw::datasets::KNOWN.len()
        );
        assert_eq!(outcome.exit_code, 0);
    }

    #[test]
    fn the_envelope_always_carries_the_same_keys() {
        // A consumer must never have to test for a key's presence.
        let source = source_with_vehicle();
        let v = envelope(
            &run(
                &source,
                &request(Command::Lookup {
                    plates: vec![plate("X99XXX")],
                }),
            )
            .unwrap(),
        );
        for key in [
            "items",
            "total",
            "limit",
            "offset",
            "truncated",
            "not_found",
            "no_rows",
        ] {
            assert!(v.get(key).is_some(), "envelope is missing {key}");
        }
    }

    #[test]
    fn lookup_carries_a_derived_block_beside_rdws_own_columns() {
        // An agent must be able to reach the same answer the summary shows,
        // without reimplementing the placeholder filtering or the date maths.
        let source = FakeSource::default().with(
            rdw::datasets::VEHICLE,
            "XXX99X",
            vec![json!({
                "kenteken": "XXX99X",
                "merk": "TESLA",
                "eerste_kleur": "ZWART",
                "tweede_kleur": "Niet geregistreerd",
                "vervaldatum_apk": "20261231",
                "wam_verzekerd": "Ja",
            })],
        );
        let outcome = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("XXX99X")],
            }),
        )
        .unwrap();
        let item = &envelope(&outcome)["items"][0];

        assert_eq!(item["derived"]["plate"], "XXX-99-X");
        assert_eq!(item["derived"]["apk_expiry"], "2026-12-31");
        assert_eq!(item["derived"]["colour"], "ZWART");
        assert_eq!(
            item["derived"]["second_colour"],
            Value::Null,
            "an RDW placeholder reached the derived block"
        );
        assert_eq!(item["derived"]["insured"], true);
        // RDW's own columns are untouched next to it, placeholders and all, so a
        // consumer that wants the register verbatim still has it.
        assert_eq!(item["tweede_kleur"], "Niet geregistreerd");
        assert_eq!(item["vervaldatum_apk"], "20261231");
    }

    #[test]
    fn an_indicator_rdw_did_not_report_is_null_in_the_derived_block() {
        let source = source_with_vehicle();
        let outcome = run(
            &source,
            &request(Command::Lookup {
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap();
        let derived = envelope(&outcome)["items"][0]["derived"].clone();
        for key in ["insured", "open_recall", "apk_expired", "odometer"] {
            assert_eq!(derived[key], Value::Null, "{key} was invented");
            assert!(derived.get(key).is_some(), "{key} key vanished");
        }
    }

    #[test]
    fn defects_and_fuel_carry_derived_blocks_too() {
        let source = source_with_vehicle()
            .with(
                rdw::datasets::DEFECTS,
                "X99XXX",
                vec![json!({
                    "kenteken": "X99XXX",
                    "gebrek_identificatie": "AC4",
                    "meld_datum_door_keuringsinstantie": "20251010",
                })],
            )
            .with(
                rdw::datasets::FUEL,
                "X99XXX",
                vec![json!({
                    "kenteken": "X99XXX",
                    "brandstof_omschrijving": "Diesel",
                    "nettomaximumvermogen": "103.00",
                })],
            );

        let defects = envelope(
            &run(
                &source,
                &request(Command::Defects {
                    plates: vec![plate("X99XXX")],
                }),
            )
            .unwrap(),
        );
        assert_eq!(
            defects["items"][0]["derived"]["inspection_date"],
            "2025-10-10"
        );
        assert_eq!(defects["items"][0]["derived"]["code"], "AC4");

        let fuel = envelope(
            &run(
                &source,
                &request(Command::Fuel {
                    plates: vec![plate("X99XXX")],
                }),
            )
            .unwrap(),
        );
        assert_eq!(fuel["items"][0]["derived"]["power_kw"], 103.0);
        assert_eq!(fuel["items"][0]["derived"]["fuel"], "Diesel");
    }

    #[test]
    fn raw_returns_rows_untouched() {
        let axles = rdw::datasets::resolve("assen").unwrap();
        let source = source_with_vehicle().with(
            axles,
            "X99XXX",
            vec![json!({"as_nummer": "1", "spoorbreedte": "174"})],
        );
        let outcome = run(
            &source,
            &request(Command::Raw {
                dataset: axles,
                plates: vec![plate("X99XXX")],
            }),
        )
        .unwrap();
        let v = envelope(&outcome);
        assert_eq!(
            v["items"][0],
            json!({"as_nummer": "1", "spoorbreedte": "174"})
        );
    }
}

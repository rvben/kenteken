//! The clispec v0.2 contract emitted by `kenteken schema`.
//!
//! Conforms to <https://clispec.dev/schema/v0.2.json> (validated by a test
//! against the vendored copy in `schemas/clispec-v0.2.json`).
//!
//! The contract is written by hand rather than derived from the clap tree,
//! because it declares things clap does not know: which exit code means what,
//! which errors are worth retrying, and the shape of the rows RDW returns.
//!
//! Each item carries RDW's own columns plus a `derived` block, and the contract
//! declares both. The raw columns are exactly what RDW sent, placeholders
//! included; `derived` is this tool's reading of them, with the placeholders
//! resolved to `null` and dates, powers and verdicts normalized. Tests assert
//! that every key the code emits is declared here, so the two cannot drift.

use crate::error::EXIT_PARTIAL;
use serde_json::{Value, json};

/// The version of The CLI Spec this document conforms to.
pub const CLISPEC_VERSION: &str = "0.2";

/// Build the clispec contract as a JSON value.
pub fn contract() -> Value {
    json!({
        "clispec": CLISPEC_VERSION,
        "name": "kenteken",
        "version": env!("CARGO_PKG_VERSION"),
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "global_args": global_args(),
        "commands": commands(),
        "errors": errors(),
        "outcomes": [
            {
                "code": EXIT_PARTIAL,
                "name": "partial",
                "description": "Some plates resolved and some are not registered. The rows that did resolve are on stdout, and `not_found` names the rest. No error envelope is written."
            }
        ]
    })
}

fn global_args() -> Value {
    json!([
        {
            "name": "--output",
            "type": "string",
            "enum": ["auto", "json", "text", "yaml", "ndjson"],
            "default": "auto",
            "description": "Output format. auto = text on a TTY, JSON when piped."
        },
        {
            "name": "--quiet",
            "type": "boolean",
            "default": false,
            "description": "Suppress warnings on stderr. Errors, and the ndjson metadata line carrying `total`/`truncated`/`not_found`/`no_rows`, are still written."
        },
        {
            "name": "--fields",
            "type": "string[]",
            "description": "Keep only these fields in each item. A name present in no row is a usage error, never a silently empty column."
        },
        {
            "name": "--limit",
            "type": "integer",
            "default": 100,
            "description": "Maximum items returned. `total` still counts every row, and `truncated` says whether rows remain after this page."
        },
        {
            "name": "--offset",
            "type": "integer",
            "default": 0,
            "description": "Items to skip before the page starts. Page by raising it until `truncated` is false."
        },
        {
            "name": "--concurrency",
            "type": "integer",
            "default": 4,
            "description": "Requests in flight against RDW, capped at 8."
        },
        {
            "name": "--timeout",
            "type": "integer",
            "default": 15,
            "description": "Per-request timeout in seconds."
        }
    ])
}

fn commands() -> Value {
    json!([
        {
            "name": "lookup",
            "description": "Registration summary for one or more plates, enriched with the vehicle's fuel rows.",
            "mutating": false,
            "stability": "stable",
            "args": [plates_arg()],
            "output_fields": vehicle_fields()
        },
        {
            "name": "defects",
            "description": "Defects recorded at inspection, with each defect code resolved to its description.",
            "mutating": false,
            "stability": "stable",
            "args": [plates_arg()],
            "output_fields": defect_fields()
        },
        {
            "name": "fuel",
            "description": "Fuel and emissions rows. A hybrid or bifuel vehicle has one row per fuel.",
            "mutating": false,
            "stability": "stable",
            "args": [plates_arg()],
            "output_fields": fuel_fields()
        },
        {
            "name": "raw",
            "description": "Rows from any known RDW dataset, exactly as RDW returned them.",
            "mutating": false,
            "stability": "stable",
            "args": [
                {
                    "name": "dataset",
                    "type": "string",
                    "required": true,
                    "description": "Dataset short name or Socrata id, as listed by `kenteken datasets`."
                },
                plates_arg()
            ]
        },
        {
            "name": "datasets",
            "description": "List the RDW datasets this build knows. Makes no network request.",
            "mutating": false,
            "stability": "stable",
            "output_fields": [
                {"name": "id", "type": "string", "description": "Socrata four-by-four resource id."},
                {"name": "name", "type": "string", "description": "Short name accepted by `raw`."},
                {"name": "description", "type": "string", "description": "What the dataset holds."},
                {"name": "plate_keyed", "type": "boolean", "description": "Whether the dataset can be queried by kenteken."},
                {"name": "order", "type": "string", "description": "SoQL $order sent with every query to this dataset. Socrata leaves an unsorted result's order undefined, so this is what makes --limit and --offset return the same rows twice. Datasets of events are newest first."}
            ]
        },
        {
            "name": "schema",
            "description": "Print this clispec contract as JSON.",
            "mutating": false,
            "stability": "stable"
        },
        {
            "name": "completions",
            "description": "Generate a shell completion script.",
            "mutating": false,
            "stability": "stable",
            "args": [
                {
                    "name": "shell",
                    "type": "string",
                    "required": true,
                    "enum": ["bash", "zsh", "fish", "powershell", "elvish"],
                    "description": "Target shell."
                }
            ]
        }
    ])
}

fn plates_arg() -> Value {
    json!({
        "name": "plates",
        "type": "string[]",
        "required": true,
        "description": "One or more Dutch licence plates. Separators and case are normalized, so X-99-XXX, x99xxx and X99XXX are the same plate."
    })
}

/// What RDW writes into a column it has no value for, spelled exactly.
///
/// Carried in the description of every `derived` block, so a consumer reading
/// the untouched RDW columns can filter them the same way this tool does.
/// Reading one of these as a value is how a one-tone car acquires a second
/// colour.
const SENTINELS: &str = "RDW writes a placeholder rather than leaving a column empty: N.v.t., Niet geregistreerd, or Geen verstrekking in Open Data. The columns above carry them verbatim; here they are null.";

/// The columns of the vehicle register worth declaring.
///
/// RDW serves 62 columns and adds to them; rows are passed through untouched, so
/// this lists the fields a consumer can rely on rather than everything present.
fn vehicle_fields() -> Value {
    json!([
        {"name": "kenteken", "type": "string", "description": "Plate, uppercase and without separators."},
        {"name": "merk", "type": "string", "description": "Make."},
        {"name": "handelsbenaming", "type": "string", "description": "Trade name (model)."},
        {"name": "voertuigsoort", "type": "string", "description": "Vehicle kind, e.g. Personenauto."},
        {"name": "europese_voertuigcategorie", "type": "string", "description": "EU category, e.g. M1."},
        {"name": "vervaldatum_apk", "type": "string", "description": "APK expiry as YYYYMMDD. Absent when the vehicle needs no inspection."},
        {"name": "datum_eerste_toelating", "type": "string", "description": "First admission to the road, YYYYMMDD."},
        {"name": "datum_tenaamstelling", "type": "string", "description": "Date of the current registration, YYYYMMDD."},
        {"name": "eerste_kleur", "type": "string", "description": "Primary colour. May be a placeholder; see derived.colour."},
        {"name": "tweede_kleur", "type": "string", "description": "Second colour. Usually a placeholder, since most vehicles have one colour; see derived.second_colour."},
        {"name": "massa_ledig_voertuig", "type": "string", "description": "Kerb mass in kg."},
        {"name": "toegestane_maximum_massa_voertuig", "type": "string", "description": "Maximum permitted mass in kg."},
        {"name": "catalogusprijs", "type": "integer", "description": "Catalogue price in euro. Absent for vehicles that never had one."},
        {"name": "tellerstandoordeel", "type": "string", "description": "RDW's verdict on the odometer history: Logisch, Onlogisch, Geen oordeel, or a placeholder."},
        {"name": "wam_verzekerd", "type": "string", "description": "Whether third-party insurance is on record (Ja/Nee)."},
        {"name": "openstaande_terugroepactie_indicator", "type": "string", "description": "Whether an unresolved recall applies (Ja/Nee)."},
        {"name": "export_indicator", "type": "string", "description": "Whether the vehicle has been exported (Ja/Nee)."},
        {"name": "tenaamstellen_mogelijk", "type": "string", "description": "Whether the registration can be transferred (Ja/Nee)."},
        {"name": "fuel", "type": "array", "description": "The vehicle's fuel rows, added by this tool. Empty when RDW records none."},
        {"name": "derived", "type": "object", "description": derived_note("This tool's reading of the row. Every key below is always present; a fact RDW did not supply is null and never a stand-in value.")},
        {"name": "derived.plate", "type": "string | null", "description": "Plate in its readable grouped form, e.g. X-99-XXX."},
        {"name": "derived.make", "type": "string | null"},
        {"name": "derived.model", "type": "string | null"},
        {"name": "derived.kind", "type": "string | null", "description": "Vehicle kind, e.g. Personenauto."},
        {"name": "derived.eu_category", "type": "string | null", "description": "EU category, e.g. M1."},
        {"name": "derived.body", "type": "string | null", "description": "Body style, e.g. hatchback."},
        {"name": "derived.colour", "type": "string | null"},
        {"name": "derived.second_colour", "type": "string | null", "description": "Null for a single-tone vehicle, which is most of them."},
        {"name": "derived.apk_expiry", "type": "string | null", "description": "APK expiry as ISO 8601. Null when the vehicle needs no inspection."},
        {"name": "derived.apk_expired", "type": "boolean | null", "description": "Null, never false, when there is no expiry date or no usable clock: a vehicle that needs no inspection has not passed one. Measured against the Dutch calendar day."},
        {"name": "derived.apk_days_remaining", "type": "integer | null", "description": "Days until expiry, negative once past. Null when apk_expired is null."},
        {"name": "derived.first_admission", "type": "string | null", "description": "First admission to the road, ISO 8601."},
        {"name": "derived.age_days", "type": "integer | null", "description": "Days since first admission."},
        {"name": "derived.registered_since", "type": "string | null", "description": "Date of the current registration, ISO 8601."},
        {"name": "derived.fuels", "type": "string[]", "description": "Every fuel the vehicle runs on, in RDW's sequence. Empty when RDW records none."},
        {"name": "derived.power_kw", "type": "number | null", "description": "Net maximum power of the primary fuel, in kW. Read from the electric power column when that is the one RDW filled in."},
        {"name": "derived.co2_g_per_km", "type": "number | null", "description": "Combined CO2 in g/km."},
        {"name": "derived.co2_basis", "type": "string | null", "enum_note": "wltp | nedc", "description": "Which test cycle produced co2_g_per_km. The two are not comparable, so the figure is never reported without it."},
        {"name": "derived.electric_range_km", "type": "number | null"},
        {"name": "derived.mass_empty_kg", "type": "integer | null"},
        {"name": "derived.mass_max_kg", "type": "integer | null"},
        {"name": "derived.catalogue_price_eur", "type": "integer | null"},
        {"name": "derived.odometer", "type": "string | null", "description": "consistent, inconsistent, or no_judgement. Null when RDW recorded no verdict, which is not the same as no_judgement: RDW looked and declined to judge."},
        {"name": "derived.insured", "type": "boolean | null", "description": "Third-party (WAM) insurance on record."},
        {"name": "derived.open_recall", "type": "boolean | null"},
        {"name": "derived.exported", "type": "boolean | null"},
        {"name": "derived.taxi", "type": "boolean | null"},
        {"name": "derived.transferable", "type": "boolean | null", "description": "Whether the registration can be transferred."}
    ])
}

fn defect_fields() -> Value {
    json!([
        {"name": "kenteken", "type": "string"},
        {"name": "gebrek_identificatie", "type": "string", "description": "RDW defect code, e.g. AC4."},
        {"name": "gebrek_omschrijving", "type": "string | null", "description": "Description resolved from the table embedded in this binary. Null when this build does not know the code, never a placeholder."},
        {"name": "meld_datum_door_keuringsinstantie", "type": "string", "description": "Date the inspection body reported the defect, YYYYMMDD."},
        {"name": "aantal_gebreken_geconstateerd", "type": "string", "description": "How many instances of this defect were found."},
        {"name": "derived", "type": "object", "description": derived_note("This tool's reading of the row. Rows arrive newest inspection first, so a page cut short by --limit shows the most recent.")},
        {"name": "derived.plate", "type": "string | null", "description": "Plate in its readable grouped form."},
        {"name": "derived.inspection_date", "type": "string | null", "description": "Inspection date as ISO 8601."},
        {"name": "derived.code", "type": "string | null"},
        {"name": "derived.description", "type": "string | null"},
        {"name": "derived.count", "type": "integer | null", "description": "How many instances of this defect were found."}
    ])
}

fn fuel_fields() -> Value {
    json!([
        {"name": "kenteken", "type": "string"},
        {"name": "brandstof_omschrijving", "type": "string", "description": "Fuel type, e.g. Benzine, Diesel, Elektriciteit."},
        {"name": "brandstof_volgnummer", "type": "string", "description": "Fuel sequence number, 1 for the primary fuel."},
        {"name": "nettomaximumvermogen", "type": "string", "description": "Net maximum power in kW. Absent on a fully electric vehicle, which uses netto_max_vermogen_elektrisch instead."},
        {"name": "emissiecode_omschrijving", "type": "string", "description": "Euro emission class."},
        {"name": "emissie_co2_gecombineerd_wltp", "type": "string", "description": "Combined WLTP CO2 in g/km. Absent for vehicles predating WLTP."},
        {"name": "co2_uitstoot_gecombineerd", "type": "string", "description": "Combined NEDC CO2 in g/km."},
        {"name": "derived", "type": "object", "description": derived_note("This tool's reading of the row, with the power and CO2 columns already reconciled.")},
        {"name": "derived.plate", "type": "string | null", "description": "Plate in its readable grouped form."},
        {"name": "derived.fuel", "type": "string | null"},
        {"name": "derived.power_kw", "type": "number | null", "description": "Net maximum power in kW, from whichever column RDW filled in."},
        {"name": "derived.co2_g_per_km", "type": "number | null"},
        {"name": "derived.co2_basis", "type": "string | null", "description": "wltp or nedc: which test cycle produced co2_g_per_km."},
        {"name": "derived.electric_range_km", "type": "number | null"},
        {"name": "derived.euro_class", "type": "string | null"},
        {"name": "derived.consumption_l_per_100km", "type": "number | null"}
    ])
}

/// Describe a `derived` block, always ending with RDW's placeholder spellings.
fn derived_note(lead: &str) -> String {
    format!("{lead} {SENTINELS}")
}

/// The stable error set, mirroring [`crate::KentekenError`].
///
/// A test asserts this list and the enum agree, so a new variant cannot ship
/// undeclared.
fn errors() -> Value {
    json!([
        {"kind": "usage", "exit_code": 3, "retryable": false, "description": "Invalid command-line arguments, or a --fields name that appears in no row."},
        {"kind": "invalid_plate", "exit_code": 3, "retryable": false, "description": "An argument could not be normalized into a Dutch licence plate."},
        {"kind": "unknown_dataset", "exit_code": 3, "retryable": false, "description": "The dataset given to `raw` is not one this build knows."},
        {"kind": "not_found", "exit_code": 4, "retryable": false, "description": "No requested plate is registered with RDW. Distinct from a registered vehicle having no rows in the queried dataset, which exits 0 and reports the plate in `no_rows`."},
        {"kind": "network", "exit_code": 2, "retryable": true, "description": "The RDW API could not be reached."},
        {"kind": "timeout", "exit_code": 5, "retryable": true, "description": "The RDW API did not respond within the timeout."},
        {"kind": "rate_limit", "exit_code": 6, "retryable": true, "description": "RDW rate-limited the request. Set RDW_APP_TOKEN to a Socrata app token for a higher limit."},
        {"kind": "api", "exit_code": 7, "retryable": false, "description": "RDW answered with an error status, or with a body this tool could not parse."},
        {"kind": "io", "exit_code": 8, "retryable": false, "description": "The result was fetched but could not be written to stdout. A consumer that closes the pipe early, as `| head` does, is not this error and exits 0."}
    ])
}

/// The contract as a pretty-printed JSON string.
pub fn contract_json() -> String {
    serde_json::to_string_pretty(&contract()).expect("contract serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KentekenError;
    use crate::plate::PlateError;

    /// One value of every error variant, so the declared set can be checked
    /// against the code rather than against itself.
    fn all_errors() -> Vec<KentekenError> {
        vec![
            KentekenError::Usage {
                message: String::new(),
            },
            KentekenError::InvalidPlate {
                input: String::new(),
                source: PlateError::Empty,
            },
            KentekenError::UnknownDataset {
                dataset: String::new(),
            },
            KentekenError::NotFound { plates: Vec::new() },
            KentekenError::Network {
                message: String::new(),
            },
            KentekenError::Timeout { seconds: 1 },
            KentekenError::RateLimit,
            KentekenError::Api {
                message: String::new(),
            },
            KentekenError::Io {
                message: String::new(),
            },
        ]
    }

    fn declared_errors() -> Vec<Value> {
        contract()["errors"].as_array().unwrap().clone()
    }

    #[test]
    fn the_contract_is_pretty_printed_json() {
        let parsed: Value = serde_json::from_str(&contract_json()).unwrap();
        assert_eq!(parsed, contract());
        assert!(contract_json().contains('\n'), "should be pretty-printed");
    }

    #[test]
    fn every_error_variant_is_declared_with_its_real_exit_code_and_retryability() {
        // The schema is the contract consumers write handlers against. If it
        // drifts from the code, the handler is wrong in a way nothing else
        // catches.
        let declared = declared_errors();
        for err in all_errors() {
            let entry = declared
                .iter()
                .find(|e| e["kind"] == err.kind())
                .unwrap_or_else(|| panic!("kind {} is not declared in the schema", err.kind()));
            assert_eq!(
                entry["exit_code"],
                json!(err.exit_code()),
                "exit code mismatch for {}",
                err.kind()
            );
            assert_eq!(
                entry["retryable"],
                json!(err.retryable()),
                "retryable mismatch for {}",
                err.kind()
            );
        }
    }

    #[test]
    fn no_error_kind_is_declared_that_the_code_cannot_produce() {
        let kinds: Vec<&str> = all_errors().iter().map(|e| e.kind()).collect();
        for entry in declared_errors() {
            let kind = entry["kind"].as_str().unwrap().to_string();
            assert!(
                kinds.contains(&kind.as_str()),
                "schema declares {kind}, which no variant produces"
            );
        }
    }

    #[test]
    fn outcome_codes_never_overlap_error_exit_codes() {
        // clispec requires this: a consumer must be able to tell a data state
        // from a failure by exit code alone.
        let error_codes: Vec<u64> = declared_errors()
            .iter()
            .map(|e| e["exit_code"].as_u64().unwrap())
            .collect();
        for outcome in contract()["outcomes"].as_array().unwrap() {
            let code = outcome["code"].as_u64().unwrap();
            assert!(
                !error_codes.contains(&code),
                "outcome code {code} collides with an error exit code"
            );
        }
    }

    #[test]
    fn every_command_declares_whether_it_mutates() {
        // An omitted `mutating` means unknown, which costs the tool
        // auto-approval. Every command here is read-only and says so.
        for command in contract()["commands"].as_array().unwrap() {
            assert_eq!(
                command["mutating"],
                json!(false),
                "command {} does not declare itself read-only",
                command["name"]
            );
        }
    }

    #[test]
    fn the_declared_datasets_command_matches_the_registry_shape() {
        let contract = contract();
        let datasets = contract["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "datasets")
            .unwrap();
        let declared: Vec<&str> = datasets["output_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        let actual = serde_json::to_value(crate::rdw::datasets::VEHICLE).unwrap();
        for key in actual.as_object().unwrap().keys() {
            assert!(
                declared.contains(&key.as_str()),
                "datasets output omits the {key} field it actually emits"
            );
        }
    }

    /// The `output_fields` of one command, by name.
    fn declared_fields(command: &str) -> Vec<String> {
        contract()["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == command)
            .unwrap_or_else(|| panic!("no {command} command"))["output_fields"]
            .as_array()
            .unwrap_or_else(|| panic!("{command} declares no output_fields"))
            .iter()
            .map(|f| f["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn every_derived_key_the_code_emits_is_declared() {
        // The derived block is what an agent reads instead of RDW's Dutch
        // columns. An undeclared key is a fact no consumer knows to ask for.
        let empty = json!({});
        let blocks = [
            ("lookup", crate::facts::vehicle(&empty, None)),
            ("defects", crate::facts::defect(&empty)),
            ("fuel", crate::facts::fuel(&empty)),
        ];
        for (command, block) in blocks {
            let declared = declared_fields(command);
            assert!(
                declared.iter().any(|f| f == "derived"),
                "{command} does not declare the derived block itself"
            );
            for key in block.as_object().unwrap().keys() {
                let dotted = format!("derived.{key}");
                assert!(
                    declared.contains(&dotted),
                    "{command} emits {dotted} without declaring it"
                );
            }
        }
    }

    #[test]
    fn no_derived_key_is_declared_that_the_code_never_emits() {
        let empty = json!({});
        let blocks = [
            ("lookup", crate::facts::vehicle(&empty, None)),
            ("defects", crate::facts::defect(&empty)),
            ("fuel", crate::facts::fuel(&empty)),
        ];
        for (command, block) in blocks {
            let emitted = block.as_object().unwrap();
            for field in declared_fields(command) {
                let Some(key) = field.strip_prefix("derived.") else {
                    continue;
                };
                assert!(
                    emitted.contains_key(key),
                    "{command} declares derived.{key}, which it never emits"
                );
            }
        }
    }

    #[test]
    fn every_command_that_returns_rdw_columns_says_what_an_empty_one_looks_like() {
        // A consumer reading the raw columns needs the placeholder spellings, or
        // it will read "Niet geregistreerd" as a colour.
        let spelled = SENTINELS.to_lowercase();
        for sentinel in crate::facts::SENTINELS {
            assert!(
                spelled.contains(sentinel),
                "the placeholder note omits {sentinel}, which the code filters"
            );
        }
        for command in ["lookup", "defects", "fuel"] {
            let note = contract()["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == command)
                .unwrap()["output_fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["name"] == "derived")
                .unwrap()["description"]
                .as_str()
                .unwrap()
                .to_string();
            assert!(
                note.ends_with(SENTINELS),
                "{command} describes its derived block without RDW's placeholders: {note}"
            );
        }
    }

    #[test]
    fn the_default_limit_declared_matches_the_code() {
        let limit = global_args()
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == "--limit")
            .unwrap()["default"]
            .as_u64()
            .unwrap() as usize;
        assert_eq!(limit, crate::DEFAULT_LIMIT);
    }

    #[test]
    fn the_declared_concurrency_default_is_within_the_hard_cap() {
        let default = global_args()
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == "--concurrency")
            .unwrap()["default"]
            .as_u64()
            .unwrap() as usize;
        assert!(default <= crate::MAX_CONCURRENCY);
    }
}

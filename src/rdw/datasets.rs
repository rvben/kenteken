//! The RDW open-data datasets this tool knows about.
//!
//! Every entry was confirmed against the live catalog at opendata.rdw.nl. The
//! `plate_keyed` flag records whether the dataset has a `kenteken` column: RDW
//! answers a query against a column it does not have with HTTP 400, so knowing
//! this up front turns a confusing API error into a clear one.
//!
//! Every entry also carries an `order`. Socrata leaves the order of an unsorted
//! result undefined, and it demonstrably varies between two identical requests,
//! so without one `--limit 4` would return an arbitrary four of eight rows and
//! `--offset` could skip or repeat rows between pages. The column each entry
//! sorts on was read from the dataset's own Socrata metadata, including its
//! type: `meld_datum_door_keuringsinstantie` is a number, so `DESC` on it is
//! genuinely newest-first rather than lexical.

use serde::Serialize;

/// One RDW dataset: its Socrata four-by-four id and what it holds.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Dataset {
    /// Socrata resource id, e.g. `m9d7-ebf2`.
    pub id: &'static str,
    /// Short name used by this tool and shown in `kenteken datasets`.
    pub name: &'static str,
    /// What the dataset contains.
    pub description: &'static str,
    /// Whether rows can be selected by `kenteken`.
    pub plate_keyed: bool,
    /// SoQL `$order` clause making the row order total and repeatable.
    ///
    /// Where rows describe events, the newest comes first: a defect list cut
    /// short by `--limit` must show the most recent inspection, not the oldest.
    /// Where they describe parts of one vehicle, the sequence number RDW
    /// assigned comes first, because axle 1 before axle 2 is the order the data
    /// is meant to be read in.
    pub order: &'static str,
}

/// The main vehicle register: one row per plate, 62 columns.
pub const VEHICLE: Dataset = Dataset {
    id: "m9d7-ebf2",
    name: "voertuigen",
    description: "Registered vehicles: make, model, APK expiry, masses, prices, indicators.",
    plate_keyed: true,
    order: "kenteken",
};

/// Fuel and emissions, one row per fuel (hybrids and bifuel have several).
pub const FUEL: Dataset = Dataset {
    id: "8ys7-d773",
    name: "brandstof",
    description: "Fuel and emissions: fuel type, power, CO2, particulates, noise levels.",
    plate_keyed: true,
    order: "brandstof_volgnummer",
};

/// Defects recorded at inspection, one row per defect found.
pub const DEFECTS: Dataset = Dataset {
    id: "a34c-vvps",
    name: "gebreken",
    description: "Defects found at APK inspections, by defect code and inspection date.",
    plate_keyed: true,
    order: "meld_datum_door_keuringsinstantie DESC, meld_tijd_door_keuringsinstantie DESC, gebrek_identificatie",
};

/// The defect code lookup table, embedded at compile time rather than queried.
pub const DEFECT_CODES: Dataset = Dataset {
    id: "hx2c-gt7k",
    name: "gebrekcodes",
    description: "Defect code descriptions. Embedded in this binary; no request needed.",
    plate_keyed: false,
    order: "gebrek_identificatie",
};

/// Which recalls apply to a plate, open and already repaired.
///
/// Sorting on `code_status` first is a deliberate choice rather than a
/// convenience: the column holds exactly two values, `O` for an open recall and
/// `P` for one the manufacturer has reported repaired, so ascending order puts
/// the open ones first and a page cut short by `--limit` cannot hide one.
pub const RECALL_STATUS: Dataset = Dataset {
    id: "t49b-isb7",
    name: "terugroepactie-status",
    description: "Recalls per vehicle: open, or reported repaired by the manufacturer.",
    plate_keyed: true,
    order: "code_status, referentiecode_rdw",
};

/// What a recall is about, keyed by RDW's reference code rather than by plate.
pub const RECALL_DETAIL: Dataset = Dataset {
    id: "j9yg-7rg9",
    name: "terugroepactie",
    description: "Recall detail: the defect, its consequences, the repair and who to contact.",
    plate_keyed: false,
    order: "referentiecode_rdw",
};

/// The hazard a recall guards against, keyed by the same reference code.
pub const RECALL_RISK: Dataset = Dataset {
    id: "9ihi-jgpf",
    name: "terugroepactie-risico",
    description: "The hazard a recall guards against, in RDW's own words.",
    plate_keyed: false,
    order: "referentiecode_rdw, code_mogelijk_gevaar",
};

/// Everything an inspection body has filed against a plate.
pub const INSPECTIONS: Dataset = Dataset {
    id: "sgfe-77wx",
    name: "meldingen",
    description: "Notifications filed by inspection bodies, including tachograph tampering.",
    plate_keyed: true,
    order: "meld_datum_door_keuringsinstantie DESC, meld_tijd_door_keuringsinstantie DESC, soort_melding_ki_omschrijving",
};

/// The odometer-judgement explanations, embedded at compile time.
pub const ODOMETER_REASONS: Dataset = Dataset {
    id: "jqs4-4kvw",
    name: "tellerstandtoelichting",
    description: "Why RDW judged an odometer as it did. Embedded in this binary; no request needed.",
    plate_keyed: false,
    order: "code_toelichting_tellerstandoordeel",
};

/// Every dataset `kenteken raw` accepts by name, and `kenteken datasets` lists.
pub const KNOWN: &[Dataset] = &[
    VEHICLE,
    FUEL,
    DEFECTS,
    DEFECT_CODES,
    RECALL_STATUS,
    RECALL_DETAIL,
    RECALL_RISK,
    INSPECTIONS,
    ODOMETER_REASONS,
    Dataset {
        id: "3huj-srit",
        name: "assen",
        description: "Axles: axle loads, track width, driven axles, spacing.",
        plate_keyed: true,
        order: "as_nummer",
    },
    Dataset {
        id: "vezc-m2t6",
        name: "carrosserie",
        description: "Body type of the vehicle.",
        plate_keyed: true,
        order: "carrosserie_volgnummer",
    },
    Dataset {
        id: "jhie-znh9",
        name: "carrosserie-specifiek",
        description: "Body detail codes and their European descriptions.",
        plate_keyed: true,
        order: "carrosserie_volgnummer, carrosserie_voertuig_nummer_code_volgnummer",
    },
    Dataset {
        id: "kmfi-hrps",
        name: "voertuigklasse",
        description: "European vehicle class per body.",
        plate_keyed: true,
        order: "carrosserie_volgnummer, carrosserie_klasse_volgnummer",
    },
    Dataset {
        id: "7ug8-2dtt",
        name: "bijzonderheden",
        description: "Special provisions and exemptions recorded against the vehicle.",
        plate_keyed: true,
        order: "bijzonderheid_volgnummer",
    },
    Dataset {
        id: "sghb-dzxx",
        name: "toegevoegde-objecten",
        description: "Retrofitted objects, such as an LPG installation.",
        plate_keyed: true,
        order: "montagedatum DESC, uitvoerings_volgnr_toegev_obj",
    },
    Dataset {
        id: "2ba7-embk",
        name: "subcategorie",
        description: "Vehicle subcategory.",
        plate_keyed: true,
        order: "subcategorie_voertuig_volgnummer",
    },
    Dataset {
        id: "3xwf-ince",
        name: "rupsbanden",
        description: "Track (crawler) details for tracked vehicles.",
        plate_keyed: true,
        order: "rupsband_set_volgnr",
    },
    Dataset {
        id: "vkij-7mwc",
        name: "keuringen",
        description: "Inspection expiry dates.",
        plate_keyed: true,
        order: "vervaldatum_keuring DESC",
    },
];

/// Resolve a dataset by short name or by Socrata id.
///
/// Both spellings are accepted so a consumer can use the friendly name from
/// `kenteken datasets` or paste an id straight out of the RDW catalog.
pub fn resolve(needle: &str) -> Option<Dataset> {
    let needle = needle.trim().to_ascii_lowercase();
    KNOWN
        .iter()
        .find(|d| d.name == needle || d.id == needle)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names_are_unique() {
        for field in ["id", "name"] {
            let mut seen: Vec<&str> = KNOWN
                .iter()
                .map(|d| if field == "id" { d.id } else { d.name })
                .collect();
            let count = seen.len();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), count, "duplicate {field} in the registry");
        }
    }

    #[test]
    fn every_id_is_a_socrata_four_by_four() {
        for d in KNOWN {
            let (left, right) = d.id.split_once('-').expect("id has a dash");
            assert_eq!(left.len(), 4, "{} left part", d.id);
            assert_eq!(right.len(), 4, "{} right part", d.id);
            assert!(
                d.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} has unexpected characters",
                d.id
            );
        }
    }

    #[test]
    fn resolves_by_name_and_by_id_case_insensitively() {
        assert_eq!(resolve("voertuigen"), Some(VEHICLE));
        assert_eq!(resolve("m9d7-ebf2"), Some(VEHICLE));
        assert_eq!(resolve("  VOERTUIGEN "), Some(VEHICLE));
        assert_eq!(resolve("M9D7-EBF2"), Some(VEHICLE));
    }

    #[test]
    fn does_not_resolve_unknown_names() {
        assert_eq!(resolve("nonsense"), None);
        assert_eq!(resolve(""), None);
    }

    #[test]
    fn the_datasets_the_commands_depend_on_are_plate_keyed() {
        for d in [VEHICLE, FUEL, DEFECTS, RECALL_STATUS, INSPECTIONS] {
            assert!(d.plate_keyed, "{} must be queryable by plate", d.name);
        }
    }

    #[test]
    fn the_embedded_code_table_is_marked_as_not_plate_keyed() {
        // Querying it by kenteken is an HTTP 400 from RDW; the flag is what lets
        // `raw` refuse before making that request. Checked through `resolve`,
        // because that is the path `raw` actually takes to reach the flag.
        let resolved = resolve("gebrekcodes").expect("the code table is resolvable by name");
        assert!(
            !resolved.plate_keyed,
            "{} would be queried by plate and get an HTTP 400",
            resolved.name
        );
    }

    #[test]
    fn every_dataset_declares_a_non_empty_order() {
        // An empty order would silently restore Socrata's undefined ordering,
        // which is what makes `--limit` return an arbitrary subset.
        for d in KNOWN {
            assert!(!d.order.trim().is_empty(), "{} has no order", d.name);
        }
    }

    #[test]
    fn every_order_sorts_on_a_column_of_its_own_dataset() {
        // A column name from the wrong dataset is an HTTP 400 on every request
        // to it, and the tests all use a fake source that would never notice.
        // The pairs below are read from each dataset's Socrata metadata.
        let columns: &[(&str, &[&str])] = &[
            ("m9d7-ebf2", &["kenteken"]),
            ("8ys7-d773", &["brandstof_volgnummer"]),
            (
                "a34c-vvps",
                &[
                    "meld_datum_door_keuringsinstantie",
                    "meld_tijd_door_keuringsinstantie",
                    "gebrek_identificatie",
                ],
            ),
            ("hx2c-gt7k", &["gebrek_identificatie"]),
            ("t49b-isb7", &["code_status", "referentiecode_rdw"]),
            ("j9yg-7rg9", &["referentiecode_rdw"]),
            ("9ihi-jgpf", &["referentiecode_rdw", "code_mogelijk_gevaar"]),
            (
                "sgfe-77wx",
                &[
                    "meld_datum_door_keuringsinstantie",
                    "meld_tijd_door_keuringsinstantie",
                    "soort_melding_ki_omschrijving",
                ],
            ),
            ("jqs4-4kvw", &["code_toelichting_tellerstandoordeel"]),
            ("3huj-srit", &["as_nummer"]),
            ("vezc-m2t6", &["carrosserie_volgnummer"]),
            (
                "jhie-znh9",
                &[
                    "carrosserie_volgnummer",
                    "carrosserie_voertuig_nummer_code_volgnummer",
                ],
            ),
            (
                "kmfi-hrps",
                &["carrosserie_volgnummer", "carrosserie_klasse_volgnummer"],
            ),
            ("7ug8-2dtt", &["bijzonderheid_volgnummer"]),
            (
                "sghb-dzxx",
                &["montagedatum", "uitvoerings_volgnr_toegev_obj"],
            ),
            ("2ba7-embk", &["subcategorie_voertuig_volgnummer"]),
            ("3xwf-ince", &["rupsband_set_volgnr"]),
            ("vkij-7mwc", &["vervaldatum_keuring"]),
        ];
        assert_eq!(columns.len(), KNOWN.len(), "a dataset has no column table");

        for d in KNOWN {
            let known = columns
                .iter()
                .find(|(id, _)| *id == d.id)
                .unwrap_or_else(|| panic!("{} has no column table", d.name))
                .1;
            for term in d.order.split(',') {
                let column = term.split_whitespace().next().unwrap_or("");
                assert!(
                    known.contains(&column),
                    "{} orders on {column:?}, which is not one of its columns",
                    d.name
                );
            }
        }
    }

    #[test]
    fn the_defect_order_puts_the_newest_inspection_first() {
        // A defect list cut short by `--limit` has to show the most recent
        // inspection. Oldest-first would answer "what was wrong in 2023" to
        // someone asking "what is wrong now".
        assert!(
            DEFECTS
                .order
                .starts_with("meld_datum_door_keuringsinstantie DESC"),
            "defects order is {:?}",
            DEFECTS.order
        );
    }

    #[test]
    fn the_recall_order_puts_open_recalls_first() {
        // `code_status` holds `O` (open) and `P` (repair reported), and nothing
        // else, so ascending order lists the open ones first. A page cut short
        // by `--limit` must never drop an open recall to show a repaired one.
        assert!(
            RECALL_STATUS.order.starts_with("code_status"),
            "recall order is {:?}",
            RECALL_STATUS.order
        );
    }

    #[test]
    fn the_reference_keyed_recall_datasets_are_not_plate_keyed() {
        // Both are reached through a recall's reference code. Querying either by
        // kenteken is an HTTP 400, and the flag is what refuses that locally.
        for d in [RECALL_DETAIL, RECALL_RISK] {
            assert!(
                !d.plate_keyed,
                "{} would be queried by plate and get an HTTP 400",
                d.name
            );
        }
    }

    #[test]
    fn named_constants_are_in_the_registry() {
        for d in [
            VEHICLE,
            FUEL,
            DEFECTS,
            DEFECT_CODES,
            RECALL_STATUS,
            RECALL_DETAIL,
            RECALL_RISK,
            INSPECTIONS,
            ODOMETER_REASONS,
        ] {
            assert!(KNOWN.contains(&d), "{} missing from KNOWN", d.name);
        }
    }
}

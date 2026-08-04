//! Reading facts out of an RDW row, once, for both audiences.
//!
//! Every computed value a human sees comes from this module, and so does every
//! value in the `derived` block an agent reads. One implementation means the two
//! surfaces cannot disagree about whether an inspection has expired.
//!
//! # RDW's placeholders
//!
//! RDW does not leave a column out when it has nothing to record. It writes a
//! sentence in it: `N.v.t.`, `Niet geregistreerd`, or `Geen verstrekking in Open
//! Data`. Those are absences wearing the clothes of a value, and they are common
//! rather than exotic: `Niet geregistreerd` is the single most frequent value of
//! `tweede_kleur` (10.6M of 17M rows), so passing it through renders a one-tone
//! car as two-tone, and `wacht_op_keuren` is `Geen verstrekking in Open Data` in
//! every row of the register. [`text`] treats all three as absent, which is what
//! RDW means by them.
//!
//! The raw columns still carry the sentinels verbatim in JSON output, because
//! `raw` promises rows exactly as RDW returned them. `derived` is the sentinel
//! free view, and `schema` lists the sentinels so a consumer of the raw columns
//! can filter them too.

use crate::date::{self, Date};
use serde_json::{Value, json};

/// The strings RDW writes into a column that has no value.
///
/// Each was confirmed against the live register by counting how often it occurs,
/// which is also how their weight became clear: `Niet geregistreerd` is the most
/// frequent value of `tweede_kleur` in the entire dataset. Compared
/// case-insensitively after trimming, since spelling varies between datasets.
///
/// Deliberately short. A word that merely sounds like an absence, such as
/// `Onbekend`, is a value RDW chose to record and is passed through.
pub const SENTINELS: &[&str] = &[
    "n.v.t.",
    "niet geregistreerd",
    "geen verstrekking in open data",
];

/// Read a column as text, or `None` when RDW recorded nothing for it.
///
/// Accepts the column whether RDW typed it as a string or a number: the register
/// sends `catalogusprijs` as a number and the sub-datasets send the same kind of
/// value as a string.
pub fn text(row: &Value, key: &str) -> Option<String> {
    let raw = match row.get(key)? {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return None,
    };
    if raw.is_empty() || is_sentinel(&raw) {
        return None;
    }
    Some(raw)
}

/// Whether a value is one of RDW's placeholders for "nothing recorded".
pub fn is_sentinel(value: &str) -> bool {
    let value = value.trim().to_lowercase();
    SENTINELS.contains(&value.as_str())
}

/// Read a column as a number, or `None` when it is absent or not numeric.
pub fn number(row: &Value, key: &str) -> Option<f64> {
    text(row, key)?.replace(',', ".").parse().ok()
}

/// Read a column as a whole number, rounding a value RDW sent with decimals.
pub fn integer(row: &Value, key: &str) -> Option<i64> {
    let n = number(row, key)?;
    n.is_finite().then(|| n.round() as i64)
}

/// Read one of RDW's `Ja`/`Nee` indicator columns.
///
/// Anything else, including a sentinel, is `None`. An indicator RDW did not
/// report must not become a confident `false`: "no recall is outstanding" and
/// "nobody said" are different answers.
pub fn flag(row: &Value, key: &str) -> Option<bool> {
    match text(row, key)?.to_lowercase().as_str() {
        "ja" => Some(true),
        "nee" => Some(false),
        _ => None,
    }
}

/// Read one of RDW's `YYYYMMDD` date columns.
pub fn date(row: &Value, key: &str) -> Option<Date> {
    Date::parse_compact(&text(row, key)?)
}

/// RDW's verdict on an odometer history, as a stable machine value.
///
/// `Niet geregistreerd` is absent rather than a verdict. `Geen oordeel` is a
/// verdict: RDW looked and declined to judge, which is not the same as never
/// having looked.
pub fn odometer_judgement(row: &Value) -> Option<&'static str> {
    match text(row, "tellerstandoordeel")?.to_lowercase().as_str() {
        "logisch" => Some("consistent"),
        "onlogisch" => Some("inconsistent"),
        "geen oordeel" => Some("no_judgement"),
        _ => None,
    }
}

/// Group thousands so a six-figure price can be read at a glance.
///
/// The separator is the comma that goes with the tool's English labels; the
/// numbers themselves are unchanged, and JSON output carries them unformatted.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Render a measurement without RDW's trailing zeros: `100.00` becomes `100`.
pub fn measure(value: f64) -> String {
    if value.fract() == 0.0 {
        return thousands(value as i64);
    }
    format!("{value}")
}

/// Title-case one of RDW's shouted enumerations: `ZWART` becomes `Zwart`.
///
/// Only applied to display. The raw column and the `derived` block both keep
/// RDW's own spelling, so a consumer can match it against RDW's documentation.
pub fn title_case(value: &str) -> String {
    value
        .split(' ')
        .map(|word| {
            if word.chars().any(|c| c.is_lowercase()) {
                return word.to_string();
            }
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_string() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The plate in its readable grouped form, e.g. `X-99-XXX`.
fn plate(row: &Value) -> Option<String> {
    let raw = text(row, "kenteken")?;
    Some(
        crate::plate::Plate::parse(&raw)
            .map(|p| p.display_grouped())
            .unwrap_or(raw),
    )
}

/// The `derived` block for a vehicle: every fact the summary is built from.
///
/// Keys are always present so a consumer never has to test for one, and a fact
/// RDW did not supply is `null`. `apk_expired` in particular is `null` and not
/// `false` when there is no expiry date to compare, because a vehicle that needs
/// no inspection has not passed one.
pub fn vehicle(item: &Value, today: Option<Date>) -> Value {
    let apk = date(item, "vervaldatum_apk");
    let days_remaining = match (today, apk) {
        (Some(today), Some(apk)) => Some(today.days_until(&apk)),
        _ => None,
    };
    let admitted = date(item, "datum_eerste_toelating");
    let fuels = fuel_rows(item);

    json!({
        "plate": plate(item),
        "make": text(item, "merk"),
        "model": text(item, "handelsbenaming"),
        "kind": text(item, "voertuigsoort"),
        "eu_category": text(item, "europese_voertuigcategorie"),
        "body": text(item, "inrichting"),
        "colour": text(item, "eerste_kleur"),
        "second_colour": text(item, "tweede_kleur"),
        "apk_expiry": apk.map(|d| d.iso()),
        "apk_expired": days_remaining.map(|d| d < 0),
        "apk_days_remaining": days_remaining,
        "first_admission": admitted.map(|d| d.iso()),
        "age_days": match (today, admitted) {
            (Some(today), Some(admitted)) => Some(admitted.days_until(&today)),
            _ => None,
        },
        "registered_since": date(item, "datum_tenaamstelling").map(|d| d.iso()),
        "fuels": fuels.iter().filter_map(|r| text(r, "brandstof_omschrijving")).collect::<Vec<_>>(),
        "power_kw": fuels.iter().filter_map(power_kw).next(),
        "co2_g_per_km": fuels.iter().filter_map(|r| co2(r).map(|(v, _)| v)).next(),
        "co2_basis": fuels.iter().filter_map(|r| co2(r).map(|(_, b)| b)).next(),
        "electric_range_km": fuels.iter().filter_map(electric_range_km).next(),
        "mass_empty_kg": integer(item, "massa_ledig_voertuig"),
        "mass_max_kg": integer(item, "toegestane_maximum_massa_voertuig"),
        "catalogue_price_eur": integer(item, "catalogusprijs"),
        "odometer": odometer_judgement(item),
        "insured": flag(item, "wam_verzekerd"),
        "open_recall": flag(item, "openstaande_terugroepactie_indicator"),
        "exported": flag(item, "export_indicator"),
        "taxi": flag(item, "taxi_indicator"),
        "transferable": flag(item, "tenaamstellen_mogelijk"),
    })
}

/// The `derived` block for one defect row.
pub fn defect(row: &Value) -> Value {
    json!({
        "plate": plate(row),
        "inspection_date": date(row, "meld_datum_door_keuringsinstantie").map(|d| d.iso()),
        "code": text(row, "gebrek_identificatie"),
        "description": text(row, "gebrek_omschrijving"),
        "count": integer(row, "aantal_gebreken_geconstateerd"),
    })
}

/// The `derived` block for one fuel row.
pub fn fuel(row: &Value) -> Value {
    let co2 = co2(row);
    json!({
        "plate": plate(row),
        "fuel": text(row, "brandstof_omschrijving"),
        "power_kw": power_kw(row),
        "co2_g_per_km": co2.map(|(v, _)| v),
        "co2_basis": co2.map(|(_, basis)| basis),
        "electric_range_km": electric_range_km(row),
        "euro_class": text(row, "emissiecode_omschrijving"),
        "consumption_l_per_100km": number(row, "brandstof_verbruik_gecombineerd_wltp")
            .or_else(|| number(row, "brandstofverbruik_gecombineerd")),
    })
}

/// The fuel rows `lookup` attaches to a vehicle.
fn fuel_rows(item: &Value) -> Vec<Value> {
    item.get("fuel")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Net maximum power in kW, from whichever column RDW filled in.
///
/// A combustion vehicle carries `nettomaximumvermogen`; a fully electric one
/// often leaves that empty and reports `netto_max_vermogen_elektrisch` instead,
/// which is why an EV used to render with no power at all.
pub fn power_kw(row: &Value) -> Option<f64> {
    number(row, "nettomaximumvermogen").or_else(|| number(row, "netto_max_vermogen_elektrisch"))
}

/// Combined CO2 in g/km, and which test cycle it came from.
///
/// WLTP where RDW has it, NEDC otherwise. The two are not comparable, so which
/// one produced the number travels with it rather than being dropped.
pub fn co2(row: &Value) -> Option<(f64, &'static str)> {
    number(row, "emissie_co2_gecombineerd_wltp")
        .map(|v| (v, "wltp"))
        .or_else(|| number(row, "co2_uitstoot_gecombineerd").map(|v| (v, "nedc")))
}

/// Electric range in km, for a battery or plug-in hybrid vehicle.
pub fn electric_range_km(row: &Value) -> Option<f64> {
    number(row, "actie_radius_enkel_elektrisch_wltp")
        .or_else(|| number(row, "actieradius"))
        .or_else(|| number(row, "actie_radius_extern_opladen_wltp"))
}

/// A date with the relative phrase a reader actually wants next to it.
pub fn dated(date: &Date, today: Option<Date>) -> String {
    match today {
        Some(today) => format!(
            "{}, {}",
            date.iso(),
            date::humanize_offset(today.days_until(date))
        ),
        None => date.iso(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One RDW row, checked to be the shape RDW actually sends.
    ///
    /// Every accessor here returns `None` for a non-object, so a fixture written
    /// as an array by mistake would make an assertion pass without testing
    /// anything.
    fn row(v: Value) -> Value {
        assert!(v.is_object(), "an RDW row is a JSON object, got {v}");
        v
    }

    #[test]
    fn rdw_placeholders_read_as_absent_rather_than_as_values() {
        // The bug this module exists to prevent: `Niet geregistreerd` is the
        // most common value of tweede_kleur, and passing it through renders a
        // one-tone car as two-tone.
        for placeholder in [
            "N.v.t.",
            "n.v.t.",
            "Niet geregistreerd",
            "NIET GEREGISTREERD",
            "Geen verstrekking in Open Data",
            "  Niet geregistreerd  ",
        ] {
            let r = row(json!({ "tweede_kleur": placeholder }));
            assert_eq!(
                text(&r, "tweede_kleur"),
                None,
                "{placeholder:?} came through as a value"
            );
        }
    }

    #[test]
    fn a_real_value_is_not_mistaken_for_a_placeholder() {
        // The negative control: without it, a filter that dropped everything
        // would pass the test above.
        for real in ["ZWART", "Zwart", "GRIJS", "hatchback", "Logisch"] {
            let r = row(json!({ "eerste_kleur": real }));
            assert_eq!(text(&r, "eerste_kleur"), Some(real.to_string()));
        }
    }

    #[test]
    fn an_absent_column_and_an_empty_one_both_read_as_absent() {
        let r = row(json!({"a": "", "b": "   ", "c": Value::Null}));
        for key in ["a", "b", "c", "missing"] {
            assert_eq!(text(&r, key), None, "key {key}");
        }
    }

    #[test]
    fn numbers_read_the_same_whether_rdw_typed_them_as_text_or_numbers() {
        assert_eq!(number(&row(json!({"n": "91144"})), "n"), Some(91144.0));
        assert_eq!(number(&row(json!({"n": 91144})), "n"), Some(91144.0));
        assert_eq!(number(&row(json!({"n": "100.00"})), "n"), Some(100.0));
    }

    #[test]
    fn an_indicator_rdw_did_not_report_is_null_rather_than_false() {
        // "no recall is outstanding" and "nobody said" are different answers.
        assert_eq!(flag(&row(json!({"i": "Ja"})), "i"), Some(true));
        assert_eq!(flag(&row(json!({"i": "Nee"})), "i"), Some(false));
        assert_eq!(flag(&row(json!({"i": "Niet geregistreerd"})), "i"), None);
        assert_eq!(flag(&row(json!({})), "i"), None);
    }

    #[test]
    fn the_odometer_verdict_keeps_no_judgement_apart_from_no_record() {
        let cases = [
            ("Logisch", Some("consistent")),
            ("Onlogisch", Some("inconsistent")),
            ("Geen oordeel", Some("no_judgement")),
            ("Niet geregistreerd", None),
        ];
        for (raw, expected) in cases {
            let r = row(json!({ "tellerstandoordeel": raw }));
            assert_eq!(odometer_judgement(&r), expected, "input {raw:?}");
        }
    }

    #[test]
    fn thousands_groups_from_the_right() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(91_144), "91,144");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-2_059), "-2,059");
    }

    #[test]
    fn a_measurement_drops_the_zeros_rdw_pads_it_with() {
        assert_eq!(measure(100.0), "100");
        assert_eq!(measure(103.5), "103.5");
        assert_eq!(measure(1500.0), "1,500");
    }

    #[test]
    fn title_case_calms_a_shouted_value_and_leaves_others_alone() {
        assert_eq!(title_case("ZWART"), "Zwart");
        assert_eq!(title_case("LICHT BLAUW"), "Licht Blauw");
        assert_eq!(title_case("hatchback"), "hatchback");
        assert_eq!(title_case("MODEL Y"), "Model Y");
    }

    #[test]
    fn an_apk_with_no_expiry_date_is_unknown_rather_than_valid() {
        // A trailer needs no inspection. Reporting `apk_expired: false` would
        // claim it holds a current certificate.
        let today = Date::new(2026, 8, 4);
        let v = vehicle(&row(json!({"kenteken": "X99XXX"})), today);
        assert_eq!(v["apk_expiry"], Value::Null);
        assert_eq!(v["apk_expired"], Value::Null);
        assert_eq!(v["apk_days_remaining"], Value::Null);
    }

    #[test]
    fn an_apk_is_expired_the_day_after_it_runs_out() {
        let expiry = json!({"kenteken": "X99XXX", "vervaldatum_apk": "20260804"});
        let cases = [
            (Date::new(2026, 8, 3), false, 1),
            (Date::new(2026, 8, 4), false, 0),
            (Date::new(2026, 8, 5), true, -1),
        ];
        for (today, expired, remaining) in cases {
            let v = vehicle(&expiry, today);
            assert_eq!(v["apk_expired"], json!(expired), "on {:?}", today.unwrap());
            assert_eq!(v["apk_days_remaining"], json!(remaining));
        }
    }

    #[test]
    fn without_a_clock_the_expiry_is_reported_but_not_judged() {
        let v = vehicle(
            &row(json!({"kenteken": "X99XXX", "vervaldatum_apk": "20260804"})),
            None,
        );
        assert_eq!(v["apk_expiry"], "2026-08-04");
        assert_eq!(v["apk_expired"], Value::Null, "no clock, no verdict");
    }

    #[test]
    fn the_derived_block_has_the_same_keys_whatever_rdw_sent() {
        // A consumer must never have to test for a key's presence.
        let full = vehicle(
            &row(json!({
                "kenteken": "X99XXX",
                "merk": "IVECO",
                "vervaldatum_apk": "20271211",
                "catalogusprijs": 91144,
                "wam_verzekerd": "Ja",
            })),
            Date::new(2026, 8, 4),
        );
        let empty = vehicle(&row(json!({})), None);
        let mut a: Vec<&String> = full.as_object().unwrap().keys().collect();
        let mut b: Vec<&String> = empty.as_object().unwrap().keys().collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn a_second_colour_placeholder_does_not_become_a_second_colour() {
        let v = vehicle(
            &row(json!({
                "kenteken": "XXX99X",
                "eerste_kleur": "ZWART",
                "tweede_kleur": "Niet geregistreerd",
            })),
            None,
        );
        assert_eq!(v["colour"], "ZWART");
        assert_eq!(v["second_colour"], Value::Null);
    }

    #[test]
    fn an_electric_vehicle_reports_the_power_column_rdw_actually_filled() {
        let v = vehicle(
            &row(json!({
                "kenteken": "XXX99X",
                "fuel": [{
                    "brandstof_omschrijving": "Elektriciteit",
                    "netto_max_vermogen_elektrisch": "220.00",
                    "actie_radius_enkel_elektrisch_wltp": 533,
                }],
            })),
            None,
        );
        assert_eq!(v["power_kw"], 220.0);
        assert_eq!(v["electric_range_km"], 533.0);
        assert_eq!(v["fuels"], json!(["Elektriciteit"]));
    }

    #[test]
    fn co2_says_which_test_cycle_produced_it() {
        // WLTP and NEDC numbers are not comparable, so a bare number would be
        // a figure a consumer cannot safely use.
        let wltp =
            json!({"emissie_co2_gecombineerd_wltp": 243, "co2_uitstoot_gecombineerd": "180"});
        assert_eq!(co2(&wltp), Some((243.0, "wltp")));

        let nedc = json!({"co2_uitstoot_gecombineerd": "180"});
        assert_eq!(co2(&nedc), Some((180.0, "nedc")));

        assert_eq!(co2(&json!({})), None);
    }

    #[test]
    fn a_hybrid_reports_every_fuel_in_order() {
        let v = vehicle(
            &row(json!({
                "kenteken": "X99XXX",
                "fuel": [
                    {"brandstof_omschrijving": "Benzine", "nettomaximumvermogen": "70.00"},
                    {"brandstof_omschrijving": "Elektriciteit", "nettomaximumvermogen": "30.00"},
                ],
            })),
            None,
        );
        assert_eq!(v["fuels"], json!(["Benzine", "Elektriciteit"]));
        assert_eq!(v["power_kw"], 70.0, "the primary fuel's power leads");
    }

    #[test]
    fn the_derived_plate_is_the_grouped_form() {
        let v = vehicle(&row(json!({"kenteken": "X99XXX"})), None);
        assert_eq!(v["plate"], "X-99-XXX");
    }

    #[test]
    fn a_defect_row_derives_an_iso_date_and_keeps_an_unknown_code_visible() {
        let d = defect(&row(json!({
            "kenteken": "999XX9",
            "meld_datum_door_keuringsinstantie": "20251010",
            "gebrek_identificatie": "AC4",
            "gebrek_omschrijving": Value::Null,
        })));
        assert_eq!(d["inspection_date"], "2025-10-10");
        assert_eq!(d["code"], "AC4");
        assert_eq!(d["description"], Value::Null);
    }
}

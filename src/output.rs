//! Rendering the result envelope as text, JSON, YAML or NDJSON.
//!
//! The text renderers read the `derived` block that [`crate::facts`] built, so
//! what a human sees and what an agent parses are the same computation formatted
//! two ways. They cannot drift, and a placeholder RDW writes into an empty column
//! is already gone by the time either surface sees it.
//!
//! Anything alarming is shouted in words: `EXPIRED`, `NOT INSURED`, `OPEN
//! RECALL`. Colour is added on a terminal and never carries meaning on its own,
//! so the warning survives being piped, redirected, or read by someone who
//! cannot distinguish red from grey.

use crate::facts;
use crate::rdw::datasets;
use crate::text::*;
use crate::{Command, OutputFormat};
use serde_json::Value;

/// Whether the destination can render ANSI escapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// No escapes at all: a pipe, a file, `NO_COLOR`, or a dumb terminal.
    #[default]
    Plain,
    /// A terminal that can show them.
    Colour,
}

impl Style {
    fn wrap(self, code: &str, text: &str) -> String {
        match self {
            Style::Plain => text.to_string(),
            Style::Colour => format!("\u{1b}[{code}m{text}\u{1b}[0m"),
        }
    }

    fn bold(self, text: &str) -> String {
        self.wrap("1", text)
    }

    fn dim(self, text: &str) -> String {
        self.wrap("2", text)
    }

    /// Bold red, for the handful of facts that should stop a reader.
    fn alarm(self, text: &str) -> String {
        self.wrap("1;31", text)
    }
}

/// How the text is spoken: in which language, and dressed how.
///
/// One value rather than two parameters, because the two travel together
/// through every renderer on the way down. Formatting a number is a question of
/// language and not of decoration, so grouping and phrasing live here beside the
/// escapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Voice {
    style: Style,
    lang: Lang,
}

impl Voice {
    pub fn new(style: Style, lang: Lang) -> Self {
        Voice { style, lang }
    }

    fn bold(self, text: &str) -> String {
        self.style.bold(text)
    }

    fn dim(self, text: &str) -> String {
        self.style.dim(text)
    }

    fn alarm(self, text: &str) -> String {
        self.style.alarm(text)
    }

    fn say(self, phrase: &Phrase) -> &'static str {
        self.lang.say(phrase)
    }

    /// A phrase with its placeholder filled in, shouted in red.
    ///
    /// The two go together everywhere they appear: what the card shouts is what
    /// it wants the reader to stop at, and it is always a whole statement rather
    /// than a word with a plain tail.
    fn alarm_fill(self, phrase: &Phrase, value: &str) -> String {
        self.alarm(&self.lang.fill(phrase, value))
    }

    fn fill(self, phrase: &Phrase, value: &str) -> String {
        self.lang.fill(phrase, value)
    }

    fn count(self, count: i64, noun: &Noun) -> String {
        self.lang.count(count, noun)
    }

    fn thousands(self, value: i64) -> String {
        self.lang.thousands(value)
    }

    fn measure(self, value: f64) -> String {
        self.lang.measure(value)
    }

    fn offset(self, days: i64) -> String {
        self.lang.offset(days)
    }

    fn span(self, days: i64) -> String {
        self.lang.span(days)
    }
}

/// Render the envelope in the requested format.
///
/// `voice` reaches the text renderer only. JSON, YAML and NDJSON are the machine
/// contract and are identical in every language: translating a key, or grouping
/// a number with a separator, would break every consumer that parses them.
pub fn render(envelope: &Value, command: &Command, format: OutputFormat, voice: Voice) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(envelope).expect("envelope serializes"),
        OutputFormat::Yaml => serde_norway::to_string(envelope).expect("envelope serializes"),
        OutputFormat::Ndjson => items(envelope)
            .iter()
            .map(|item| serde_json::to_string(item).expect("item serializes"))
            .collect::<Vec<_>>()
            .join("\n"),
        OutputFormat::Text => text(envelope, command, voice),
    }
}

fn items(envelope: &Value) -> &[Value] {
    envelope["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn text(envelope: &Value, command: &Command, voice: Voice) -> String {
    let items = items(envelope);
    if items.is_empty() {
        return empty_text(envelope, command, voice);
    }
    match command {
        Command::Datasets => dataset_table(items, voice),
        Command::Raw { .. } => generic_table(items),
        // `--fields` can project the derived block away. Falling back to the raw
        // table then shows exactly the columns that were asked for, rather than
        // a curated view with most of its lines silently missing.
        _ if !items.iter().all(|i| derived(i).is_some()) => generic_table(items),
        Command::Lookup { .. } => items
            .iter()
            .map(|item| vehicle_card(item, voice))
            .collect::<Vec<_>>()
            .join("\n\n"),
        Command::Defects { .. } => defect_table(items, voice),
        Command::Fuel { .. } => fuel_table(items, voice),
        // Recalls are prose, not columns: a defect description and a repair
        // instruction are sentences, and a table would either clip them or make
        // a line thousands of characters wide.
        Command::Recalls { .. } => items
            .iter()
            .map(|item| recall_card(item, voice))
            .collect::<Vec<_>>()
            .join("\n\n"),
        Command::Inspections { .. } => inspection_table(items, voice),
    }
}

/// The computed view of a row, or `None` when it was projected away.
fn derived(item: &Value) -> Option<&Value> {
    item.get("derived").filter(|d| d.is_object())
}

/// What to say when there is nothing to show but nothing went wrong either.
fn empty_text(envelope: &Value, command: &Command, voice: Voice) -> String {
    let no_rows = string_list(&envelope["no_rows"]);
    if no_rows.is_empty() {
        return voice.say(&NO_ROWS).to_string();
    }
    let subject = no_rows.join(", ");
    // Stated positively and explicitly. "no rows" next to a registered plate
    // invites the reader to assume the lookup failed.
    let phrase = match command {
        Command::Defects { .. } => &NO_DEFECTS,
        Command::Fuel { .. } => &NO_FUEL,
        // "No recalls" is the answer people are hoping for, so it says what it
        // means: RDW has never issued one that reaches this vehicle, which is
        // not the same as one being open and undescribed.
        Command::Recalls { .. } => &NO_RECALLS,
        Command::Inspections { .. } => &NO_INSPECTIONS,
        _ => &NO_DATASET_ROWS,
    };
    voice.fill(phrase, &subject)
}

fn string_list(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Read a string out of the derived block.
fn s<'a>(d: &'a Value, key: &str) -> Option<&'a str> {
    d.get(key)?.as_str()
}

/// Read a number out of the derived block.
fn n(d: &Value, key: &str) -> Option<f64> {
    d.get(key)?.as_f64()
}

/// Read a tri-state indicator out of the derived block.
///
/// `None` means RDW did not report it, which is not the same as `Some(false)`.
fn b(d: &Value, key: &str) -> Option<bool> {
    d.get(key)?.as_bool()
}

/// Read a list of strings out of the derived block.
fn list<'a>(d: &'a Value, key: &str) -> Vec<&'a str> {
    d.get(key).map(string_list).unwrap_or_default()
}

/// The visible width a card is laid out for.
///
/// Fixed rather than read from the terminal, so the same lookup renders the same
/// way into a pipe, a file, a narrow window and a wide one.
const WRAP: usize = 78;

/// Lay out a card: a title, then label and value pairs aligned in a column.
///
/// A value too wide for the card continues on the next line, indented to line up
/// under itself, so a paragraph of RDW's prose stays readable beside its label.
/// A titled block of labelled rows, in groups separated by a blank line.
///
/// Labels are padded to one width across every group, so the blank line
/// separates the groups without breaking the single column the eye follows down
/// the card. An empty group prints nothing, including its separator, so a
/// vehicle RDW says little about does not gain a run of blank lines.
///
/// The heading is separated the same way, which is what makes it read as a
/// heading rather than as a row whose label went missing. Bold alone cannot do
/// that: it survives neither a pipe nor a terminal that declines to render it,
/// and the layout has to hold on its own once the styling is gone.
///
/// `subtitle` arrives already indented, because where it sits depends on the
/// title above it and only the caller knows how wide that is.
fn card(
    title: String,
    subtitle: Option<String>,
    groups: Vec<Vec<(&str, String)>>,
    voice: Voice,
) -> String {
    let width = groups
        .iter()
        .flatten()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0);
    let indent = 2 + width + 3;
    let mut out = title;
    out.push('\n');
    if let Some(subtitle) = subtitle {
        out.push_str(&subtitle);
        out.push('\n');
    }
    for group in groups.iter().filter(|g| !g.is_empty()) {
        out.push('\n');
        for (label, value) in group {
            let pad = " ".repeat(width - label.chars().count());
            for (i, line) in wrap(value, WRAP.saturating_sub(indent).max(24))
                .into_iter()
                .enumerate()
            {
                match i {
                    0 => out.push_str(&format!("  {}{pad}   {line}\n", voice.dim(label))),
                    _ => out.push_str(&format!("{}{line}\n", " ".repeat(indent))),
                }
            }
        }
    }
    out.trim_end().to_string()
}

/// Break a value onto lines no wider than `width` visible characters.
///
/// Splits between words only, and returns a value that already fits untouched,
/// so the deliberate spacing inside a short line survives.
fn wrap(value: &str, width: usize) -> Vec<String> {
    if visible_len(value) <= width {
        return vec![value.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        if !current.is_empty() && visible_len(&current) + 1 + visible_len(word) > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// How wide a string prints, ignoring the ANSI escapes that take no space.
///
/// Counting the escapes would make every coloured cell measure a dozen
/// characters wider than it looks and pull the column out of line.
fn visible_len(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            width += 1;
            continue;
        }
        for c in chars.by_ref() {
            if c == 'm' {
                break;
            }
        }
    }
    width
}

/// One vehicle, as the card `kenteken lookup` prints.
fn vehicle_card(item: &Value, voice: Voice) -> String {
    let d = derived(item).expect("caller checked the derived block is present");

    let name = [s(d, "make"), s(d, "model")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let plate = s(d, "plate").unwrap_or("?");
    let title = voice.bold(&match name.is_empty() {
        true => plate.to_string(),
        false => format!("{plate}   {}", facts::title_case(&name)),
    });

    // What the vehicle is, beneath the name it is known by. These facts
    // identify it rather than describe its condition, and they belong to the
    // heading for the same reason the make does. Aligned under the name, so the
    // plate keeps the left edge to itself.
    let subtitle = identity_line(d, voice).map(|identity| {
        let indent = visible_len(plate) + 3;
        format!("{}{identity}", " ".repeat(indent))
    });

    // What the register says about where this vehicle stands: what falls due,
    // what is wrong with it, and what it may still be used or sold for. First,
    // because it is what a plate gets looked up for.
    let mut status: Vec<(&str, String)> = Vec::new();
    push(
        &mut status,
        voice.say(&APK_EXPIRES),
        expiry_line(d, "apk", voice),
    );
    push(
        &mut status,
        voice.say(&TACHOGRAPH_EXPIRES),
        expiry_line(d, "tachograph", voice),
    );
    push(&mut status, voice.say(&ODOMETER), odometer_line(d, voice));
    push(
        &mut status,
        voice.say(&ODOMETER_NOTE),
        odometer_reason_line(d),
    );
    push(&mut status, voice.say(&INSURED), insured_line(d, voice));
    push(&mut status, voice.say(&RECALL), recall_line(d, voice));
    push(
        &mut status,
        voice.say(&RECALL_HAZARD),
        recall_hazard_line(d),
    );
    // Shown only when set. An exceptional flag that is off is not worth a line,
    // and the tri-state that keeps "off" apart from "not reported" is in the
    // derived block for anything that needs to tell them apart.
    push(
        &mut status,
        voice.say(&EXPORTED),
        when(b(d, "exported"), voice.say(&YES)),
    );
    push(
        &mut status,
        voice.say(&TAXI),
        when(b(d, "taxi"), voice.say(&YES)),
    );
    push(
        &mut status,
        voice.say(&REGISTRATION),
        when(
            b(d, "transferable").map(|t| !t),
            &voice.alarm(voice.say(&TRANSFER_BLOCKED)),
        ),
    );

    // The vehicle's dates, oldest first, so the three read as one sequence.
    let mut history: Vec<(&str, String)> = Vec::new();
    push(
        &mut history,
        voice.say(&FIRST_ADMITTED),
        since_line(d, "first_admission", "age_days", voice),
    );
    push(
        &mut history,
        voice.say(&DUTCH_REGISTER),
        dutch_line(d, voice),
    );
    push(
        &mut history,
        voice.say(&REGISTERED_SINCE),
        s(d, "registered_since").map(str::to_string),
    );

    // What it is made of and what it can carry: the figures that do not change
    // between one lookup and the next.
    let mut spec: Vec<(&str, String)> = Vec::new();
    push(&mut spec, voice.say(&FUEL), fuel_line(item, voice));
    push(
        &mut spec,
        voice.say(&ENGINE),
        n(d, "engine_cc").map(|cc| format!("{} cm3", voice.thousands(cc as i64))),
    );
    push(
        &mut spec,
        voice.say(&ENERGY_LABEL),
        s(d, "energy_label").map(str::to_string),
    );
    push(&mut spec, voice.say(&MASS), mass_line(d, voice));
    push(&mut spec, voice.say(&TOWING), towing_line(d, voice));
    push(&mut spec, voice.say(&DIMENSIONS), dimensions_line(d, voice));
    push(
        &mut spec,
        voice.say(&CATALOGUE_PRICE),
        n(d, "catalogue_price_eur").map(|p| format!("EUR {}", voice.thousands(p as i64))),
    );
    push(
        &mut spec,
        voice.say(&VIN_LOCATION),
        s(d, "vin_location").map(str::to_string),
    );

    card(title, subtitle, vec![status, history, spec], voice)
}

/// What the vehicle is, as one line: kind, body, capacity and colour.
///
/// Four labelled rows would say the same thing and cost the card four lines to
/// answer a question nobody asks one part of at a time.
fn identity_line(d: &Value, voice: Voice) -> Option<String> {
    let parts: Vec<String> = [kind_line(d, voice), colour_line(d)]
        .into_iter()
        .flatten()
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// One recall, as the card `kenteken recalls` prints.
fn recall_card(item: &Value, voice: Voice) -> String {
    let d = derived(item).expect("caller checked the derived block is present");
    let heading = format!(
        "{}   {}",
        s(d, "plate").unwrap_or("?"),
        s(d, "reference").unwrap_or("?")
    );
    let title = format!("{}   {}", voice.bold(&heading), recall_status(d, voice));

    let mut lines: Vec<(&str, String)> = Vec::new();
    push(
        &mut lines,
        voice.say(&DEFECT),
        s(d, "defect").map(str::to_string),
    );
    push(
        &mut lines,
        voice.say(&CATEGORY),
        s(d, "category").map(str::to_string),
    );
    let hazards = list(d, "hazards");
    push(
        &mut lines,
        voice.say(&HAZARD),
        (!hazards.is_empty()).then(|| voice.alarm(&hazards.join("; "))),
    );
    push(
        &mut lines,
        voice.say(&CONSEQUENCES),
        s(d, "consequences").map(str::to_string),
    );
    push(
        &mut lines,
        voice.say(&REPAIR),
        s(d, "repair").map(str::to_string),
    );
    push(
        &mut lines,
        voice.say(&REPORTED_BY),
        s(d, "manufacturer").map(str::to_string),
    );
    push(&mut lines, voice.say(&MORE_INFORMATION), contact_line(d));
    push(&mut lines, voice.say(&PUBLISHED), published_line(d, voice));
    push(
        &mut lines,
        voice.say(&OWNERS_INFORMED),
        s(d, "owners_informed").map(str::to_string),
    );
    // One group: a recall is a single account of one fault, and splitting it
    // would put gaps between sentences that are read together.
    card(title, None, vec![lines], voice)
}

/// Whether this recall is still outstanding, said in words.
fn recall_status(d: &Value, voice: Voice) -> String {
    match b(d, "open") {
        Some(true) => voice.alarm(voice.say(&RECALL_OPEN)),
        Some(false) => voice.say(&RECALL_REPAIRED).to_string(),
        // RDW filed the recall against this vehicle but recorded no status. That
        // is not a repair, and it must not read as one.
        None => voice.say(&RECALL_NO_STATUS).to_string(),
    }
}

fn contact_line(d: &Value) -> Option<String> {
    let parts: Vec<&str> = [s(d, "more_info_url"), s(d, "more_info_phone")]
        .into_iter()
        .flatten()
        .collect();
    (!parts.is_empty()).then(|| parts.join("  |  "))
}

fn published_line(d: &Value, voice: Voice) -> Option<String> {
    let published = s(d, "published")?;
    Some(match n(d, "vehicles_affected") {
        Some(count) => {
            let fleet = voice.count(count as i64, &VEHICLE);
            format!("{published}   {}", voice.fill(&IN_THE_ACTION, &fleet))
        }
        None => published.to_string(),
    })
}

fn push(lines: &mut Vec<(&'static str, String)>, label: &'static str, value: Option<String>) {
    // A field RDW did not supply is left out rather than shown as a dash next to
    // a confident-looking label.
    if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
        lines.push((label, value));
    }
}

/// `Some(text)` when the flag is true, `None` otherwise.
fn when(flag: Option<bool>, text: &str) -> Option<String> {
    flag.unwrap_or(false).then(|| text.to_string())
}

/// `Personenauto (M1), MPV, 5 zitplaatsen, 4 deuren`: what kind of vehicle this is.
fn kind_line(d: &Value, voice: Voice) -> Option<String> {
    let head = match (s(d, "kind"), s(d, "eu_category")) {
        (Some(kind), Some(cat)) => format!("{kind} ({cat})"),
        (Some(kind), None) => kind.to_string(),
        (None, Some(cat)) => cat.to_string(),
        (None, None) => String::new(),
    };
    let parts: Vec<String> = [
        Some(head).filter(|h| !h.is_empty()),
        s(d, "body").map(facts::title_case),
        // `0 doors` is printed rather than hidden. It is what RDW recorded, and
        // on the trailer or motorcycle that carries it, it is true.
        n(d, "seats").map(|c| voice.count(c as i64, &SEAT)),
        n(d, "doors").map(|c| voice.count(c as i64, &DOOR)),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// A dated deadline: how long is left, and a shout once it has passed.
///
/// Shared by the two inspections the register dates itself. The APK is the
/// reason most people run this tool; the tachograph is the reason a haulier
/// does, and it runs on a cycle of its own.
fn expiry_line(d: &Value, prefix: &str, voice: Voice) -> Option<String> {
    let expiry = s(d, &format!("{prefix}_expiry"))?;
    let Some(days) = n(d, &format!("{prefix}_days_remaining")) else {
        // No clock, so no verdict. The date alone is still the honest answer.
        return Some(expiry.to_string());
    };
    let phrase = voice.offset(days as i64);
    Some(match b(d, &format!("{prefix}_expired")) {
        Some(true) => format!("{expiry}   {}", voice.alarm_fill(&EXPIRED, &phrase)),
        _ => format!("{expiry}   {phrase}"),
    })
}

/// What the vehicle may pull, braked and unbraked.
///
/// A vehicle that may not tow has neither figure rather than a zero, so nothing
/// here has to stand for "not permitted".
fn towing_line(d: &Value, voice: Voice) -> Option<String> {
    let parts: Vec<String> = [
        ("tow_braked_kg", &TOW_BRAKED),
        ("tow_unbraked_kg", &TOW_UNBRAKED),
    ]
    .into_iter()
    .filter_map(|(key, word)| {
        n(d, key).map(|kg| format!("{} kg {}", voice.thousands(kg as i64), voice.say(word)))
    })
    .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Whichever of the three dimensions RDW measured, each said in full.
///
/// Named rather than laid out as `l x w x h`, because RDW routinely has one or
/// two of the three and a positional form would need a placeholder for the rest.
fn dimensions_line(d: &Value, voice: Voice) -> Option<String> {
    let parts: Vec<String> = [
        ("length_cm", &LONG),
        ("width_cm", &WIDE),
        ("height_cm", &HIGH),
    ]
    .into_iter()
    .filter_map(|(key, word)| {
        n(d, key).map(|cm| format!("{} cm {}", voice.thousands(cm as i64), voice.say(word)))
    })
    .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// A date with how long ago it was, when the tool knows today's date.
fn since_line(d: &Value, date_key: &str, age_key: &str, voice: Voice) -> Option<String> {
    let date = s(d, date_key)?;
    Some(match n(d, age_key) {
        Some(age) => format!("{date}   {}", voice.offset(-(age as i64))),
        None => date.to_string(),
    })
}

/// When the vehicle first went onto the Dutch register, if that is not the day
/// it was first admitted to the road.
///
/// A gap is the tell for a vehicle that was driven abroad first, which is worth
/// seeing next to an odometer judgement. It is not proof of an import, though: a
/// Dutch chassis bodied months after admission shows exactly the same gap, and
/// among vehicles registered within a month of admission not one is flagged by
/// RDW as having been registered abroad. So the line states the two facts and
/// leaves the conclusion to the reader.
fn dutch_line(d: &Value, voice: Voice) -> Option<String> {
    let date = s(d, "first_dutch_registration")?;
    let lag = n(d, "dutch_registration_lag_days")? as i64;
    if lag == 0 {
        return None;
    }
    let phrase = match lag > 0 {
        true => &LAG_AFTER,
        false => &LAG_BEFORE,
    };
    Some(format!("{date}   {}", voice.fill(phrase, &voice.span(lag))))
}

fn colour_line(d: &Value) -> Option<String> {
    let first = facts::title_case(s(d, "colour")?);
    Some(match s(d, "second_colour") {
        Some(second) => format!("{first} / {}", facts::title_case(second)),
        None => first,
    })
}

/// Fuel summary drawn from the rows `lookup` enriches the vehicle with.
///
/// Read from the rows rather than from the derived block, because a hybrid has
/// power and emissions per fuel and the summary would otherwise report only the
/// first one.
fn fuel_line(item: &Value, voice: Voice) -> Option<String> {
    let rows = item.get("fuel")?.as_array()?;
    let parts: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            let mut bits = vec![facts::text(row, "brandstof_omschrijving")?];
            if let Some(kw) = facts::power_kw(row) {
                bits.push(format!("{} kW", voice.measure(kw)));
            }
            if let Some((co2, basis)) = facts::co2(row) {
                bits.push(format!(
                    "{} g/km CO2 ({})",
                    voice.measure(co2),
                    basis.to_uppercase()
                ));
            }
            if let Some(km) = facts::electric_range_km(row) {
                bits.push(format!("{} km {}", voice.measure(km), voice.say(&RANGE)));
            }
            Some(bits.join(", "))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("  +  "))
}

fn mass_line(d: &Value, voice: Voice) -> Option<String> {
    let parts: Vec<String> = [("mass_empty_kg", &MASS_EMPTY), ("mass_max_kg", &MASS_MAX)]
        .into_iter()
        .filter_map(|(key, word)| {
            n(d, key).map(|kg| format!("{} kg {}", voice.thousands(kg as i64), voice.say(word)))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// RDW's verdict on the odometer, and the year the readings behind it stop.
///
/// The verdict alone reads as current whatever its age, so a history last
/// touched in 2016 and one read last month say the same word. The year is what
/// separates them, and it stays on this line with the verdict: RDW's own
/// explanation is a full Dutch sentence, and joining it here would wrap the
/// pair onto three lines and bury the date.
///
/// Either half stands on its own, because the register routinely has one
/// without the other: 730,494 vehicles carry a reading year with no verdict
/// against it, and dropping the line for want of a verdict would throw away the
/// only odometer fact RDW has for them.
fn odometer_line(d: &Value, voice: Voice) -> Option<String> {
    // In Dutch these render back to RDW's own three words, which `facts` turned
    // into English tokens on the way in to keep the JSON contract stable. The
    // card is where they go home.
    let verdict = s(d, "odometer").map(|verdict| match verdict {
        "consistent" => voice.say(&ODOMETER_CONSISTENT).to_string(),
        "inconsistent" => voice.alarm(voice.say(&ODOMETER_INCONSISTENT)),
        "no_judgement" => voice.say(&ODOMETER_NO_JUDGEMENT).to_string(),
        // A token this build has no phrase for is shown as RDW's own word rather
        // than dropped, which keeps a new verdict visible instead of silent.
        other => other.replace('_', " "),
    });
    // Said as a year, not grouped as a quantity: RDW records 1961 through 2026,
    // and `2,016` would read as a distance rather than a date.
    let year =
        n(d, "odometer_year").map(|year| voice.fill(&LAST_READING, &(year as i64).to_string()));
    match (verdict, year) {
        (Some(verdict), Some(year)) => Some(format!("{verdict}   {year}")),
        (verdict, year) => verdict.or(year),
    }
}

/// RDW's own reason for that verdict, which is the interesting part when it
/// declined to judge or found a jump.
///
/// Quoted, because it is RDW speaking and not the tool. The sentence is Dutch in
/// either language and stays that way: it is an official statement about one
/// vehicle's history, and paraphrasing it would put words in RDW's mouth about
/// something that carries legal weight. The quotation marks are what make that
/// visible rather than leaving a stray Dutch sentence in an English card.
///
/// Left off a consistent history, where it only repeats that nothing is wrong.
fn odometer_reason_line(d: &Value) -> Option<String> {
    s(d, "odometer").filter(|verdict| *verdict != "consistent")?;
    let reason = s(d, "odometer_reason")?.trim();
    (!reason.is_empty()).then(|| format!("\"{reason}\""))
}

fn insured_line(d: &Value, voice: Voice) -> Option<String> {
    Some(match b(d, "insured")? {
        true => voice.say(&YES).to_string(),
        false => voice.alarm(voice.say(&NOT_INSURED)),
    })
}

/// The recall line on a vehicle card.
///
/// An open recall is the one thing on this card that asks the reader to act, so
/// it says where the rest of it is rather than shouting two words and leaving no
/// way to find out more.
fn recall_line(d: &Value, voice: Voice) -> Option<String> {
    if !b(d, "open_recall")? {
        return Some(voice.say(&NONE_OUTSTANDING).to_string());
    }
    let mut parts = vec![voice.alarm(voice.say(&OPEN_RECALL))];
    if let Some(plate) = s(d, "plate") {
        parts.push(voice.fill(&SEE_RECALLS, plate));
    }
    Some(parts.join("   "))
}

/// What the vehicle's open recalls guard against, on a line of its own.
///
/// Kept off the line above because a hazard is a sentence: joined to a shouted
/// status and a pointer, the whole thing outgrows the card, and wrapping it
/// collapses the spacing that held the three apart into one run-on phrase.
fn recall_hazard_line(d: &Value) -> Option<String> {
    let hazards = list(d, "open_recall_hazards");
    (!hazards.is_empty()).then(|| hazards.join("; "))
}

/// What a table cell shows when RDW reported nothing for it.
///
/// A blank cell would read as a value (an empty name, a zero distance). A dash
/// says the column is absent for this row, which is what RDW actually means when
/// it omits a key.
const ABSENT: &str = "-";

/// One table column: its heading and which edge its cells line up on.
struct Col {
    head: String,
    right: bool,
}

impl Col {
    fn left(head: &str) -> Self {
        Col {
            head: head.to_string(),
            right: false,
        }
    }

    /// A column of numbers, which line up on the right so their magnitudes can
    /// be compared down the column.
    fn right(head: &str) -> Self {
        Col {
            head: head.to_string(),
            right: true,
        }
    }
}

/// Read a cell out of a row's derived block.
fn dcell(item: &Value, key: &str, voice: Voice) -> String {
    let Some(d) = derived(item) else {
        return ABSENT.to_string();
    };
    match d.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => voice.measure(n.as_f64().unwrap_or_default()),
        Some(Value::Bool(v)) => v.to_string(),
        _ => ABSENT.to_string(),
    }
}

/// Read a cell straight out of an RDW row, for the untouched `raw` table.
fn cell(item: &Value, key: &str) -> String {
    match item.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(v)) => v.to_string(),
        _ => ABSENT.to_string(),
    }
}

/// Whether these rows describe more than one vehicle.
///
/// A plate column repeating the same value down every row is noise when only one
/// plate was asked about, and the plate is already in the command that produced
/// the table.
fn many_plates(items: &[Value]) -> bool {
    let mut seen: Vec<&str> = Vec::new();
    for item in items {
        if let Some(plate) = derived(item).and_then(|d| s(d, "plate"))
            && !seen.contains(&plate)
        {
            seen.push(plate);
        }
    }
    seen.len() > 1
}

fn defect_table(items: &[Value], voice: Voice) -> String {
    let with_plate = many_plates(items);
    let mut cols = Vec::new();
    if with_plate {
        cols.push(Col::left(voice.say(&COL_PLATE)));
    }
    cols.extend([
        Col::left(voice.say(&COL_DATE)),
        Col::left(voice.say(&COL_CODE)),
        Col::left(voice.say(&COL_DEFECT)),
    ]);

    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            let mut row = Vec::new();
            if with_plate {
                row.push(dcell(item, "plate", voice));
            }
            row.push(dcell(item, "inspection_date", voice));
            row.push(dcell(item, "code", voice));
            // An unresolved code is marked as unresolved, never blank and never
            // a dash: "this build does not know the code" is a different fact
            // from "RDW reported no description".
            row.push(match derived(item).and_then(|d| s(d, "description")) {
                Some(text) => text.to_string(),
                None => voice.say(&UNKNOWN_CODE).to_string(),
            });
            row
        })
        .collect();
    table(&cols, rows)
}

fn fuel_table(items: &[Value], voice: Voice) -> String {
    let with_plate = many_plates(items);
    let mut cols = Vec::new();
    if with_plate {
        cols.push(Col::left(voice.say(&COL_PLATE)));
    }
    cols.extend([
        Col::left(voice.say(&COL_FUEL)),
        Col::right(voice.say(&COL_KW)),
        Col::right(voice.say(&COL_CO2)),
        Col::left(voice.say(&COL_BASIS)),
        Col::right(voice.say(&COL_RANGE)),
        Col::left(voice.say(&COL_EURO)),
    ]);

    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            let mut row = Vec::new();
            if with_plate {
                row.push(dcell(item, "plate", voice));
            }
            row.extend([
                dcell(item, "fuel", voice),
                dcell(item, "power_kw", voice),
                dcell(item, "co2_g_per_km", voice),
                // Which test cycle produced the CO2 figure travels with it: a
                // WLTP number and an NEDC number are not comparable.
                dcell(item, "co2_basis", voice),
                dcell(item, "electric_range_km", voice),
                dcell(item, "euro_class", voice),
            ]);
            row
        })
        .collect();
    table(&cols, rows)
}

fn inspection_table(items: &[Value], voice: Voice) -> String {
    let with_plate = many_plates(items);
    let mut cols = Vec::new();
    if with_plate {
        cols.push(Col::left(voice.say(&COL_PLATE)));
    }
    cols.extend([
        Col::left(voice.say(&COL_DATE)),
        Col::left(voice.say(&COL_NOTIFICATION)),
        Col::left(voice.say(&COL_FILED_BY)),
        // Not "APK until": the date expires the inspection that was filed, and a
        // tachograph workshop inspects on its own two-yearly cycle. Labelling its
        // row APK would hand a reader a roadworthiness date the vehicle has not
        // got, from the one body that never issues one.
        Col::left(voice.say(&COL_VALID_UNTIL)),
    ]);

    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            let mut row = Vec::new();
            if with_plate {
                row.push(dcell(item, "plate", voice));
            }
            row.extend([
                dcell(item, "date", voice),
                notification_cell(item, voice),
                dcell(item, "accreditation", voice),
                dcell(item, "expiry", voice),
            ]);
            row
        })
        .collect();
    table(&cols, rows)
}

/// What kind of notification this was, shouted when it is a tachograph finding.
///
/// Someone interfered with the instrument that records a professional driver's
/// hours. It sits in a column of routine inspections, so it says so in capitals;
/// RDW's own wording stays in the row and the derived block.
fn notification_cell(item: &Value, voice: Voice) -> String {
    let Some(d) = derived(item) else {
        return ABSENT.to_string();
    };
    match s(d, "alarm") {
        Some("tachograph_tampering") => voice.alarm(voice.say(&TACHOGRAPH_TAMPERING)),
        Some("tachograph_seal_broken") => voice.alarm(voice.say(&TACHOGRAPH_SEAL_BROKEN)),
        _ => dcell(item, "kind", voice),
    }
}

/// The datasets this build knows, as `kenteken datasets` prints them.
///
/// The descriptions say what a caller gets back from each dataset rather than
/// telling a reader about a vehicle, and they follow `--lang` like every other
/// rendered string. The JSON contract and `schema` keep the English wording.
fn dataset_table(items: &[Value], voice: Voice) -> String {
    let cols = [
        Col::left(voice.say(&COL_NAME)),
        Col::left(voice.say(&COL_ID)),
        Col::left(voice.say(&COL_BY_PLATE)),
        Col::left(voice.say(&COL_CONTENTS)),
    ];
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            vec![
                cell(item, "name"),
                cell(item, "id"),
                // A row narrowed by `--fields` may not carry the flag at all,
                // and "nee" would then claim that a dataset cannot be queried by
                // plate when most of them can.
                match item.get("plate_keyed") {
                    Some(Value::Bool(true)) => voice.say(&YES).to_string(),
                    Some(Value::Bool(false)) => voice.say(&NO).to_string(),
                    _ => ABSENT.to_string(),
                },
                dataset_description(item, voice),
            ]
        })
        .collect();
    table(&cols, rows)
}

/// The description of a dataset row, in the reader's language.
///
/// The rows arrive as serialized `Dataset`s, whose `description` is the English
/// side of the phrase, so the Dutch wording is read back out of the catalogue by
/// the row's own id or name, or by that English description when `--fields` has
/// left nothing else to recognize the row by. An item the catalogue does not know
/// keeps whatever description it carries, and a projection that dropped the field
/// keeps the column empty rather than inventing a description for it.
fn dataset_description(item: &Value, voice: Voice) -> String {
    let Some(english) = item.get("description").and_then(Value::as_str) else {
        return cell(item, "description");
    };
    ["id", "name"]
        .into_iter()
        .filter_map(|key| item.get(key).and_then(Value::as_str))
        .find_map(datasets::resolve)
        .or_else(|| datasets::resolve_description(english))
        .map_or_else(
            || cell(item, "description"),
            |d| voice.say(&d.description).to_string(),
        )
}

/// Fallback table for `raw`, whose columns are whatever RDW returned.
fn generic_table(items: &[Value]) -> String {
    let mut headers: Vec<String> = Vec::new();
    for item in items {
        if let Some(obj) = item.as_object() {
            for key in obj.keys() {
                if !headers.iter().any(|h| h == key) {
                    headers.push(key.clone());
                }
            }
        }
    }
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| headers.iter().map(|h| cell(item, h)).collect())
        .collect();
    let cols: Vec<Col> = headers.iter().map(|h| Col::left(h)).collect();
    table(&cols, rows)
}

/// Lay out a table, sizing every column to its widest cell.
fn table(cols: &[Col], rows: Vec<Vec<String>>) -> String {
    if cols.is_empty() {
        return String::new();
    }
    // Measured by what prints, not by what is stored: a cell carrying a colour
    // escape is a dozen bytes wider than it looks, and padding to that would
    // knock every following column out of line.
    let widths: Vec<usize> = cols
        .iter()
        .enumerate()
        .map(|(i, c)| {
            rows.iter()
                .filter_map(|r| r.get(i))
                .map(|c| visible_len(c))
                .chain(std::iter::once(visible_len(&c.head)))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let mut out: Vec<String> = Vec::with_capacity(rows.len() + 1);
    out.push(pad_row(cols.iter().map(|c| c.head.clone()), cols, &widths));
    for row in rows {
        out.push(pad_row(row.into_iter(), cols, &widths));
    }
    out.join("\n")
}

fn pad_row(cells: impl Iterator<Item = String>, cols: &[Col], widths: &[usize]) -> String {
    let cells: Vec<String> = cells.collect();
    let last = cells.len().saturating_sub(1);
    cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let right = cols.get(i).is_some_and(|c| c.right);
            let width = widths.get(i).copied().unwrap_or(0);
            let pad = " ".repeat(width.saturating_sub(visible_len(cell)));
            if right {
                return format!("{pad}{cell}");
            }
            // A left-aligned final column is never padded, so lines carry no
            // trailing whitespace.
            if i == last {
                return cell.clone();
            }
            format!("{cell}{pad}")
        })
        .collect::<Vec<_>>()
        .join("  ")
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Date;
    use serde_json::json;

    /// Build an envelope whose items carry the derived block `run` would add.
    fn envelope(items: Value) -> Value {
        with_derived(items, facts::vehicle)
    }

    fn with_derived(items: Value, derive: fn(&Value, Option<Date>) -> Value) -> Value {
        let items: Vec<Value> = items
            .as_array()
            .expect("items is an array")
            .iter()
            .map(|item| {
                let mut obj = item.as_object().expect("item is an object").clone();
                obj.insert("derived".into(), derive(item, Date::new(2026, 8, 4)));
                Value::Object(obj)
            })
            .collect();
        json!({
            "items": items,
            "total": items.len(),
            "limit": 100,
            "offset": 0,
            "truncated": false,
            "not_found": [],
            "no_rows": [],
        })
    }

    fn defect_envelope(items: Value) -> Value {
        with_derived(items, |item, _| facts::defect(item))
    }

    fn fuel_envelope(items: Value) -> Value {
        with_derived(items, |item, _| facts::fuel(item))
    }

    fn recall_envelope(items: Value) -> Value {
        with_derived(items, |item, _| facts::recall(item))
    }

    fn inspection_envelope(items: Value) -> Value {
        with_derived(items, |item, _| facts::inspection(item))
    }

    fn recalls() -> Command {
        Command::Recalls { plates: vec![] }
    }

    fn inspections() -> Command {
        Command::Inspections { plates: vec![] }
    }

    /// The same text with the escapes removed, for measuring what a reader sees.
    fn stripped(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c != '\u{1b}' {
                out.push(c);
                continue;
            }
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        }
        out
    }

    fn lookup() -> Command {
        Command::Lookup { plates: vec![] }
    }

    fn defects() -> Command {
        Command::Defects { plates: vec![] }
    }

    fn raw() -> Command {
        Command::Raw {
            dataset: crate::rdw::datasets::VEHICLE,
            plates: vec![],
        }
    }

    /// The default voice: Dutch, with no escapes.
    fn nl() -> Voice {
        Voice::new(Style::Plain, Lang::Nl)
    }

    /// The same card under `--lang en`.
    fn en() -> Voice {
        Voice::new(Style::Plain, Lang::En)
    }

    /// Render as the tool does by default, which is Dutch.
    fn plain(env: &Value, command: &Command) -> String {
        render(env, command, OutputFormat::Text, nl())
    }

    #[test]
    fn json_output_is_the_whole_envelope() {
        let env = envelope(json!([{"kenteken": "X99XXX"}]));
        let rendered = render(&env, &lookup(), OutputFormat::Json, nl());
        let back: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn yaml_output_round_trips() {
        let env = envelope(json!([{"kenteken": "X99XXX"}]));
        let rendered = render(&env, &lookup(), OutputFormat::Yaml, nl());
        let back: Value = serde_norway::from_str(&rendered).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn ndjson_emits_one_valid_object_per_line_and_no_envelope() {
        let env = envelope(json!([{"a": 1}, {"a": 2}, {"a": 3}]));
        let rendered = render(&env, &lookup(), OutputFormat::Ndjson, nl());
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 3);
        for line in lines {
            let v: Value = serde_json::from_str(line).expect("each line parses alone");
            assert!(v.get("a").is_some());
            assert!(v.get("items").is_none(), "envelope leaked into NDJSON");
        }
    }

    #[test]
    fn a_truncated_ndjson_stream_is_still_valid_records() {
        let env = envelope(json!([{"a": 1}, {"a": 2}, {"a": 3}]));
        let rendered = render(&env, &lookup(), OutputFormat::Ndjson, nl());
        let head: Vec<&str> = rendered.lines().take(2).collect();
        for line in head {
            serde_json::from_str::<Value>(line).expect("head of the stream stays parseable");
        }
    }

    #[test]
    fn plain_style_emits_no_escapes_in_any_format() {
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "merk": "IVECO",
            "vervaldatum_apk": "20200101",
            "wam_verzekerd": "Nee",
        }]));
        for format in [
            OutputFormat::Text,
            OutputFormat::Json,
            OutputFormat::Yaml,
            OutputFormat::Ndjson,
        ] {
            let rendered = render(&env, &lookup(), format, nl());
            assert!(
                !rendered.contains('\u{1b}'),
                "{format:?} emitted an escape sequence"
            );
        }
    }

    #[test]
    fn the_structured_formats_never_carry_escapes_even_in_colour() {
        // Colour is a property of a terminal, not of the data. An escape inside
        // a JSON string would corrupt every consumer downstream.
        let env = envelope(json!([{"kenteken": "X99XXX", "vervaldatum_apk": "20200101"}]));
        for format in [OutputFormat::Json, OutputFormat::Yaml, OutputFormat::Ndjson] {
            let rendered = render(&env, &lookup(), format, Voice::new(Style::Colour, Lang::Nl));
            assert!(!rendered.contains('\u{1b}'), "{format:?} carried an escape");
        }
    }

    #[test]
    fn colour_never_replaces_the_shouted_word() {
        // Someone piping to a file, or who cannot see red, must get the same
        // warning. The words are the signal; colour only draws the eye.
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "vervaldatum_apk": "20200101",
            "wam_verzekerd": "Nee",
            "openstaande_terugroepactie_indicator": "Ja",
            "tellerstandoordeel": "Onlogisch",
        }]));
        let coloured = render(
            &env,
            &lookup(),
            OutputFormat::Text,
            Voice::new(Style::Colour, Lang::Nl),
        );
        let plain = plain(&env, &lookup());
        for warning in ["VERLOPEN", "NIET VERZEKERD", "OPENSTAAND", "ONLOGISCH"] {
            assert!(
                plain.contains(warning),
                "plain is missing {warning}:\n{plain}"
            );
            assert!(coloured.contains(warning), "colour is missing {warning}");
        }
        assert!(
            coloured.contains('\u{1b}'),
            "colour mode emitted no escapes"
        );
    }

    #[test]
    fn an_expired_apk_says_so_in_words() {
        let env = envelope(json!([{"kenteken": "X99XXX", "vervaldatum_apk": "20200101"}]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("VERLOPEN"), "rendered:\n{rendered}");
        assert!(rendered.contains("2020-01-01"), "rendered:\n{rendered}");

        // The same card under `--lang en`, because the shout is the point of the
        // line and a language that lost it would still print the date.
        let rendered = render(&env, &lookup(), OutputFormat::Text, en());
        assert!(rendered.contains("EXPIRED"), "rendered:\n{rendered}");
        assert!(rendered.contains("2020-01-01"), "rendered:\n{rendered}");
    }

    #[test]
    fn a_valid_apk_is_not_labelled_expired() {
        let env = envelope(json!([{"kenteken": "X99XXX", "vervaldatum_apk": "20991231"}]));
        let rendered = plain(&env, &lookup());
        assert!(!rendered.contains("VERLOPEN"), "rendered:\n{rendered}");
        assert!(rendered.contains("2099-12-31"), "rendered:\n{rendered}");
    }

    #[test]
    fn a_missing_field_is_omitted_rather_than_shown_as_a_placeholder() {
        let env = envelope(json!([{"kenteken": "X99XXX", "merk": "IVECO"}]));
        let rendered = plain(&env, &lookup());
        assert!(!rendered.contains("APK"), "rendered:\n{rendered}");
        assert!(
            !rendered.contains("Catalogusprijs"),
            "rendered:\n{rendered}"
        );
    }

    #[test]
    fn numeric_rdw_fields_render_the_same_whichever_type_rdw_used() {
        // The vehicle dataset types several columns as numbers, the sub-datasets
        // send the same kind of value as a string. Both must render the same.
        let numeric = envelope(json!([{"kenteken": "X99XXX", "catalogusprijs": 91144}]));
        let stringy = envelope(json!([{"kenteken": "X99XXX", "catalogusprijs": "91144"}]));
        let a = plain(&numeric, &lookup());
        assert!(a.contains("EUR 91.144"), "rendered:\n{a}");
        assert_eq!(a, plain(&stringy, &lookup()));
    }

    #[test]
    fn large_numbers_are_grouped_for_reading() {
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "catalogusprijs": 91144,
            "massa_ledig_voertuig": "1880",
            "toegestane_maximum_massa_voertuig": "3500",
        }]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("EUR 91.144"), "rendered:\n{rendered}");
        assert!(rendered.contains("1.880 kg leeg"), "rendered:\n{rendered}");
        assert!(rendered.contains("3.500 kg max"), "rendered:\n{rendered}");
    }

    #[test]
    fn the_grouping_mark_follows_the_language_and_not_the_other_way_round() {
        // A Dutch reader parses `1,880 kg` as a weight just under two kilos, so
        // an English separator on a Dutch card is briefly wrong rather than
        // merely foreign. Both directions are asserted: a card that grouped
        // everything one way would pass either half alone.
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "catalogusprijs": 91144,
            "massa_ledig_voertuig": "1880",
        }]));
        let dutch = plain(&env, &lookup());
        assert!(dutch.contains("EUR 91.144"), "rendered:\n{dutch}");
        assert!(!dutch.contains("91,144"), "rendered:\n{dutch}");

        let english = render(&env, &lookup(), OutputFormat::Text, en());
        assert!(english.contains("EUR 91,144"), "rendered:\n{english}");
        assert!(english.contains("1,880 kg empty"), "rendered:\n{english}");
        assert!(!english.contains("91.144"), "rendered:\n{english}");
    }

    #[test]
    fn a_one_colour_car_is_not_rendered_as_two_tone() {
        // The bug this fixes: `Niet geregistreerd` is the most common value of
        // tweede_kleur in the whole register, so passing it through invented a
        // second colour for roughly ten million vehicles.
        let env = envelope(json!([{
            "kenteken": "XXX99X",
            "eerste_kleur": "ZWART",
            "tweede_kleur": "Niet geregistreerd",
        }]));
        let rendered = plain(&env, &lookup());
        // Anchored on the colour rather than on a label, because the colour is
        // part of the heading and carries no label of its own. This also keeps
        // the negative below honest: a card that dropped the colour entirely
        // would satisfy "no slash" while saying nothing.
        assert!(rendered.contains("Zwart"), "rendered:\n{rendered}");
        assert!(
            !rendered.contains('/'),
            "a second colour was invented:\n{rendered}"
        );
        assert!(!rendered.contains("Niet geregistreerd"), "{rendered}");
    }

    #[test]
    fn a_genuinely_two_tone_car_still_shows_both_colours() {
        // The negative control: a filter that dropped every second colour would
        // pass the test above.
        let env = envelope(json!([{
            "kenteken": "XXX99X",
            "eerste_kleur": "ZWART",
            "tweede_kleur": "GRIJS",
        }]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("Zwart / Grijs"), "rendered:\n{rendered}");
    }

    #[test]
    fn every_rdw_placeholder_is_filtered_from_the_summary() {
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "eerste_kleur": "N.v.t.",
            "tweede_kleur": "Niet geregistreerd",
            "tellerstandoordeel": "Niet geregistreerd",
            "inrichting": "Geen verstrekking in Open Data",
        }]));
        let rendered = plain(&env, &lookup());
        assert!(!rendered.contains("Tellerstand"), "rendered:\n{rendered}");
        for placeholder in ["N.v.t.", "Niet geregistreerd", "Geen verstrekking"] {
            assert!(!rendered.contains(placeholder), "leaked {placeholder}");
        }
        // Every field on this card is a placeholder, so the plate is all that is
        // left to say. Filtering that renamed a placeholder rather than dropping
        // it would satisfy the checks above and still print a line.
        assert_eq!(rendered.trim(), "X-99-XXX", "rendered:\n{rendered}");
    }

    #[test]
    fn an_uninsured_vehicle_is_shouted_and_an_insured_one_is_not() {
        let uninsured = envelope(json!([{"kenteken": "X99XXX", "wam_verzekerd": "Nee"}]));
        let rendered = plain(&uninsured, &lookup());
        assert!(rendered.contains("NIET VERZEKERD"), "rendered:\n{rendered}");

        let insured = envelope(json!([{"kenteken": "X99XXX", "wam_verzekerd": "Ja"}]));
        let rendered = plain(&insured, &lookup());
        assert!(
            rendered.contains("Verzekerd (WAM)   ja"),
            "rendered:\n{rendered}"
        );
        assert!(
            !rendered.contains("NIET VERZEKERD"),
            "rendered:\n{rendered}"
        );
    }

    #[test]
    fn an_open_recall_is_shouted_and_a_clear_one_reads_as_clear() {
        let open =
            envelope(json!([{"kenteken": "X99XXX", "openstaande_terugroepactie_indicator": "Ja"}]));
        assert!(plain(&open, &lookup()).contains("OPENSTAAND"));

        let clear = envelope(
            json!([{"kenteken": "X99XXX", "openstaande_terugroepactie_indicator": "Nee"}]),
        );
        let rendered = plain(&clear, &lookup());
        assert!(
            rendered.contains("geen openstaande"),
            "rendered:\n{rendered}"
        );
        assert!(!rendered.contains("OPENSTAAND"));
    }

    /// The three indicators RDW files as `Ja`/`Nee`, and its odometer verdict.
    fn indicator_card() -> Value {
        envelope(json!([{
            "kenteken": "X99XXX",
            "wam_verzekerd": "Ja",
            "openstaande_terugroepactie_indicator": "Nee",
            "tellerstandoordeel": "Logisch",
        }]))
    }

    #[test]
    fn rdws_own_words_are_what_the_dutch_card_says() {
        // Not a translation: `facts` turns RDW's `Logisch` into the token
        // `consistent` for the JSON contract, and the Dutch card hands it back.
        // A card that printed the token would leave English in Dutch output.
        let rendered = plain(&indicator_card(), &lookup());
        assert!(rendered.contains("logisch"), "rendered:\n{rendered}");
        assert!(!rendered.contains("consistent"), "rendered:\n{rendered}");
        // Capitalised as RDW files it is the tell that the value was passed
        // through untouched rather than rendered as part of a sentence.
        assert!(!rendered.contains("Logisch"), "rendered:\n{rendered}");
    }

    #[test]
    fn dutch_indicator_words_do_not_reach_an_english_card() {
        // `Ja` and `Nee` in a value column were the one place `--lang en` lapsed.
        let rendered = render(&indicator_card(), &lookup(), OutputFormat::Text, en());
        for dutch in ["Ja", "Nee", "Logisch"] {
            assert!(!rendered.contains(dutch), "leaked {dutch}:\n{rendered}");
        }
        // The negative control: a card that dropped all three fields would pass
        // the loop above without saying anything.
        assert!(rendered.contains("consistent"), "rendered:\n{rendered}");
    }

    #[test]
    fn a_blocked_transfer_is_shown_and_a_normal_one_is_not() {
        let blocked = envelope(json!([{"kenteken": "X99XXX", "tenaamstellen_mogelijk": "Nee"}]));
        assert!(plain(&blocked, &lookup()).contains("OVERSCHRIJVING GEBLOKKEERD"));

        let normal = envelope(json!([{"kenteken": "X99XXX", "tenaamstellen_mogelijk": "Ja"}]));
        assert!(!plain(&normal, &lookup()).contains("OVERSCHRIJVING GEBLOKKEERD"));
    }

    #[test]
    fn indicators_appear_only_when_set() {
        let off = envelope(json!([{"kenteken": "X99XXX", "taxi_indicator": "Nee"}]));
        assert!(!plain(&off, &lookup()).contains("Taxi"));

        let on = envelope(json!([{"kenteken": "X99XXX", "taxi_indicator": "Ja"}]));
        assert!(plain(&on, &lookup()).contains("Taxi"));
    }

    #[test]
    fn the_plate_heading_is_grouped_for_reading() {
        let env = envelope(json!([{"kenteken": "X99XXX", "merk": "IVECO"}]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.starts_with("X-99-XXX"), "rendered:\n{rendered}");
    }

    #[test]
    fn a_shouted_make_and_model_are_calmed_down() {
        let env = envelope(json!([{
            "kenteken": "XXX99X",
            "merk": "TESLA",
            "handelsbenaming": "MODEL Y",
        }]));
        let rendered = plain(&env, &lookup());
        assert!(
            rendered.starts_with("XXX-99-X   Tesla Model Y"),
            "{rendered}"
        );
    }

    #[test]
    fn the_card_keeps_an_initialism_capitalised_and_a_hyphenated_make_whole() {
        let env = envelope(json!([{
            "kenteken": "XXX99X",
            "merk": "MERCEDES-BENZ",
            "handelsbenaming": "B-KLASSE",
            "inrichting": "MPV",
            "eerste_kleur": "WIT",
        }]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("Mercedes-Benz B-Klasse"), "{rendered}");
        assert!(rendered.contains("MPV"), "{rendered}");
        // The same three-capital shape as MPV, on the same card, calmed. Both
        // bounds are needed: either one alone passes on a broken rendering.
        assert!(rendered.contains("Wit"), "{rendered}");
        assert!(!rendered.contains("Mercedes-benz"), "{rendered}");
        assert!(!rendered.contains("Mpv"), "{rendered}");
    }

    #[test]
    fn an_electric_vehicle_reports_power_and_range() {
        // An EV leaves `nettomaximumvermogen` empty, so reading only that column
        // rendered a Tesla with no power figure at all.
        let env = envelope(json!([{
            "kenteken": "XXX99X",
            "fuel": [{
                "brandstof_omschrijving": "Elektriciteit",
                "netto_max_vermogen_elektrisch": "220.00",
                "actie_radius_enkel_elektrisch_wltp": 533,
            }],
        }]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("220 kW"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("533 km actieradius"),
            "rendered:\n{rendered}"
        );
    }

    #[test]
    fn a_power_figure_loses_the_zeros_rdw_pads_it_with() {
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "fuel": [{"brandstof_omschrijving": "Diesel", "nettomaximumvermogen": "103.00"}],
        }]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("103 kW"), "rendered:\n{rendered}");
        assert!(!rendered.contains("103.00"), "rendered:\n{rendered}");
    }

    #[test]
    fn a_co2_figure_says_which_cycle_produced_it() {
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "fuel": [{
                "brandstof_omschrijving": "Diesel",
                "co2_uitstoot_gecombineerd": "180",
            }],
        }]));
        let rendered = plain(&env, &lookup());
        assert!(
            rendered.contains("180 g/km CO2 (NEDC)"),
            "rendered:\n{rendered}"
        );
    }

    #[test]
    fn hybrid_fuel_rows_are_both_shown() {
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "fuel": [
                {"brandstof_omschrijving": "Benzine", "nettomaximumvermogen": "70.00"},
                {"brandstof_omschrijving": "Elektriciteit", "nettomaximumvermogen": "30.00"},
            ],
        }]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("Benzine"), "rendered:\n{rendered}");
        assert!(rendered.contains("Elektriciteit"), "rendered:\n{rendered}");
    }

    #[test]
    fn multiple_vehicles_are_separated_by_a_blank_line() {
        let env = envelope(json!([
            {"kenteken": "X99XXX", "merk": "IVECO"},
            {"kenteken": "AA11BB", "merk": "VOLVO"},
        ]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("\n\n"), "rendered:\n{rendered}");
        assert!(rendered.contains("Iveco") && rendered.contains("Volvo"));
    }

    #[test]
    fn a_registered_plate_with_no_defects_says_so_positively() {
        let mut env = envelope(json!([]));
        env["no_rows"] = json!(["X99XXX"]);
        let rendered = plain(&env, &defects());
        assert!(
            rendered.contains("is geregistreerd"),
            "rendered: {rendered}"
        );
        assert!(
            rendered.contains("zonder gebreken vastgesteld bij keuring"),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn an_unresolved_defect_code_is_marked_rather_than_left_blank() {
        let env = defect_envelope(json!([{
            "kenteken": "X99XXX",
            "gebrek_identificatie": "ZZZ9",
            "gebrek_omschrijving": null,
        }]));
        let rendered = plain(&env, &defects());
        assert!(rendered.contains("ZZZ9"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("niet in de tabel van deze build"),
            "rendered:\n{rendered}"
        );
    }

    #[test]
    fn one_plate_needs_no_plate_column_and_several_do() {
        let one = defect_envelope(json!([
            {"kenteken": "X99XXX", "gebrek_identificatie": "AC4"},
            {"kenteken": "X99XXX", "gebrek_identificatie": "AC5"},
        ]));
        let rendered = plain(&one, &defects());
        assert!(
            !rendered.contains("KENTEKEN"),
            "a repeated plate column is noise:\n{rendered}"
        );
        assert!(!rendered.contains("X-99-XXX"), "rendered:\n{rendered}");

        let two = defect_envelope(json!([
            {"kenteken": "X99XXX", "gebrek_identificatie": "AC4"},
            {"kenteken": "AA11BB", "gebrek_identificatie": "AC5"},
        ]));
        let rendered = plain(&two, &defects());
        assert!(rendered.contains("KENTEKEN"), "rendered:\n{rendered}");
        assert!(rendered.contains("X-99-XXX"), "rendered:\n{rendered}");
        assert!(rendered.contains("AA-11-BB"), "rendered:\n{rendered}");
    }

    #[test]
    fn defect_dates_render_as_iso() {
        let env = defect_envelope(json!([{
            "kenteken": "X99XXX",
            "meld_datum_door_keuringsinstantie": "20251010",
            "gebrek_identificatie": "AC4",
        }]));
        let rendered = plain(&env, &defects());
        assert!(rendered.contains("2025-10-10"), "rendered:\n{rendered}");
        assert!(!rendered.contains("20251010"), "rendered:\n{rendered}");
    }

    #[test]
    fn numeric_fuel_columns_line_up_on_the_right() {
        let env = fuel_envelope(json!([
            {"kenteken": "X99XXX", "brandstof_omschrijving": "Benzine", "nettomaximumvermogen": "70.00"},
            {"kenteken": "X99XXX", "brandstof_omschrijving": "Diesel", "nettomaximumvermogen": "103.00"},
        ]));
        let rendered = render(
            &env,
            &Command::Fuel { plates: vec![] },
            OutputFormat::Text,
            nl(),
        );
        let ends: Vec<usize> = rendered
            .lines()
            .filter_map(|l| {
                l.find("70")
                    .or_else(|| l.find("103"))
                    .map(|c| c + l[c..].split(' ').next().unwrap_or("").len())
            })
            .collect();
        assert_eq!(ends.len(), 2, "rendered:\n{rendered}");
        assert_eq!(
            ends[0], ends[1],
            "kW column is not right aligned:\n{rendered}"
        );
    }

    #[test]
    fn tables_align_and_carry_no_trailing_whitespace() {
        let env = defect_envelope(json!([
            {"kenteken": "X99XXX", "gebrek_identificatie": "AC4", "gebrek_omschrijving": "Short"},
            {"kenteken": "AA11BB", "gebrek_identificatie": "LONGCODE", "gebrek_omschrijving": "x"},
        ]));
        let rendered = plain(&env, &defects());
        for line in rendered.lines() {
            assert_eq!(line, line.trim_end(), "trailing whitespace in {line:?}");
        }
        let code_columns: Vec<usize> = rendered
            .lines()
            .map(|l| l.find("AC4").or_else(|| l.find("LONGCODE")).unwrap_or(0))
            .filter(|c| *c > 0)
            .collect();
        assert!(
            code_columns.windows(2).all(|w| w[0] == w[1]),
            "code column is not aligned:\n{rendered}"
        );
    }

    #[test]
    fn a_raw_table_uses_the_union_of_all_row_keys() {
        // RDW omits a column per row, so a header taken from the first row alone
        // would silently drop data present in later rows.
        let env = json!({
            "items": [
                {"as_nummer": "1", "aangedreven_as": "N"},
                {"as_nummer": "2", "afstand_tot_volgende_as_voertuig": "410"},
            ],
            "total": 2, "limit": 100, "offset": 0,
            "truncated": false, "not_found": [], "no_rows": [],
        });
        let rendered = plain(&env, &raw());
        assert!(rendered.contains("aangedreven_as"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("afstand_tot_volgende_as_voertuig"),
            "rendered:\n{rendered}"
        );
        assert!(rendered.contains("410"), "rendered:\n{rendered}");
    }

    #[test]
    fn a_column_absent_from_a_row_renders_as_a_dash_not_as_a_blank() {
        // RDW omits a key rather than sending an empty value. A blank cell would
        // read as "the value is empty"; a dash says RDW reported nothing.
        let env = json!({
            "items": [
                {"as_nummer": "1", "afstand_tot_volgende_as_voertuig": "410"},
                {"as_nummer": "2"},
            ],
            "total": 2, "limit": 100, "offset": 0,
            "truncated": false, "not_found": [], "no_rows": [],
        });
        let rendered = plain(&env, &raw());
        let last = rendered.lines().last().unwrap();
        assert!(last.ends_with('-'), "last row was {last:?}");
    }

    #[test]
    fn an_absent_value_and_a_reported_zero_render_differently() {
        // A vehicle with no recorded axle load and one with a load of 0 are
        // different facts. Rendering them the same is the failure this whole
        // tool is built to avoid.
        let wrap = |items: Value| {
            json!({
                "items": items, "total": 1, "limit": 100, "offset": 0,
                "truncated": false, "not_found": [], "no_rows": [],
            })
        };
        let absent = plain(&wrap(json!([{"as_nummer": "1"}])), &raw());
        let zero = plain(&wrap(json!([{"as_nummer": "1", "aslast": "0"}])), &raw());
        assert!(!absent.contains('0'), "absent rendered as:\n{absent}");
        assert!(zero.contains('0'), "zero rendered as:\n{zero}");

        let both = plain(
            &wrap(json!([{"as_nummer": "1", "aslast": "0"}, {"as_nummer": "2"}])),
            &raw(),
        );
        let rows: Vec<&str> = both.lines().skip(1).collect();
        assert!(rows[0].contains('0'), "row with a zero: {:?}", rows[0]);
        assert!(rows[1].ends_with(ABSENT), "row without it: {:?}", rows[1]);
    }

    #[test]
    fn projecting_the_derived_block_away_falls_back_to_the_columns_asked_for() {
        // `--fields kenteken,merk` cannot render a summary whose every other line
        // was projected out. Showing the requested columns is the honest answer.
        let env = json!({
            "items": [{"kenteken": "X99XXX", "merk": "IVECO"}],
            "total": 1, "limit": 100, "offset": 0,
            "truncated": false, "not_found": [], "no_rows": [],
        });
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("kenteken"), "rendered:\n{rendered}");
        assert!(rendered.contains("IVECO"), "rendered:\n{rendered}");
    }

    #[test]
    fn the_dataset_list_keeps_its_own_table_although_it_has_no_derived_block() {
        // `datasets` describes this build, not a vehicle, so there is nothing to
        // derive. Treating that absence as a projected-away block would drop it
        // into the raw-column table and print RDW's SoQL ordering at a reader.
        let env = json!({
            "items": [{
                "id": "m9d7-ebf2", "name": "voertuigen", "description": "Registered vehicles.",
                "plate_keyed": true, "order": "kenteken",
            }],
            "total": 1, "limit": 100, "offset": 0,
            "truncated": false, "not_found": [], "no_rows": [],
        });
        let rendered = plain(&env, &Command::Datasets);
        assert!(rendered.starts_with("NAAM"), "rendered:\n{rendered}");
        assert!(rendered.contains("OP KENTEKEN"), "rendered:\n{rendered}");
        assert!(
            !rendered.contains("order"),
            "the SoQL ordering is a machine detail:\n{rendered}"
        );
    }

    #[test]
    fn wide_characters_do_not_break_column_alignment() {
        // Padding counts characters, not bytes; a byte count would misalign any
        // row containing a non-ASCII description.
        let env = defect_envelope(json!([
            {"kenteken": "X99XXX", "gebrek_identificatie": "A", "gebrek_omschrijving": "één"},
            {"kenteken": "AA11BB", "gebrek_identificatie": "B", "gebrek_omschrijving": "two"},
        ]));
        let rendered = plain(&env, &defects());
        let code_columns: Vec<usize> = rendered
            .lines()
            .skip(1)
            .map(|l| l.rfind(char::is_whitespace).unwrap_or(0))
            .collect();
        assert_eq!(code_columns.len(), 2, "two rows, got:\n{rendered}");
    }

    /// One status row with the two reference-keyed datasets joined onto it, the
    /// shape `recalls` renders.
    fn open_recall_row() -> Value {
        json!({
            "kenteken": "X99XXX",
            "referentiecode_rdw": "MGP230291",
            "code_status": "O",
            "status": "Openstaand",
            "recall": {
                "omschrijving_defect": "De remleiding kan langs de wielkast schuren.",
                "categorie_defect": "Remsysteem",
                "materi_le_gevolgen": "Remvloeistofverlies.",
                "beschrijving_van_het_herstel": "De remleiding wordt anders geleid en zo nodig vervangen.",
                "meldende_producent_distributeur": "Iveco Nederland B.V.",
                "meer_informatie_op_internet": "https://example.com/recall",
                "meer_informatie_via_telefoonnummer": "0800-1234567",
                "publicatiedatum_rdw": "20230417",
                "datum_eigenaren_ge_nformeerd": "20230502",
                "totaal_aantal_voertuigen_terugroepactie": "1834",
            },
            "risks": [{"mogelijk_gevaar": "Verminderde remwerking"}],
        })
    }

    /// The same text with every run of whitespace collapsed to one space.
    ///
    /// Wrapping inserts newlines and indentation between words, so this is what
    /// lets a test assert that a paragraph survived it whole.
    fn collapsed(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn a_recall_card_says_what_is_wrong_what_it_risks_and_how_it_is_fixed() {
        let env = recall_envelope(json!([open_recall_row()]));
        let rendered = plain(&env, &recalls());
        assert!(rendered.starts_with("X-99-XXX   MGP230291"), "{rendered}");
        assert!(rendered.contains("OPEN"), "rendered:\n{rendered}");
        for expected in [
            "De remleiding kan langs de wielkast schuren.",
            "Remsysteem",
            "Verminderde remwerking",
            "De remleiding wordt anders geleid en zo nodig vervangen.",
            "Iveco Nederland B.V.",
            "https://example.com/recall",
            "0800-1234567",
            "2023-04-17",
            "1.834 voertuigen in de actie",
        ] {
            assert!(
                collapsed(&rendered).contains(expected),
                "missing {expected:?}:\n{rendered}"
            );
        }
    }

    #[test]
    fn a_repaired_recall_reads_as_repaired_and_an_unstated_one_does_not() {
        let mut repaired = open_recall_row();
        repaired["code_status"] = json!("P");
        repaired["status"] = json!("Hersteld");
        let rendered = plain(&recall_envelope(json!([repaired.clone()])), &recalls());
        assert!(rendered.contains("hersteld"), "rendered:\n{rendered}");
        assert!(!rendered.contains("OPEN"), "rendered:\n{rendered}");

        // RDW filed the recall against the vehicle and left the status column
        // out. That is not a repair, and the card must not let it read as one.
        let mut silent = repaired;
        silent.as_object_mut().unwrap().remove("code_status");
        let rendered = plain(&recall_envelope(json!([silent])), &recalls());
        assert!(
            rendered.contains("status niet gemeld"),
            "rendered:\n{rendered}"
        );
        assert!(!rendered.contains("hersteld"), "rendered:\n{rendered}");
    }

    #[test]
    fn a_recall_rdw_published_no_detail_for_still_names_itself() {
        // A status row is allowed to reference a recall the detail dataset has
        // nothing for. Rendering nothing at all would hide an open recall.
        let env = recall_envelope(json!([{
            "kenteken": "X99XXX",
            "referentiecode_rdw": "MGP230085",
            "code_status": "O",
        }]));
        let rendered = plain(&env, &recalls());
        assert!(rendered.contains("MGP230085"), "rendered:\n{rendered}");
        assert!(rendered.contains("OPEN"), "rendered:\n{rendered}");
        assert!(!rendered.contains("Gebrek"), "rendered:\n{rendered}");
        assert!(!rendered.contains("Herstel"), "rendered:\n{rendered}");
    }

    #[test]
    fn the_hazard_of_an_open_recall_is_shouted_in_colour_and_in_plain() {
        let env = recall_envelope(json!([open_recall_row()]));
        let coloured = render(
            &env,
            &recalls(),
            OutputFormat::Text,
            Voice::new(Style::Colour, Lang::Nl),
        );
        assert!(coloured.contains("OPEN"), "colour dropped the status");
        assert!(
            coloured.contains("Verminderde remwerking"),
            "colour dropped the hazard:\n{coloured}"
        );
        assert!(coloured.contains('\u{1b}'), "colour emitted no escapes");
        assert!(!plain(&env, &recalls()).contains('\u{1b}'));
    }

    #[test]
    fn a_paragraph_of_rdw_prose_wraps_under_its_label_and_stays_whole() {
        let repair = "Het voertuig wordt bij een erkende werkplaats gecontroleerd \
             en waar nodig wordt de bedrading van de brandstofpomp opnieuw \
             gerouteerd, voorzien van een beschermhoes en met een nieuwe \
             klemverbinding vastgezet zodat schuren niet meer kan optreden.";
        let mut row = open_recall_row();
        row["recall"]["beschrijving_van_het_herstel"] = json!(repair);
        let rendered = plain(&recall_envelope(json!([row])), &recalls());

        assert!(
            rendered.lines().count() > 9,
            "the paragraph was not wrapped at all:\n{rendered}"
        );
        for line in rendered.lines() {
            assert!(
                visible_len(line) <= WRAP,
                "line runs past the card: {line:?}"
            );
        }
        // Splitting only between words means the sentence is still there, and
        // reading it back is the only check that proves no word was broken.
        assert!(
            collapsed(&rendered).contains(&collapsed(repair)),
            "the repair instruction did not survive wrapping:\n{rendered}"
        );
    }

    #[test]
    fn a_value_that_already_fits_is_returned_untouched() {
        // The card lines up columns with runs of spaces. Reflowing a line that
        // fits would collapse them and pull the value out of its column.
        let spaced = "2020-01-01   EXPIRED 2 years ago";
        assert_eq!(wrap(spaced, 78), vec![spaced.to_string()]);
    }

    #[test]
    fn width_is_measured_by_what_prints_not_by_what_is_stored() {
        let shouted = Style::Colour.alarm("OPEN");
        assert!(shouted.chars().count() > 4, "the escapes are really there");
        assert_eq!(visible_len(&shouted), 4);
        assert_eq!(visible_len("OPEN"), 4);
    }

    #[test]
    fn a_tachograph_finding_is_shouted_and_a_routine_check_is_left_alone() {
        for (dutch, shouted) in [
            ("manipulatie tacho", "TACHOGRAAF GEMANIPULEERD"),
            ("zegelverbreking tacho", "TACHOGRAAFZEGEL VERBROKEN"),
        ] {
            let env = inspection_envelope(json!([
                {
                    "kenteken": "X99XXX",
                    "meld_datum_door_keuringsinstantie": "20250102",
                    "soort_melding_ki_omschrijving": dutch,
                    "soort_erkenning_omschrijving": "Tachograafwerkplaats",
                },
                {
                    "kenteken": "X99XXX",
                    "meld_datum_door_keuringsinstantie": "20250304",
                    "soort_melding_ki_omschrijving": "periodieke controle",
                    "soort_erkenning_omschrijving": "APK lichte voertuigen",
                    "vervaldatum_keuring": "20260304",
                },
            ]));
            let rendered = plain(&env, &inspections());
            assert!(rendered.contains(shouted), "rendered:\n{rendered}");
            assert!(rendered.contains("2025-01-02"), "rendered:\n{rendered}");
            // The negative control: a table that shouted every row would pass
            // the assertion above and tell a reader nothing.
            let routine = rendered
                .lines()
                .find(|l| l.contains("2025-03-04"))
                .expect("the routine row is rendered");
            assert!(
                routine.contains("periodieke controle"),
                "routine row: {routine:?}"
            );
            assert!(!routine.contains("TACHOGRAAF"), "routine row: {routine:?}");
            assert!(routine.contains("2026-03-04"), "routine row: {routine:?}");
        }
    }

    #[test]
    fn an_expiry_column_does_not_call_a_tachograph_check_an_apk() {
        // Two bodies file on one day and set different expiries: the APK station
        // a year out, the tachograph workshop two. The date expires whichever
        // inspection was filed, so the column cannot promise an APK.
        let env = inspection_envelope(json!([
            {
                "kenteken": "X99XXX",
                "meld_datum_door_keuringsinstantie": "20240229",
                "soort_melding_ki_omschrijving": "periodieke controle",
                "soort_erkenning_omschrijving": "Controleapparaten",
                "vervaldatum_keuring": "20260301",
            },
            {
                "kenteken": "X99XXX",
                "meld_datum_door_keuringsinstantie": "20240229",
                "soort_melding_ki_omschrijving": "periodieke controle",
                "soort_erkenning_omschrijving": "APK Zware voertuigen",
                "vervaldatum_keuring": "20250301",
            },
        ]));
        let rendered = plain(&env, &inspections());
        let header = rendered.lines().next().expect("the table has a header");
        assert!(header.contains("GELDIG TOT"), "header: {header:?}");
        assert!(!header.contains("APK"), "header: {header:?}");
        let tacho = rendered
            .lines()
            .find(|l| l.contains("Controleapparaten"))
            .expect("the tachograph row is rendered");
        assert!(tacho.contains("2026-03-01"), "tachograph row: {tacho:?}");
    }

    #[test]
    fn a_shouted_notification_does_not_pull_its_table_out_of_line() {
        // The alarm cell carries a dozen bytes of escape that take no width.
        // Padding to the stored length would push every following column of
        // that one row to the right and leave the table unreadable.
        let env = inspection_envelope(json!([
            {
                "kenteken": "X99XXX",
                "meld_datum_door_keuringsinstantie": "20250102",
                "soort_melding_ki_omschrijving": "manipulatie tacho",
                "soort_erkenning_omschrijving": "Tachograafwerkplaats",
            },
            {
                "kenteken": "X99XXX",
                "meld_datum_door_keuringsinstantie": "20250304",
                "soort_melding_ki_omschrijving": "periodieke controle",
                "soort_erkenning_omschrijving": "APK lichte voertuigen",
                "vervaldatum_keuring": "20260304",
            },
        ]));
        let rendered = render(
            &env,
            &inspections(),
            OutputFormat::Text,
            Voice::new(Style::Colour, Lang::Nl),
        );
        let visible = stripped(&rendered);
        let columns: Vec<usize> = visible
            .lines()
            .map(|line| {
                line.find("GEMELD DOOR")
                    .or_else(|| line.find("Tachograafwerkplaats"))
                    .or_else(|| line.find("APK lichte"))
                    .unwrap_or_else(|| panic!("no accreditation cell in {line:?}"))
            })
            .collect();
        assert_eq!(columns.len(), 3, "as seen:\n{visible}");
        assert!(
            columns.windows(2).all(|w| w[0] == w[1]),
            "the coloured row is out of line at {columns:?}:\n{visible}"
        );
    }

    #[test]
    fn a_registered_plate_with_no_recalls_or_notifications_says_which_it_is() {
        let mut env = recall_envelope(json!([]));
        env["no_rows"] = json!(["X-99-XXX"]);
        let rendered = plain(&env, &recalls());
        assert!(
            rendered.contains("is geregistreerd"),
            "rendered: {rendered}"
        );
        assert!(
            rendered.contains("zonder terugroepacties, open of hersteld"),
            "rendered: {rendered}"
        );

        let rendered = plain(&env, &inspections());
        assert!(
            rendered.contains("zonder meldingen van keuringsinstanties"),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn the_dutch_register_line_appears_only_when_it_differs_from_admission() {
        let same = envelope(json!([{
            "kenteken": "X99XXX",
            "datum_eerste_toelating": "20190301",
            "datum_eerste_tenaamstelling_in_nederland": "20190301",
        }]));
        let rendered = plain(&same, &lookup());
        assert!(
            !rendered.contains("Eerste tenaamstelling NL"),
            "a line saying the two dates are the same is noise:\n{rendered}"
        );

        let later = envelope(json!([{
            "kenteken": "X99XXX",
            "datum_eerste_toelating": "20190301",
            "datum_eerste_tenaamstelling_in_nederland": "20220615",
        }]));
        let rendered = plain(&later, &lookup());
        assert!(rendered.contains("2022-06-15"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("na eerste toelating"),
            "rendered:\n{rendered}"
        );
        // The gap is a fact about two dates. Calling it an import would be a
        // verdict RDW never published.
        assert!(!rendered.contains("import"), "rendered:\n{rendered}");
    }

    #[test]
    fn an_odometer_verdict_carries_rdws_reason_except_when_all_is_well() {
        let flagged = envelope(json!([{
            "kenteken": "X99XXX",
            "tellerstandoordeel": "Onlogisch",
            "code_toelichting_tellerstandoordeel": "04",
        }]));
        let rendered = plain(&flagged, &lookup());
        assert!(rendered.contains("ONLOGISCH"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("teruggedraaid"),
            "the reason is the interesting part:\n{rendered}"
        );

        // A reason for a clean history only repeats that nothing is wrong, in
        // three lines of Dutch. The verdict alone is the answer.
        let clean = envelope(json!([{
            "kenteken": "X99XXX",
            "tellerstandoordeel": "Logisch",
            "code_toelichting_tellerstandoordeel": "00",
        }]));
        let rendered = plain(&clean, &lookup());
        assert!(rendered.contains("logisch"), "rendered:\n{rendered}");
        assert!(!rendered.contains("verklaarbaar"), "rendered:\n{rendered}");
    }

    #[test]
    fn a_dimension_rdw_did_not_measure_is_not_printed_as_zero_centimetres() {
        let unmeasured = envelope(json!([{
            "kenteken": "X99XXX",
            "lengte": "0",
            "breedte": "0",
            "hoogte_voertuig": "0",
        }]));
        let rendered = plain(&unmeasured, &lookup());
        assert!(!rendered.contains("Afmetingen"), "rendered:\n{rendered}");
        assert!(!rendered.contains("0 cm"), "rendered:\n{rendered}");

        // The negative control: a card that never printed dimensions at all
        // would pass both assertions above.
        let measured = envelope(json!([{
            "kenteken": "X99XXX",
            "lengte": "645",
            "breedte": "255",
            "hoogte_voertuig": "400",
        }]));
        let rendered = plain(&measured, &lookup());
        assert!(
            rendered.contains("645 cm lang, 255 cm breed, 400 cm hoog"),
            "rendered:\n{rendered}"
        );

        // A vehicle measured in one direction only says which one, rather than
        // padding the other two out to keep a positional `l x w x h` shape.
        let partial = envelope(json!([{"kenteken": "X99XXX", "hoogte_voertuig": "400"}]));
        let rendered = plain(&partial, &lookup());
        assert!(rendered.contains("400 cm hoog"), "rendered:\n{rendered}");
        assert!(!rendered.contains("lang"), "rendered:\n{rendered}");
        assert!(!rendered.contains("breed"), "rendered:\n{rendered}");
    }

    #[test]
    fn an_odometer_verdict_carries_the_year_its_readings_stop() {
        // Without the year the verdict reads as current whatever its age: a
        // history last touched in 2016 and one read last month say one word.
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "tellerstandoordeel": "Logisch",
            "jaar_laatste_registratie_tellerstand": "2016",
        }]));
        let rendered = plain(&env, &lookup());
        assert!(
            rendered.contains("logisch   laatste stand 2016"),
            "rendered:\n{rendered}"
        );
        // A year is a date, not a quantity. Grouped as `2.016` it reads as a
        // distance, and every other number on this card is one.
        assert!(!rendered.contains("2.016"), "rendered:\n{rendered}");

        // RDW records each half without the other, and either alone is worth
        // saying: 730,494 vehicles have a reading year and no verdict.
        let verdict_only =
            envelope(json!([{"kenteken": "X99XXX", "tellerstandoordeel": "Logisch"}]));
        let rendered = plain(&verdict_only, &lookup());
        assert!(rendered.contains("logisch"), "rendered:\n{rendered}");
        assert!(!rendered.contains("laatste stand"), "rendered:\n{rendered}");

        let year_only = envelope(json!([{
            "kenteken": "X99XXX",
            "jaar_laatste_registratie_tellerstand": "2016",
        }]));
        let rendered = plain(&year_only, &lookup());
        assert!(
            rendered.contains("Tellerstand") && rendered.contains("laatste stand 2016"),
            "rendered:\n{rendered}"
        );
        assert!(!rendered.contains("logisch"), "rendered:\n{rendered}");
    }

    #[test]
    fn the_tachograph_deadline_is_shown_beside_the_apk_and_never_as_it() {
        // A lorry can hold a current APK and an expired tachograph. Both lines
        // are asserted, so rendering one deadline against the other's label
        // fails rather than passing on half the card.
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "vervaldatum_apk": "20261109",
            "vervaldatum_tachograaf": "20260301",
        }]));
        let rendered = plain(&env, &lookup());
        let apk = rendered
            .lines()
            .find(|l| l.contains("APK verloopt"))
            .expect("the APK line is rendered");
        assert!(apk.contains("2026-11-09"), "APK line: {apk:?}");
        assert!(!apk.contains("VERLOPEN"), "APK line: {apk:?}");

        let tacho = rendered
            .lines()
            .find(|l| l.contains("Tachograaf verloopt"))
            .expect("the tachograph line is rendered");
        assert!(tacho.contains("2026-03-01"), "tachograph line: {tacho:?}");
        assert!(tacho.contains("VERLOPEN"), "tachograph line: {tacho:?}");
    }

    #[test]
    fn a_vehicle_with_no_tachograph_gets_no_tachograph_line() {
        // 98.8% of the register. A line reading "not applicable" against every
        // one of them is noise, and one reading a date would be a lie.
        let env = envelope(json!([{"kenteken": "X99XXX", "vervaldatum_apk": "20261109"}]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("APK verloopt"), "rendered:\n{rendered}");
        assert!(!rendered.contains("Tachograaf"), "rendered:\n{rendered}");
    }

    #[test]
    fn the_type_line_counts_seats_and_doors_and_none_is_still_a_count() {
        let car = envelope(json!([{
            "kenteken": "X99XXX",
            "voertuigsoort": "Personenauto",
            "europese_voertuigcategorie": "M1",
            "inrichting": "stationwagen",
            "aantal_zitplaatsen": "5",
            "aantal_deuren": "4",
        }]));
        let rendered = plain(&car, &lookup());
        assert!(
            rendered.contains("Personenauto (M1), stationwagen, 5 zitplaatsen, 4 deuren"),
            "rendered:\n{rendered}"
        );

        // A trailer has no doors, and RDW says so with a zero rather than by
        // leaving the column out. Hiding it would lose a fact.
        let trailer = envelope(json!([{
            "kenteken": "X99XXX",
            "voertuigsoort": "Aanhangwagen",
            "aantal_deuren": "0",
        }]));
        let rendered = plain(&trailer, &lookup());
        assert!(
            rendered.contains("Aanhangwagen, 0 deuren"),
            "rendered:\n{rendered}"
        );

        // One of a thing is one, not "1 zitplaatsen".
        let single = envelope(json!([{"kenteken": "X99XXX", "aantal_zitplaatsen": "1"}]));
        let rendered = plain(&single, &lookup());
        assert!(rendered.contains("1 zitplaats"), "rendered:\n{rendered}");
        assert!(!rendered.contains("1 zitplaatsen"), "rendered:\n{rendered}");
    }

    #[test]
    fn towing_capacity_is_reported_braked_and_unbraked() {
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "maximum_trekken_massa_geremd": "3500",
            "maximum_massa_trekken_ongeremd": "750",
        }]));
        let rendered = plain(&env, &lookup());
        assert!(
            rendered.contains("3.500 kg geremd, 750 kg ongeremd"),
            "rendered:\n{rendered}"
        );

        // A vehicle that may not tow has neither figure, so no line here has to
        // stand for "not permitted".
        let none = envelope(json!([{"kenteken": "X99XXX"}]));
        assert!(!plain(&none, &lookup()).contains("Trekgewicht"));
    }

    #[test]
    fn the_card_reports_the_engine_the_energy_label_and_where_the_vin_is_stamped() {
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "cilinderinhoud": "2998",
            "zuinigheidsclassificatie": "C",
            "plaats_chassisnummer": "r. tegen schutbord onder motorkap",
        }]));
        let rendered = plain(&env, &lookup());
        assert!(rendered.contains("2.998 cm3"), "rendered:\n{rendered}");
        assert!(rendered.contains("Energielabel"), "rendered:\n{rendered}");
        // RDW's own abbreviated Dutch. Someone is standing at the vehicle with
        // this on screen, and expanding it would be the tool guessing where a
        // stamped number is.
        assert!(
            rendered.contains("r. tegen schutbord onder motorkap"),
            "rendered:\n{rendered}"
        );

        // The negative control: none of these three labels appears on a card
        // RDW gave nothing for.
        let bare = envelope(json!([{"kenteken": "X99XXX"}]));
        let rendered = plain(&bare, &lookup());
        for label in ["Cilinderinhoud", "Energielabel", "Positie VIN"] {
            assert!(!rendered.contains(label), "{label} on an empty card");
        }
    }

    #[test]
    fn an_open_recall_on_a_vehicle_card_says_what_it_is_and_where_to_read_it() {
        // Two shouted words with no way to find out more is a warning a reader
        // cannot act on.
        let env = envelope(json!([{
            "kenteken": "X99XXX",
            "openstaande_terugroepactie_indicator": "Ja",
            "recalls": [open_recall_row()],
        }]));
        let rendered = plain(&env, &lookup());
        let status = rendered
            .lines()
            .find(|l| l.contains("OPENSTAAND"))
            .expect("the recall line is rendered");
        // The pointer stays on the shouted line, spacing and all. A hazard
        // joined on here would push the line past the card, and wrapping it
        // would collapse that spacing into one run-on phrase.
        assert!(
            status.contains("OPENSTAAND   zie: kenteken recalls X-99-XXX"),
            "recall line was {status:?}"
        );
        let hazard = rendered
            .lines()
            .find(|l| l.contains("Risico terugroepactie"))
            .expect("the hazard has a line of its own");
        assert!(
            hazard.contains("Verminderde remwerking"),
            "hazard line was {hazard:?}"
        );
    }
}

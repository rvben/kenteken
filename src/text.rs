//! Every word the rendered text says, in both languages it says them in.
//!
//! The card is a Dutch document about a Dutch vehicle from a Dutch registry, so
//! Dutch is what it speaks by default and `--lang en` opts out. What a developer
//! or an agent touches stays English in both: JSON keys, the `derived` block,
//! `schema`, `--help` and error kinds are the contract, and a contract that
//! changes language with a flag is not one.
//!
//! # Why a struct rather than a lookup table
//!
//! [`Phrase`] has one field per language and no default. A phrase written in one
//! language and forgotten in the other does not compile. That matters more than
//! it looks: the failure mode that makes localisation rot is not a wrong
//! translation, it is a card that is 95% Dutch with three English words left in
//! it, which is exactly the incoherence this module exists to remove. Here it
//! cannot be reached by forgetting. It has to be chosen.
//!
//! # What is not translated
//!
//! RDW's own values pass through untouched: colours, body types, fuel names, VIN
//! locations, and the odometer explanation. They are data, not chrome. The
//! explanation in particular is RDW's official statement about a vehicle's
//! history, and paraphrasing it would put words in RDW's mouth about something
//! with legal weight.
//!
//! Dutch is also where those values already are, which is what makes this
//! module small. Rendering the card in Dutch translates only the words we wrote;
//! rendering it fully in English would mean owning a translation of RDW's
//! vocabulary, 95 body types and all, that would drift from the register.

/// Which language the rendered text speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// Dutch, the language of the register the data comes from.
    #[default]
    Nl,
    /// English.
    En,
}

/// One phrase, in every language the tool renders.
///
/// Both fields are required and neither has a default, which is the whole point:
/// see the module documentation. The fields stay private, so a phrase can only be
/// read through [`Lang::say`] and the language is always chosen deliberately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phrase {
    en: &'static str,
    nl: &'static str,
}

/// Write one phrase. Both languages are positional, so neither can be omitted.
///
/// Visible crate-wide because a phrase belongs beside the thing it describes when
/// that thing is a table of data: `rdw::datasets` writes its own descriptions, so
/// adding a dataset stays one edit in one file. Nothing is lost by that, since the
/// guarantee lives in the type rather than in this module.
pub(crate) const fn p(en: &'static str, nl: &'static str) -> Phrase {
    Phrase { en, nl }
}

/// A noun that is counted, which pluralises by its own rules in each language.
///
/// Dutch does not pluralise a measure of time when it is counted: `13 jaar
/// geleden`, never `13 jaren geleden`. English always does. So the plural is
/// spelled out per language rather than derived by appending an `s`, which is a
/// rule that only holds in one of the two.
pub struct Noun {
    en: (&'static str, &'static str),
    nl: (&'static str, &'static str),
}

/// Write one counted noun, as singular and plural in each language.
const fn n(
    en_one: &'static str,
    en_many: &'static str,
    nl_one: &'static str,
    nl_many: &'static str,
) -> Noun {
    Noun {
        en: (en_one, en_many),
        nl: (nl_one, nl_many),
    }
}

impl Lang {
    /// This phrase, in this language.
    pub fn say(self, phrase: &Phrase) -> &'static str {
        match self {
            Lang::En => phrase.en,
            Lang::Nl => phrase.nl,
        }
    }

    /// A phrase with its one placeholder filled in.
    ///
    /// Word order differs between the two languages often enough that a phrase
    /// cannot always be assembled by concatenation: `in 5 months` puts the
    /// preposition first and `5 maanden geleden` puts its marker last. Holding
    /// the whole sentence in one string keeps the two orders side by side where
    /// they can be compared, and [`every_template_has_its_placeholder`] fails if
    /// one of them loses its slot.
    pub fn fill(self, phrase: &Phrase, value: &str) -> String {
        self.fill_all(phrase, &[value])
    }

    /// A phrase with its placeholders filled in, left to right.
    ///
    /// Positional rather than repeated: a note that names three numbers needs
    /// three different values, and substituting one value into every slot would
    /// print the first row count three times. A slot with no value left keeps its
    /// literal `{}` rather than closing up, so a wrong count renders as visibly
    /// broken instead of as a shorter sentence that reads complete.
    pub fn fill_all(self, phrase: &Phrase, values: &[&str]) -> String {
        let template = self.say(phrase);
        debug_assert_eq!(
            template.matches("{}").count(),
            values.len(),
            "{template:?} takes a different number of values than it was given"
        );
        let mut out = String::with_capacity(template.len());
        let mut values = values.iter();
        let mut rest = template;
        while let Some((before, after)) = rest.split_once("{}") {
            out.push_str(before);
            match values.next() {
                Some(value) => out.push_str(value),
                None => out.push_str("{}"),
            }
            rest = after;
        }
        out.push_str(rest);
        out
    }

    /// A counted noun: `1 day`, `30 days`, `13 jaar`, `5 zitplaatsen`.
    pub fn count(self, count: i64, noun: &Noun) -> String {
        let (one, many) = match self {
            Lang::En => noun.en,
            Lang::Nl => noun.nl,
        };
        let word = if count == 1 { one } else { many };
        format!("{} {word}", self.thousands(count))
    }

    /// The separator that groups thousands: `1,938` in English, `1.938` in Dutch.
    ///
    /// Not decoration. A Dutch reader parses `1,938` as a number just under two,
    /// so a card that says `1,938 kg leeg` is not merely inelegant, it is
    /// briefly wrong. Localisation here is arithmetic, not vocabulary.
    pub fn thousands(self, value: i64) -> String {
        let separator = match self {
            Lang::En => ',',
            Lang::Nl => '.',
        };
        let digits = value.unsigned_abs().to_string();
        let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
        if value < 0 {
            out.push('-');
        }
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(separator);
            }
            out.push(c);
        }
        out
    }

    /// A measurement without RDW's trailing zeros: `100.00` becomes `100`.
    ///
    /// The decimal mark swaps with the grouping mark, so a Dutch card reads
    /// `103,5` where an English one reads `103.5`.
    pub fn measure(self, value: f64) -> String {
        if value.fract() == 0.0 {
            return self.thousands(value as i64);
        }
        let rendered = format!("{value}");
        match self {
            Lang::En => rendered,
            Lang::Nl => rendered.replace('.', ","),
        }
    }

    /// A day count as a phrase with a direction: `in 5 months`, `3 dagen geleden`.
    ///
    /// Deliberately coarse. The exact date is already printed next to it; this
    /// answers "is this a problem" at a glance.
    pub fn offset(self, days: i64) -> String {
        if days == 0 {
            return self.say(&TODAY).to_string();
        }
        let span = self.span(days);
        match days > 0 {
            true => self.fill(&FUTURE, &span),
            false => self.fill(&PAST, &span),
        }
    }

    /// A day count as a bare phrase with no direction: `1 year 4 months`.
    ///
    /// The same coarse phrasing as [`Lang::offset`], for the distance between two
    /// dates rather than from today.
    pub fn span(self, days: i64) -> String {
        let magnitude = days.unsigned_abs() as i64;
        if magnitude < 45 {
            return self.count(magnitude, &DAY);
        }
        if magnitude < 365 {
            // The last fortnight of the year rounds up to a full twelve months,
            // so it is spelled as the year it has all but reached.
            return match (magnitude as f64 / 30.44).round() as i64 {
                12 => self.count(1, &YEAR),
                months => self.count(months, &MONTH),
            };
        }
        let years = magnitude / 365;
        let months = ((magnitude % 365) as f64 / 30.44).round() as i64;
        match months {
            0 => self.count(years, &YEAR),
            // Rounding can push the remainder to a full year; carry it rather
            // than printing "1 year 12 months".
            12 => self.count(years + 1, &YEAR),
            m => format!("{} {}", self.count(years, &YEAR), self.count(m, &MONTH)),
        }
    }
}

// Counted nouns.

const DAY: Noun = n("day", "days", "dag", "dagen");
const MONTH: Noun = n("month", "months", "maand", "maanden");
const YEAR: Noun = n("year", "years", "jaar", "jaar");
pub const SEAT: Noun = n("seat", "seats", "zitplaats", "zitplaatsen");
pub const DOOR: Noun = n("door", "doors", "deur", "deuren");
pub const VEHICLE: Noun = n("vehicle", "vehicles", "voertuig", "voertuigen");

// Relative time.

const TODAY: Phrase = p("today", "vandaag");
const FUTURE: Phrase = p("in {}", "over {}");
const PAST: Phrase = p("{} ago", "{} geleden");

// Labels on the vehicle card. The Dutch side uses RDW's own field names where
// they differ from plain Dutch, because those are the words printed on the
// kentekenbewijs the reader can hold next to the screen.

pub const APK_EXPIRES: Phrase = p("APK expires", "APK verloopt");
pub const TACHOGRAPH_EXPIRES: Phrase = p("Tachograph expires", "Tachograaf verloopt");
pub const ODOMETER: Phrase = p("Odometer", "Tellerstand");
pub const ODOMETER_NOTE: Phrase = p("Odometer note", "Toelichting tellerstand");
pub const INSURED: Phrase = p("Insured (WAM)", "Verzekerd (WAM)");
pub const RECALL: Phrase = p("Recall", "Terugroepactie");
pub const RECALL_HAZARD: Phrase = p("Recall hazard", "Risico terugroepactie");
pub const EXPORTED: Phrase = p("Exported", "Geëxporteerd");
pub const TAXI: Phrase = p("Taxi", "Taxi");
pub const REGISTRATION: Phrase = p("Registration", "Tenaamstelling");
pub const FIRST_ADMITTED: Phrase = p("First admitted", "Eerste toelating");
pub const DUTCH_REGISTER: Phrase = p("On the Dutch register", "Eerste tenaamstelling NL");
pub const REGISTERED_SINCE: Phrase = p("Registered since", "Tenaamstelling sinds");
pub const FUEL: Phrase = p("Fuel", "Brandstof");
pub const ENGINE: Phrase = p("Engine", "Cilinderinhoud");
pub const ENERGY_LABEL: Phrase = p("Energy label", "Energielabel");
pub const MASS: Phrase = p("Mass", "Massa");
pub const TOWING: Phrase = p("Towing", "Trekgewicht");
pub const DIMENSIONS: Phrase = p("Dimensions", "Afmetingen");
pub const CATALOGUE_PRICE: Phrase = p("Catalogue price", "Catalogusprijs");
pub const VIN_LOCATION: Phrase = p("VIN location", "Positie VIN");

// Values on the vehicle card.

pub const YES: Phrase = p("yes", "ja");
pub const NO: Phrase = p("no", "nee");
pub const EXPIRED: Phrase = p("EXPIRED {}", "VERLOPEN {}");
pub const NOT_INSURED: Phrase = p("NOT INSURED", "NIET VERZEKERD");
pub const TRANSFER_BLOCKED: Phrase = p("TRANSFER BLOCKED", "OVERSCHRIJVING GEBLOKKEERD");
// RDW files this status as `Openstaand`, and the label beside it already says
// what is outstanding. English needs the noun because `OPEN` alone beside
// `Recall` reads as a heading rather than a warning.
pub const OPEN_RECALL: Phrase = p("OPEN RECALL", "OPENSTAAND");
pub const NONE_OUTSTANDING: Phrase = p("none outstanding", "geen openstaande");
pub const SEE_RECALLS: Phrase = p("see: kenteken recalls {}", "zie: kenteken recalls {}");
pub const LAST_READING: Phrase = p("last reading {}", "laatste stand {}");
pub const MASS_EMPTY: Phrase = p("empty", "leeg");
pub const MASS_MAX: Phrase = p("max", "max");
pub const TOW_BRAKED: Phrase = p("braked", "geremd");
pub const TOW_UNBRAKED: Phrase = p("unbraked", "ongeremd");
pub const LONG: Phrase = p("long", "lang");
pub const WIDE: Phrase = p("wide", "breed");
pub const HIGH: Phrase = p("high", "hoog");
pub const RANGE: Phrase = p("range", "actieradius");
pub const LAG_AFTER: Phrase = p("{} after first admission", "{} na eerste toelating");
pub const LAG_BEFORE: Phrase = p("{} before first admission", "{} voor eerste toelating");

// RDW's odometer verdict. The Dutch side is the register's own wording, which
// `facts` translated into an English token for the JSON contract on the way in.

pub const ODOMETER_CONSISTENT: Phrase = p("consistent", "logisch");
pub const ODOMETER_INCONSISTENT: Phrase = p("INCONSISTENT", "ONLOGISCH");
pub const ODOMETER_NO_JUDGEMENT: Phrase = p("no judgement", "geen oordeel");

// Labels on the recall card.

pub const DEFECT: Phrase = p("Defect", "Gebrek");
pub const CATEGORY: Phrase = p("Category", "Categorie");
pub const HAZARD: Phrase = p("Hazard", "Risico");
pub const CONSEQUENCES: Phrase = p("Consequences", "Gevolgen");
pub const REPAIR: Phrase = p("Repair", "Herstel");
pub const REPORTED_BY: Phrase = p("Reported by", "Gemeld door");
pub const MORE_INFORMATION: Phrase = p("More information", "Meer informatie");
pub const PUBLISHED: Phrase = p("Published", "Gepubliceerd");
pub const OWNERS_INFORMED: Phrase = p("Owners informed", "Eigenaren geïnformeerd");
pub const RECALL_OPEN: Phrase = p("OPEN", "OPENSTAAND");
pub const RECALL_REPAIRED: Phrase = p("repaired", "hersteld");
pub const RECALL_NO_STATUS: Phrase = p("status not reported", "status niet gemeld");
pub const IN_THE_ACTION: Phrase = p("{} in the action", "{} in de actie");

// Table headings, shouted in both languages because that is how the tables read.

pub const COL_PLATE: Phrase = p("PLATE", "KENTEKEN");
pub const COL_DATE: Phrase = p("DATE", "DATUM");
pub const COL_CODE: Phrase = p("CODE", "CODE");
pub const COL_DEFECT: Phrase = p("DEFECT", "GEBREK");
pub const COL_FUEL: Phrase = p("FUEL", "BRANDSTOF");
pub const COL_KW: Phrase = p("KW", "KW");
pub const COL_CO2: Phrase = p("CO2 G/KM", "CO2 G/KM");
pub const COL_BASIS: Phrase = p("BASIS", "BASIS");
pub const COL_RANGE: Phrase = p("RANGE KM", "ACTIERADIUS KM");
pub const COL_EURO: Phrase = p("EURO", "EURO");
pub const COL_NOTIFICATION: Phrase = p("NOTIFICATION", "MELDING");
pub const COL_FILED_BY: Phrase = p("FILED BY", "GEMELD DOOR");
pub const COL_VALID_UNTIL: Phrase = p("VALID UNTIL", "GELDIG TOT");
pub const COL_NAME: Phrase = p("NAME", "NAAM");
pub const COL_ID: Phrase = p("ID", "ID");
pub const COL_BY_PLATE: Phrase = p("BY PLATE", "OP KENTEKEN");
pub const COL_CONTENTS: Phrase = p("CONTENTS", "INHOUD");

// Table cells.

pub const UNKNOWN_CODE: Phrase = p(
    "(code not in this build's table)",
    "(code niet in de tabel van deze build)",
);
pub const TACHOGRAPH_TAMPERING: Phrase = p("TACHOGRAPH TAMPERING", "TACHOGRAAF GEMANIPULEERD");
pub const TACHOGRAPH_SEAL_BROKEN: Phrase = p("TACHOGRAPH SEAL BROKEN", "TACHOGRAAFZEGEL VERBROKEN");

// What to say when there is nothing to show but nothing went wrong either.

pub const NO_ROWS: Phrase = p("no rows", "geen rijen");
pub const NO_DEFECTS: Phrase = p(
    "{} is registered, with no defects recorded at inspection",
    "{} is geregistreerd, zonder gebreken vastgesteld bij keuring",
);
pub const NO_FUEL: Phrase = p(
    "{} is registered, with no fuel rows recorded",
    "{} is geregistreerd, zonder brandstofgegevens",
);
pub const NO_RECALLS: Phrase = p(
    "{} is registered, with no recalls on record, open or repaired",
    "{} is geregistreerd, zonder terugroepacties, open of hersteld",
);
pub const NO_INSPECTIONS: Phrase = p(
    "{} is registered, with no notifications from inspection bodies",
    "{} is geregistreerd, zonder meldingen van keuringsinstanties",
);
pub const NO_DATASET_ROWS: Phrase = p(
    "{} is registered, with no rows in this dataset",
    "{} is geregistreerd, zonder rijen in deze dataset",
);

// What stderr says beside the card.
//
// These are prose for whoever is reading the card, so they follow `--lang` like
// the card does. The severity marker is part of the phrase rather than a literal
// prefix: a Dutch line that opens with an English `warning:` is exactly the
// half-translated output this module exists to prevent. The machine surfaces on
// stderr are English in both languages and are not phrases at all: the error
// envelope and the NDJSON metadata line are JSON a consumer parses.
//
// The flag names stay literal, because they are what the reader types back.

pub const WARNING_NOT_REGISTERED: Phrase = p(
    "warning: not registered with RDW: {}",
    "waarschuwing: niet geregistreerd bij de RDW: {}",
);
pub const NOTE_NO_ROWS_IN_DATASET: Phrase = p(
    "note: registered, but no rows in this dataset: {}",
    "let op: geregistreerd, maar geen rijen in deze dataset: {}",
);
pub const NOTE_SHOWING_ROWS: Phrase = p(
    "note: showing rows {}-{} of {}; raise --limit or page with --offset",
    "let op: toont rijen {}-{} van {}; verhoog --limit of blader met --offset",
);

#[cfg(test)]
mod tests {
    use super::*;

    /// Every phrase in the catalogue, so a test can sweep all of them.
    ///
    /// Listed by hand, which is the cost of the compile-time guarantee: a phrase
    /// missing from here is still complete in both languages, it is merely
    /// unswept. The guarantee that matters, that neither language can be
    /// omitted, is enforced by the type and not by this list.
    ///
    /// This is the catalogue in this module. The phrases written beside their own
    /// data, the dataset descriptions in [`crate::rdw::datasets`], are swept by a
    /// test there over the registry itself.
    const ALL: &[&Phrase] = &[
        &TODAY,
        &FUTURE,
        &PAST,
        &APK_EXPIRES,
        &TACHOGRAPH_EXPIRES,
        &ODOMETER,
        &ODOMETER_NOTE,
        &INSURED,
        &RECALL,
        &RECALL_HAZARD,
        &EXPORTED,
        &TAXI,
        &REGISTRATION,
        &FIRST_ADMITTED,
        &DUTCH_REGISTER,
        &REGISTERED_SINCE,
        &FUEL,
        &ENGINE,
        &ENERGY_LABEL,
        &MASS,
        &TOWING,
        &DIMENSIONS,
        &CATALOGUE_PRICE,
        &VIN_LOCATION,
        &YES,
        &NO,
        &EXPIRED,
        &NOT_INSURED,
        &TRANSFER_BLOCKED,
        &OPEN_RECALL,
        &NONE_OUTSTANDING,
        &SEE_RECALLS,
        &LAST_READING,
        &MASS_EMPTY,
        &MASS_MAX,
        &TOW_BRAKED,
        &TOW_UNBRAKED,
        &LONG,
        &WIDE,
        &HIGH,
        &RANGE,
        &LAG_AFTER,
        &LAG_BEFORE,
        &ODOMETER_CONSISTENT,
        &ODOMETER_INCONSISTENT,
        &ODOMETER_NO_JUDGEMENT,
        &DEFECT,
        &CATEGORY,
        &HAZARD,
        &CONSEQUENCES,
        &REPAIR,
        &REPORTED_BY,
        &MORE_INFORMATION,
        &PUBLISHED,
        &OWNERS_INFORMED,
        &RECALL_OPEN,
        &RECALL_REPAIRED,
        &RECALL_NO_STATUS,
        &IN_THE_ACTION,
        &COL_PLATE,
        &COL_DATE,
        &COL_CODE,
        &COL_DEFECT,
        &COL_FUEL,
        &COL_KW,
        &COL_CO2,
        &COL_BASIS,
        &COL_RANGE,
        &COL_EURO,
        &COL_NOTIFICATION,
        &COL_FILED_BY,
        &COL_VALID_UNTIL,
        &COL_NAME,
        &COL_ID,
        &COL_BY_PLATE,
        &COL_CONTENTS,
        &UNKNOWN_CODE,
        &TACHOGRAPH_TAMPERING,
        &TACHOGRAPH_SEAL_BROKEN,
        &NO_ROWS,
        &NO_DEFECTS,
        &NO_FUEL,
        &NO_RECALLS,
        &NO_INSPECTIONS,
        &NO_DATASET_ROWS,
        &WARNING_NOT_REGISTERED,
        &NOTE_NO_ROWS_IN_DATASET,
        &NOTE_SHOWING_ROWS,
    ];

    #[test]
    fn no_phrase_is_left_empty_in_either_language() {
        // The type makes an omitted language impossible; an empty string is the
        // one way left to write a phrase that says nothing, and it would render
        // as a missing word rather than as a compile error.
        for phrase in ALL {
            assert!(
                !phrase.en.trim().is_empty(),
                "empty en beside {}",
                phrase.nl
            );
            assert!(
                !phrase.nl.trim().is_empty(),
                "empty nl beside {}",
                phrase.en
            );
        }
    }

    #[test]
    fn every_template_has_its_placeholder() {
        // A template that loses its slot in one language silently drops the
        // value it was carrying: `VERLOPEN 8 maanden geleden` becomes a bare
        // `VERLOPEN`, which reads as a complete answer and is not one.
        //
        // Counted rather than merely present, because a phrase carrying three
        // values has three ways to lose one, and every one of them satisfies
        // "both languages contain a slot".
        for phrase in ALL {
            assert_eq!(
                phrase.en.matches("{}").count(),
                phrase.nl.matches("{}").count(),
                "the two languages take different numbers of values: {:?} / {:?}",
                phrase.en,
                phrase.nl
            );
        }
    }

    #[test]
    fn placeholders_are_filled_in_order_and_not_repeated() {
        // The page note names three different numbers. `replace` would print the
        // first of them three times, which is a wrong answer that reads like a
        // right one.
        assert_eq!(
            Lang::Nl.fill_all(&NOTE_SHOWING_ROWS, &["1", "3", "11"]),
            "let op: toont rijen 1-3 van 11; verhoog --limit of blader met --offset"
        );
        assert_eq!(
            Lang::En.fill_all(&NOTE_SHOWING_ROWS, &["1", "3", "11"]),
            "note: showing rows 1-3 of 11; raise --limit or page with --offset"
        );

        // One value through the one-value door is the same string either way.
        assert_eq!(Lang::Nl.fill(&PAST, "3 dagen"), "3 dagen geleden");
        assert_eq!(
            Lang::Nl.fill_all(&PAST, &["3 dagen"]),
            Lang::Nl.fill(&PAST, "3 dagen")
        );
    }

    #[test]
    fn dutch_groups_thousands_with_a_full_stop_and_english_with_a_comma() {
        assert_eq!(Lang::En.thousands(1_938), "1,938");
        assert_eq!(Lang::Nl.thousands(1_938), "1.938");
        assert_eq!(Lang::En.thousands(1_234_567), "1,234,567");
        assert_eq!(Lang::Nl.thousands(1_234_567), "1.234.567");

        assert_eq!(Lang::En.thousands(91_144), "91,144");
        assert_eq!(Lang::Nl.thousands(91_144), "91.144");

        // Below the grouping threshold the two agree, which is the control that
        // keeps the assertions above about the separator rather than about the
        // digits.
        assert_eq!(Lang::En.thousands(0), "0");
        assert_eq!(Lang::Nl.thousands(0), "0");
        assert_eq!(Lang::En.thousands(999), "999");
        assert_eq!(Lang::Nl.thousands(999), "999");

        assert_eq!(Lang::En.thousands(-2_059), "-2,059");
        assert_eq!(Lang::Nl.thousands(-2_059), "-2.059");
    }

    #[test]
    fn the_decimal_mark_swaps_with_the_grouping_mark() {
        // A Dutch card that grouped with a full stop but kept the English
        // decimal point would render 1.938,5 as 1.938.5, which is unreadable as
        // either convention.
        assert_eq!(Lang::En.measure(103.5), "103.5");
        assert_eq!(Lang::Nl.measure(103.5), "103,5");

        // A whole number has no decimal mark to swap, and must not gain one.
        assert_eq!(Lang::En.measure(1_500.0), "1,500");
        assert_eq!(Lang::Nl.measure(1_500.0), "1.500");
    }

    #[test]
    fn dutch_does_not_pluralise_a_counted_year_but_does_pluralise_a_day() {
        // The rule English uses, append an s, is wrong in Dutch for exactly the
        // noun a vehicle card uses most: an age in years.
        assert_eq!(Lang::Nl.count(1, &YEAR), "1 jaar");
        assert_eq!(Lang::Nl.count(13, &YEAR), "13 jaar");

        // The negative control: a rule that never pluralised anything in Dutch
        // would satisfy the two assertions above.
        assert_eq!(Lang::Nl.count(1, &DAY), "1 dag");
        assert_eq!(Lang::Nl.count(30, &DAY), "30 dagen");
        assert_eq!(Lang::Nl.count(5, &SEAT), "5 zitplaatsen");

        assert_eq!(Lang::En.count(1, &YEAR), "1 year");
        assert_eq!(Lang::En.count(13, &YEAR), "13 years");
    }

    #[test]
    fn a_direction_reads_naturally_in_both_languages() {
        assert_eq!(Lang::En.offset(0), "today");
        assert_eq!(Lang::Nl.offset(0), "vandaag");

        // English marks the future with a leading preposition and the past with
        // a trailing word; Dutch does the same but with different words, and
        // getting the two orders confused is the mistake this asserts against.
        assert_eq!(Lang::En.offset(90), "in 3 months");
        assert_eq!(Lang::Nl.offset(90), "over 3 maanden");
        assert_eq!(Lang::En.offset(-90), "3 months ago");
        assert_eq!(Lang::Nl.offset(-90), "3 maanden geleden");

        assert_eq!(Lang::Nl.offset(494), "over 1 jaar 4 maanden");
        assert_eq!(Lang::Nl.offset(-494), "1 jaar 4 maanden geleden");
    }

    /// Whether a phrase counts a noun exactly `count` times.
    ///
    /// Compared word by word. `"in 10 months"` contains the substring
    /// `"0 months"` and is perfectly correct, so a substring test here reports a
    /// defect in phrasing that reads fine.
    fn counts(phrase: &str, lang: Lang, count: i64, noun: &Noun) -> bool {
        let counted = lang.count(count, noun);
        let (number, word) = counted.split_once(' ').expect("a count is two words");
        phrase
            .split(' ')
            .collect::<Vec<&str>>()
            .windows(2)
            .any(|pair| pair[0] == number && pair[1] == word)
    }

    #[test]
    fn the_count_check_sees_a_count_it_is_meant_to_catch() {
        // The positive control for the two tests below. A helper that matched
        // nothing would pass both of them on any phrasing at all, including the
        // two they were written to forbid.
        assert!(counts("in 0 months", Lang::En, 0, &MONTH));
        assert!(counts("over 12 maanden", Lang::Nl, 12, &MONTH));
        // And the negative control: a number that merely ends in the digits it
        // is looking for is a different number.
        assert!(!counts("in 10 months", Lang::En, 0, &MONTH));
        assert!(!counts("over 112 maanden", Lang::Nl, 12, &MONTH));
    }

    #[test]
    fn rounding_never_produces_twelve_months() {
        // 725 days is 1 year and ~11.8 months, which rounds to 12 and must carry
        // into "2 years" rather than printing "1 year 12 months".
        for days in 350..800 {
            for lang in [Lang::En, Lang::Nl] {
                let phrase = lang.offset(days);
                assert!(
                    !counts(&phrase, lang, 12, &MONTH),
                    "day {days} rendered {phrase}"
                );
            }
        }
    }

    #[test]
    fn month_phrasing_never_says_zero_months() {
        // Past a year the months are a remainder, and a remainder that rounds to
        // nothing has to disappear rather than print itself: 365 days is a year,
        // not a year and no months. The range covers both sides of that boundary.
        for days in 45..1200 {
            for lang in [Lang::En, Lang::Nl] {
                let phrase = lang.offset(days);
                assert!(
                    !counts(&phrase, lang, 0, &MONTH),
                    "day {days} rendered {phrase}"
                );
            }
        }
    }

    #[test]
    fn nothing_is_ever_pluralised_as_one_of_something() {
        // A phrase reading "in 1 years" or "over 1 maanden" is the tell that a
        // unit was formatted without checking its count.
        for days in -900i64..900 {
            for lang in [Lang::En, Lang::Nl] {
                let phrase = lang.offset(days);
                // Compared word by word rather than by substring: "11 months"
                // contains "1 months" and is perfectly correct.
                let words: Vec<&str> = phrase.split(' ').collect();
                for pair in words.windows(2) {
                    if pair[0] != "1" {
                        continue;
                    }
                    for noun in [&DAY, &MONTH, &YEAR] {
                        let (one, many) = match lang {
                            Lang::En => noun.en,
                            Lang::Nl => noun.nl,
                        };
                        // Dutch `jaar` is its own plural, so a count of one
                        // legitimately prints the same word either way.
                        assert!(
                            one == many || pair[1] != many,
                            "day {days} rendered {phrase}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_span_is_the_offset_with_its_direction_removed() {
        // The two must not drift apart: the card prints a span beside a lag and
        // an offset beside an expiry, and a reader compares them.
        for days in [1, 29, 45, 200, 364, 365, 400, 494, 3_000] {
            for lang in [Lang::En, Lang::Nl] {
                let span = lang.span(days);
                assert_eq!(lang.offset(days), lang.fill(&FUTURE, &span));
                assert_eq!(lang.offset(-days), lang.fill(&PAST, &span));
            }
        }
    }
}

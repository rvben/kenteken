# kenteken

Look up Dutch vehicle data by licence plate, from the [RDW open data API](https://opendata.rdw.nl).

Built for humans at a terminal and for agents reading a pipe, equally: text when
stdout is a TTY, JSON when it is not, a machine-readable contract under
`kenteken schema`, and structured errors on stderr. It follows
[The CLI Spec](https://clispec.dev) v0.3.

```console
$ kenteken lookup X-99-XXX
X-99-XXX   Iveco 35s14
           Bedrijfsauto (N1), neerklapbare zijschotten, 3 zitplaatsen, 2 deuren

  APK verloopt           2027-12-11   over 1 jaar 4 maanden
  Tellerstand            logisch   laatste stand 2024
  Verzekerd (WAM)        ja
  Terugroepactie         geen openstaande

  Eerste toelating       2024-12-11   1 jaar 8 maanden geleden
  Tenaamstelling sinds   2024-12-11

  Brandstof              Diesel, 100 kW, 243 g/km CO2 (WLTP)
  Cilinderinhoud         2.287 cm3
  Massa                  2.059 kg leeg, 3.500 kg max
  Trekgewicht            750 kg ongeremd
  Afmetingen             691 cm lang, 213 cm breed, 228 cm hoog
  Catalogusprijs         EUR 91.144
```

The card is Dutch, because the register is: the plate, the values RDW returns and
the kentekenbewijs it stands in for are all Dutch, and `Tenaamstelling sinds` is
the phrase printed on the document. `--lang en` renders the same card in English.
Either way the JSON is unchanged; see [Language](#language).

Every plate shown here is a placeholder that RDW holds no vehicle under, so no
example points at someone's car. The values beside them are real register entries
rendered by the tool, so the layout, wrapping and wording are what you will see.

Anything that should stop you is shouted in words, not just coloured, so it
survives being piped, redirected or read by someone who cannot tell red from
grey: `VERLOPEN`, `NIET VERZEKERD`, `OPENSTAAND`, `ONLOGISCH`,
`OVERSCHRIJVING GEBLOKKEERD`, `TACHOGRAAF GEMANIPULEERD`.

## Install

```sh
cargo install kenteken
uv tool install kenteken     # or: pipx install kenteken
```

Or from a clone:

```sh
make install     # builds --release and copies to ~/.local/bin
```

No API key is needed. RDW open data is anonymous and in the public domain.
Setting `RDW_APP_TOKEN` to a [Socrata app token](https://opendata.rdw.nl/profile/edit/developer_settings)
raises the shared per-IP rate limit; the token is read from the environment only,
never from the command line.

## Commands

| Command | What it returns |
| --- | --- |
| `lookup <PLATE>...` | Registration summary: make, model, APK and tachograph expiry, masses, towing, dimensions, fuel, indicators |
| `defects <PLATE>...` | Defects recorded at inspection, each code resolved to its description |
| `fuel <PLATE>...` | Fuel and emissions rows, one per fuel for a hybrid or bifuel vehicle |
| `recalls <PLATE>...` | Manufacturer recalls, open ones first, each resolved to the defect, the hazard and the repair |
| `inspections <PLATE>...` | Notifications filed by inspection bodies, and the expiry each one produced |
| `raw <DATASET> <PLATE>...` | Rows from any known RDW dataset, exactly as RDW returned them |
| `datasets` | The datasets this build knows. Makes no network request |
| `schema` | The machine-readable contract, as JSON |
| `completions <SHELL>` | A shell completion script |

Every command is read-only, and every command takes several plates at once:

```sh
kenteken lookup X-99-XXX 9-XXX-99 9-XXXX-9
```

Plates are normalized before anything is sent, so `X-99-XXX`, `x99xxx` and
`X99XXX` are the same plate, and a plate that cannot be one is refused without
spending a request.

### Defects

Defect codes are resolved from a table embedded in the binary, so this needs one
request rather than two:

```console
$ kenteken defects 9-XXX-99 --limit 3
DATUM       CODE  GEBREK
2024-11-21  205   Band onvoldoende profiel
2024-11-21  419   Blokkering gordel werkt niet (goed)
2024-11-21  516   Dimlicht onjuist afgesteld
let op: toont rijen 1-3 van 11; verhoog --limit of blader met --offset
```

Rows arrive newest inspection first, so a page cut short by `--limit` shows the
most recent defects rather than an arbitrary three of eleven. The plate column
appears only when more than one plate was asked for.

Refresh the table from RDW with `make update-gebreken`, which writes
`data/gebreken.json` as a reviewable diff.

### Recalls

A recall is spread over three RDW datasets: one says which recalls a plate is
subject to and whether each is repaired, one describes the recall, and one lists
what it can cause. `recalls` reads all three and prints a card per recall:

```console
$ kenteken recalls 9-XXXX-9
9-XXXX-9   MGP070060   OPENSTAAND

  Gebrek                   De mogelijkheid bestaat dat de bouten van de
                           stuurkoppeling op de stuuras niet goed zijn vast
                           gedraaid.
  Categorie                Motorrijtuigen en aanhangwagens - stuurinrichting
  Risico                   Een (verkeers)ongeval met letselschade
  Gevolgen                 De kans bestaat dat de verbinding van de
                           stuurkoppeling op de stuuras los gaat zitten. Dit
                           kan worden herkend door optredend geluid tijdens
                           inparkeren en manouvreren bij lage snelheid. Na
                           verloop van tijd kan dit leiden tot losraken van
                           deze koppeling en onbestuurbaar worden van het
                           voertuig.
  Herstel                  De producent roept de betreffende voertuigen terug,
                           neemt maatregelen om het defect te verhelpen. De
                           voertuigeigenaar wordt uitgenodigd een afspraak te
                           maken met een merkdealer. De dealer zal de
                           stuurkoppeling dan vervangen.
  Gemeld door              Louwman Parts & Service B.V.
  Meer informatie          0162-585217
  Gepubliceerd             2013-03-28   7.500 voertuigen in de actie
  Eigenaren geïnformeerd   2007-11-06
```

RDW's prose is passed through in Dutch, untranslated and unabridged, because a
paraphrase of a safety instruction is a liability rather than a convenience. It
is rewrapped to the terminal under its label, and a paragraph stays whole.

Open recalls come first, so a page cut short by `--limit` shows what is still
outstanding rather than what was repaired years ago. A recall RDW has not
published detail for still names itself, its status and its reference.

A vehicle card names the hazard and points here, on two lines rather than one,
because a hazard is a sentence and a sentence joined to a shouted status wraps
into a run-on phrase:

```
  Terugroepactie          OPENSTAAND   zie: kenteken recalls 9-XXXX-9
  Risico terugroepactie   Een (verkeers)ongeval met letselschade
```

That card asks about recalls only when the register says one is outstanding, so
an ordinary lookup still costs the two requests it always did.

### Inspections

Every notification an inspection body filed about the vehicle, newest first, and
the expiry date it produced:

```console
$ kenteken inspections 9-XXX-99
DATUM       MELDING              GEMELD DOOR           GELDIG TOT
2024-11-21  periodieke controle  APK Zware voertuigen  2025-11-21
2024-02-29  periodieke controle  APK Zware voertuigen  2025-03-01
2024-02-29  periodieke controle  Controleapparaten     2026-03-01
```

Two bodies filing on one day is normal, and the two dates they set are not the
same kind of date: the APK station's expires a year out, the tachograph
workshop's two. The column is `GELDIG TOT` rather than `APK tot` for exactly that
reason, and the expiry belongs to the notification rather than to the vehicle.

Inspection bodies file five kinds of notification. Three are routine
(`periodieke controle`, `inbouw`, `uitbouw`); the other two, `manipulatie tacho`
and `zegelverbreking tacho`, mean someone interfered with the instrument that
records a professional driver's hours. Those two are shouted, as
`TACHOGRAAF GEMANIPULEERD` and `TACHOGRAAFZEGEL VERBROKEN`, and carry a stable
`derived.alarm` for a consumer that would rather match a value than a phrase.

### Raw datasets

`raw` reaches the datasets `lookup` does not summarize:

```sh
kenteken datasets                    # names and Socrata ids
kenteken raw assen X-99-XXX          # by short name
kenteken raw 3huj-srit X-99-XXX      # or by Socrata id
```

## Output

`--output/-o` takes `auto` (the default), `text`, `json`, `yaml` or `ndjson`.
`auto` is text on a TTY and JSON in a pipe, so a script gets JSON without asking.

JSON and YAML carry the whole envelope:

```console
$ kenteken lookup X99XXX --fields kenteken,merk,vervaldatum_apk
{
  "items": [
    {
      "kenteken": "X99XXX",
      "merk": "IVECO",
      "vervaldatum_apk": "20271211"
    }
  ],
  "total": 1,
  "limit": 100,
  "offset": 0,
  "truncated": false,
  "not_found": [],
  "no_rows": []
}
```

`items` is always an array, under every command, so a consumer parses one shape.
`total` counts every matching row and `truncated` says whether rows remain after
this page; page with `--limit` and `--offset` until `truncated` is false. NDJSON
prints one item per line and puts that metadata on stderr, where it cannot
corrupt the stream. Text says it in words.

`--fields` keeps only the named columns. A field present in no row is a usage
error rather than a silently empty column.

### Language

`--lang` takes `nl` (the default) or `en`, and moves the prose only:

```console
$ kenteken lookup X-99-XXX --lang en -o text | head -4
X-99-XXX   Iveco 35s14
           Bedrijfsauto (N1), neerklapbare zijschotten, 3 seats, 2 doors

  APK expires        2027-12-11   in 1 year 4 months
```

Dutch is the default because the register is Dutch: `Tenaamstelling sinds` and
`Cilinderinhoud` are the words on the kentekenbewijs, so the card reads as the
document it stands in for rather than as a translation of it. Numbers follow the
language, `2.287 cm3` in Dutch and `2,287 cm3` in English, since `1,880 kg` reads
to a Dutch eye as a weight just under two kilos.

RDW's own values are never translated in either language. A colour is `Grijs`, a
body is `hatchback`, a recall is described in RDW's Dutch, and the words RDW files
its verdicts under (`Openstaand`, `Logisch`) are what the Dutch card says.

Everything a program reads stays English and stays put:

- `json`, `yaml` and `ndjson` are byte-identical under either language. Keys,
  `derived` values and `derived.alarm` are the contract, so `--lang` cannot move
  them.
- `schema`, `--help` and the completion scripts are English.
- The error envelope and the ndjson metadata line are English, because they are
  parsed rather than read: an `error.kind` and a `total` are keys, and a script
  that greps for one must not have to know which language produced it.

The warnings and notes on stderr are prose for a reader, so they do follow
`--lang`. So do the dataset descriptions `kenteken datasets` prints, while the
`description` field in `json`, `yaml` and `ndjson` keeps its English wording.

## What RDW sent, and what it means

Every item carries RDW's own columns untouched, plus a `derived` block holding
this tool's reading of them. The text output is that same block, formatted, so a
human and an agent are looking at one computation rather than two that can
disagree.

```console
$ kenteken lookup XXX-99-X --fields derived -o json
{
  "items": [
    {
      "derived": {
        "plate": "XXX-99-X",
        "make": "TESLA",
        "model": "MODEL Y",
        "kind": "Personenauto",
        "eu_category": "M1",
        "body": "MPV",
        "seats": 5,
        "doors": 5,
        "colour": "ZWART",
        "second_colour": null,
        "apk_expiry": "2026-12-31",
        "apk_expired": false,
        "apk_days_remaining": 148,
        "tachograph_expiry": null,
        "tachograph_expired": null,
        "tachograph_days_remaining": null,
        "first_admission": "2024-12-31",
        "age_days": 582,
        "registered_since": "2024-12-31",
        "first_dutch_registration": "2024-12-31",
        "dutch_registration_lag_days": 0,
        "fuels": [
          "Elektriciteit"
        ],
        "power_kw": 378.0,
        "co2_g_per_km": null,
        "co2_basis": null,
        "electric_range_km": 533.0,
        "engine_cc": null,
        "energy_label": null,
        "mass_empty_kg": 1954,
        "mass_max_kg": 2518,
        "tow_braked_kg": 1600,
        "tow_unbraked_kg": 750,
        "length_cm": 475,
        "width_cm": 192,
        "height_cm": 162,
        "vin_location": null,
        "catalogue_price_eur": 51990,
        "odometer": "consistent",
        "odometer_year": 2026,
        "odometer_reason": "De geregistreerde tellerstand is steeds hoger dan de daarvoor geregistreerde tellerstand. Wij oordelen dan dat de tellerstand logisch verklaarbaar is.",
        "insured": true,
        "open_recall": false,
        "open_recall_count": 0,
        "open_recall_hazards": [],
        "exported": false,
        "taxi": true,
        "transferable": true
      }
    }
  ],
  "total": 1,
  "limit": 100,
  "offset": 0,
  "truncated": false,
  "not_found": [],
  "no_rows": []
}
```

Its keys are always present, and a fact RDW did not supply is `null`. Dates are
ISO 8601 rather than `20261231`, `Ja`/`Nee` are booleans, `Logisch` is
`consistent`, and a CO2 figure never appears without the test cycle that
produced it, because a WLTP number and an NEDC number are not comparable.

RDW writes makes, models, bodies and colours in capitals. The card calms them to
be read, `ZWART` to `Zwart`, while `derived` and the raw columns keep RDW's own
spelling so a consumer can still match it against RDW's documentation. Calming
is the wrong answer for a word that is written in capitals rather than shouted,
and no rule tells the two apart: `WIT` and `BMW` are three capital letters each.
So the exceptions are a list. It is complete for RDW's closed vocabularies, the
95 body styles holding exactly one initialism (`MPV`) and the 16 colours holding
none, and it is necessarily partial for 11,422 makes and 268,884 model names. An
initialism nobody has listed yet is still calmed.

```
99-XX-99   Mercedes-Benz 208 CDI
          Personenauto (M1), MPV, 9 zitplaatsen, Wit
```

`apk_expired` is `null` rather than `false` when there is no expiry date: a
vehicle that needs no inspection has not passed one.

`tachograph_expiry` is a second deadline rather than a copy of the first. A
tachograph is inspected on its own cycle by its own kind of workshop, so the two
dates routinely disagree, and one can be long past while the other is
comfortably in hand:

```
  APK verloopt           2025-11-21   VERLOPEN 8 maanden geleden
  Tachograaf verloopt    2026-03-01   VERLOPEN 5 maanden geleden
```

Most vehicles have no tachograph, and for them all three keys are `null`.
`tachograph_expired` in particular is `null` rather than `false`, by the same
rule as `apk_expired`: an instrument that does not exist has not stayed current.

`open_recall_count` follows the same rule. Zero means the register says nothing
is outstanding; `null` means the count is unknown, which is what a register
saying a recall is open while no recall row came back actually is. Its hazards
are `null` in that case too, since an empty list would read as "nothing to
worry about". `open_recall_hazards` lists each hazard once, in the order met:
RDW files one row per hazard, so a single recall routinely names several and two
recalls on one vehicle often name the same one twice.

`dutch_registration_lag_days` is the gap between first admission anywhere and
first registration in the Netherlands. It is a day count and not an import flag:
a long gap usually means a vehicle came from abroad, but RDW does not say so,
and a re-registration after a gap in ownership produces the same number. The
two dates are reported as they are, and what they mean is left to the reader.

`tow_braked_kg` and `tow_unbraked_kg` are the two halves of "can this pull my
caravan", and they are not interchangeable: the braked figure assumes the
trailer brakes itself, and is usually several times the unbraked one. Both are
reported, labelled, and neither is presented as *the* towing capacity.

`vin_location` says where on the vehicle the chassis number is stamped, which is
what you want when you are standing next to a car rather than reading about one.
It is left in RDW's own abbreviated Dutch, `r. op trekdriehoek 075 cm a. hart
koppeling`, because expanding those abbreviations means guessing at them, and a
wrong guess sends you looking at the wrong part of the vehicle.

`odometer_reason` is RDW's own explanation of the odometer verdict, resolved
from a table embedded in the binary, so it costs no request. Refresh it with
`make update-tellerstand`. The summary card shows it only when the verdict is
something other than `consistent`, since a paragraph explaining that all is
well is noise; JSON always carries it.

`odometer_year` is the year the readings behind that verdict stop, and it is
reported beside the verdict because the verdict carries no date of its own. A
history last read in 2016 and one read last month both say `consistent`, which
is the same word about very different evidence. The two are independent in the
register as well: 730,494 vehicles have a reading year and no verdict against
it, so the card prints whichever half exists rather than dropping the line.

RDW writes a placeholder into a column it has no value for rather than leaving
it empty: `N.v.t.`, `Niet geregistreerd`, `Geen verstrekking in Open Data`,
`Niet bekend` or `(Nog) niet bekend`. Read naively, `tweede_kleur` alone gives
ten million single-colour vehicles a second colour, and a recall's contact
column offers you `(Nog) niet bekend` as a phone number. The raw columns carry those strings verbatim, because that is
what RDW sent; `derived` resolves them to `null`.

A `0` can be a placeholder as well, and nothing but the column itself says
which, so each one was counted against the live register rather than reasoned
about:

- `length_cm`, `width_cm` and `height_cm` are `null` when RDW wrote a zero.
  430,531 passenger cars carry `lengte` 0, and not one of them is zero
  centimetres long.
- `doors` is read straight, because there a zero is a true count: 1,953,411
  vehicles have no doors, being trailers and motorcycles.
- `seats` needs neither rule. RDW omits the column rather than zeroing it, and
  not one row in 16.8 million carries a `0`.

`cilinderinhoud`, `catalogusprijs` and both the mass and towing columns behave
like `seats`, holding no zero anywhere. So a card can say `0 deuren`, and will
never say `0 cm lang`.

## Paging returns the same rows twice

Socrata leaves the order of an unsorted result undefined, and this endpoint has
been observed returning the same rows in different orders for two identical
requests. Every query therefore carries an explicit sort, newest first for
anything dated, so `--limit` is the first N rather than an arbitrary N, and
`--offset` neither skips nor repeats. `kenteken datasets -o json` shows the sort
used for each dataset.

Only results go to stdout. Warnings, notes and error envelopes go to stderr, and
`--quiet` silences the warnings and notes without changing stdout by a byte. It
does not silence the NDJSON metadata line: that is the envelope rather than a
warning, and it is the only place a consumer of that format can learn that rows
were withheld.

```console
$ kenteken -o ndjson --limit 2 --quiet datasets 2>&1 >/dev/null
{"total":18,"truncated":true,"not_found":[],"no_rows":[]}
```

## Absent, empty and missing are three different answers

Collapsing them is the one mistake a vehicle lookup cannot afford, so this tool
keeps them apart everywhere:

- **Not registered.** No vehicle exists under the plate. It is listed in
  `not_found`, and when no requested plate exists at all the run is a `not_found`
  error with exit 4.
- **Registered, nothing in this dataset.** The vehicle exists but has no rows
  here. It is listed in `no_rows` and the run exits 0. In text, `defects` says
  so positively: *"X99XXX is geregistreerd, zonder gebreken vastgesteld bij
  keuring"*, never a blank table that reads like a failed lookup.
- **Some of each.** With several plates, the ones that resolved are returned and
  the ones that did not are named on stderr, and the run exits 1 (`partial`).

A field RDW did not report is left out rather than rendered as `0` or an empty
string: `null` in JSON, `-` in a table cell, and no line at all on a summary
card, since a confident-looking label next to nothing is worse than silence.

Any command reading a dataset other than the register therefore reads the
register as well, purely to tell a typo from a clean bill of health. Without it,
`recalls XX-99-XX` on a mistyped plate would answer "no recalls", which is
exactly what you were hoping to read about a car that does not exist.

## Exit codes

| Code | Meaning | Retryable |
| --- | --- | --- |
| 0 | Success | |
| 1 | `partial`: some plates resolved, some are not registered. A data state, not an error, so no error envelope is written | |
| 2 | `network`: the RDW API could not be reached | yes |
| 3 | `usage`, `invalid_plate`, `unknown_dataset` | |
| 4 | `not_found`: no requested plate is registered | |
| 5 | `timeout` | yes |
| 6 | `rate_limit`: RDW returned HTTP 429 | yes |
| 7 | `api`: RDW answered with an error | |
| 8 | `io`: the answer was fetched but could not be written to stdout | |

A consumer that closes the pipe early, as `| head` does, is not an `io` failure:
it got what it asked for, and the run exits 0.

Errors are a single JSON object on the last line of stderr:

```console
$ kenteken lookup 'not a plate'; echo "exit=$?"
{"error":{"kind":"invalid_plate","message":"not a valid Dutch licence plate: 'NOTAPLATE' has 9 characters, every Dutch plate has 6","exit_code":3,"retryable":false,"hint":"plates are six letters and digits, e.g. X-99-XXX","details":{"input":"not a plate"}}}
exit=3
```

`kenteken schema` declares every error kind, its exit code and whether a retry
can behave differently, so a consumer can branch without parsing prose.

## Requests to RDW

RDW is a free public service, so the tool is deliberately quiet:

- No request is ever retried automatically. Transient failures are reported as
  `retryable` and the caller decides.
- `--concurrency` (default 4) is capped at 8.
- Malformed plates and datasets with no `kenteken` column are refused locally.
- `defects` needs no second request for the code table; it is embedded, and so
  is the odometer table `lookup` reads.
- `lookup` asks about recalls only when the register says one is outstanding.
- Recalls are named by reference and described elsewhere. Every reference a run
  collected is resolved together: one request per recall dataset, whether that
  is one recall or forty.

## Development

```sh
make check          # fmt, clippy -D warnings, and the test suite
make test
make conformance    # clispec score ./target/release/kenteken
```

The test suite never touches the network. `run` is generic over an `RdwSource`
trait that tests substitute, and the HTTP client is exercised against a fake RDW
on a local socket.

## Licence

MIT. RDW open data is published in the public domain; this tool is not
affiliated with RDW.

## Releasing

Vership owns versioning, changelog generation, release commits, and tags. See
[the release runbook](docs/releases.md) for the verified workflow and recovery policy.

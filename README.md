# kenteken

Look up Dutch vehicle data by licence plate, from the [RDW open data API](https://opendata.rdw.nl).

Built for humans at a terminal and for agents reading a pipe, equally: text when
stdout is a TTY, JSON when it is not, a machine-readable contract under
`kenteken schema`, and structured errors on stderr. It follows
[The CLI Spec](https://clispec.dev) v0.2.

```console
$ kenteken lookup X-99-XXX
X-99-XXX   Iveco 35s14
  Type               Bedrijfsauto (N1), neerklapbare zijschotten
  APK expires        2027-12-11   in 1 year 4 months
  First admitted     2024-12-11   1 year 8 months ago
  Registered since   2024-12-11
  Fuel               Diesel, 100 kW, 243 g/km CO2 (WLTP)
  Mass               2,059 kg empty, 3,500 kg max
  Catalogue price    EUR 91,144
  Odometer           consistent
  Insured (WAM)      yes
  Recall             none outstanding
```

Anything that should stop you is shouted in words, not just coloured, so it
survives being piped, redirected or read by someone who cannot tell red from
grey: `EXPIRED`, `NOT INSURED`, `OPEN RECALL`, `INCONSISTENT`,
`TRANSFER BLOCKED`.

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
| `lookup <PLATE>...` | Registration summary: make, model, APK expiry, masses, fuel, indicators |
| `defects <PLATE>...` | Defects recorded at inspection, each code resolved to its description |
| `fuel <PLATE>...` | Fuel and emissions rows, one per fuel for a hybrid or bifuel vehicle |
| `raw <DATASET> <PLATE>...` | Rows from any known RDW dataset, exactly as RDW returned them |
| `datasets` | The datasets this build knows. Makes no network request |
| `schema` | The machine-readable contract, as JSON |
| `completions <SHELL>` | A shell completion script |

Every command is read-only, and every command takes several plates at once:

```sh
kenteken lookup X-99-XXX 9-XXX-99 12-ABC-3
```

Plates are normalized before anything is sent, so `X-99-XXX`, `x99xxx` and
`X99XXX` are the same plate, and a plate that cannot be one is refused without
spending a request.

### Defects

Defect codes are resolved from a table embedded in the binary, so this needs one
request rather than two:

```console
$ kenteken defects 9-XXX-99 --limit 3
DATE        CODE  DEFECT
2024-11-21  205   Band onvoldoende profiel
2024-11-21  419   Blokkering gordel werkt niet (goed)
2024-11-21  516   Dimlicht onjuist afgesteld
note: showing rows 1-3 of 11; raise --limit or page with --offset
```

Rows arrive newest inspection first, so a page cut short by `--limit` shows the
most recent defects rather than an arbitrary three of eleven. The plate column
appears only when more than one plate was asked for.

Refresh the table from RDW with `make update-gebreken`, which writes
`data/gebreken.json` as a reviewable diff.

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
        "colour": "ZWART",
        "second_colour": null,
        "apk_expiry": "2026-12-31",
        "apk_expired": false,
        "apk_days_remaining": 148,
        "first_admission": "2024-12-31",
        "age_days": 582,
        "registered_since": "2024-12-31",
        "fuels": ["Elektriciteit"],
        "power_kw": 378.0,
        "co2_g_per_km": null,
        "co2_basis": null,
        "electric_range_km": 533.0,
        "mass_empty_kg": 1954,
        "mass_max_kg": 2518,
        "catalogue_price_eur": 51990,
        "odometer": "consistent",
        "insured": true,
        "open_recall": false,
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

`apk_expired` is `null` rather than `false` when there is no expiry date: a
vehicle that needs no inspection has not passed one.

RDW writes a placeholder into a column it has no value for rather than leaving
it empty: `N.v.t.`, `Niet geregistreerd`, or `Geen verstrekking in Open Data`.
Read naively, `tweede_kleur` alone gives ten million single-colour vehicles a
second colour. The raw columns carry those strings verbatim, because that is
what RDW sent; `derived` resolves them to `null`.

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
{"total":13,"truncated":true,"not_found":[],"no_rows":[]}
```

## Absent, empty and missing are three different answers

Collapsing them is the one mistake a vehicle lookup cannot afford, so this tool
keeps them apart everywhere:

- **Not registered.** No vehicle exists under the plate. It is listed in
  `not_found`, and when no requested plate exists at all the run is a `not_found`
  error with exit 4.
- **Registered, nothing in this dataset.** The vehicle exists but has no rows
  here. It is listed in `no_rows` and the run exits 0. In text, `defects` says
  so positively: *"X99XXX is registered, with no defects recorded at
  inspection"*, never a blank table that reads like a failed lookup.
- **Some of each.** With several plates, the ones that resolved are returned and
  the ones that did not are named on stderr, and the run exits 1 (`partial`).

A field RDW did not report is left out rather than rendered as `0` or an empty
string: `null` in JSON, `-` in a table cell, and no line at all on a summary
card, since a confident-looking label next to nothing is worse than silence.

`defects` and `fuel` therefore read the vehicle register as well, purely to tell
a typo from a clean bill of health.

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
- `defects` needs no second request for the code table; it is embedded.

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

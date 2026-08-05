//! kenteken CLI: Dutch vehicle data from the RDW open data API, by licence plate.
//!
//! Follows The CLI Spec (clispec.dev): structured output on stdout (text on a
//! TTY, JSON when piped), structured error envelopes on the last line of stderr,
//! a `schema` subcommand, and non-interactive behaviour. Every command is
//! read-only, so all of them are `mutating: false`.

use std::io::{IsTerminal, Write};
use std::process::ExitCode;
use std::time::Duration;

use clap::error::ErrorKind as ClapErrorKind;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use kenteken::output::Style;
use kenteken::text::{Lang, NOTE_NO_ROWS_IN_DATASET, NOTE_SHOWING_ROWS, WARNING_NOT_REGISTERED};
use kenteken::{
    Command as Op, HttpSource, KentekenError, OutputFormat, Plate, Request, rdw, run, schema,
};
use serde_json::json;

/// Default per-request timeout, in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Default number of requests in flight against RDW.
const DEFAULT_CONCURRENCY: usize = 4;

#[derive(Parser)]
#[command(
    name = "kenteken",
    version,
    about = "Look up Dutch vehicle data by licence plate, from the RDW open data API.",
    long_about = "Look up Dutch vehicle data by licence plate, from the RDW open data API.\n\n\
                  `kenteken lookup X-99-XXX` shows the registration summary; `defects` lists \
                  what an inspection found; `fuel` gives fuel and emissions; `recalls` says \
                  what a manufacturer recall is about and how it is fixed; `inspections` lists \
                  what inspection bodies filed; `raw` returns any RDW dataset untouched.\n\n\
                  Plates are normalized, so X-99-XXX, x99xxx and X99XXX are the same plate. \
                  Every command takes several plates at once.\n\n\
                  Run `kenteken schema` for the machine-readable contract (clispec.dev)."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output format; auto = text on a TTY, JSON when piped.
    #[arg(long, short = 'o', value_enum, default_value = "auto", global = true)]
    output: CliOutput,

    /// Language for text and its stderr notes. JSON, YAML and ndjson are English in both.
    #[arg(long, value_enum, default_value = "nl", global = true)]
    lang: CliLang,

    /// Suppress warnings on stderr. Errors and ndjson metadata still print.
    #[arg(long, short = 'q', global = true)]
    quiet: bool,

    /// Keep only these fields in each item (repeatable, or comma-separated).
    #[arg(long, value_delimiter = ',', value_name = "FIELD", global = true)]
    fields: Option<Vec<String>>,

    /// Maximum items to return.
    #[arg(long, default_value_t = kenteken::DEFAULT_LIMIT, value_name = "N", global = true)]
    limit: usize,

    /// Items to skip before the page starts.
    #[arg(long, default_value_t = 0, value_name = "N", global = true)]
    offset: usize,

    /// Requests in flight against RDW (capped at 8).
    #[arg(long, default_value_t = DEFAULT_CONCURRENCY, value_name = "N", global = true)]
    concurrency: usize,

    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_name = "SECONDS", global = true)]
    timeout: u64,
}

#[derive(Subcommand)]
enum Command {
    /// Registration summary: make, model, APK expiry, masses, fuel.
    Lookup {
        /// One or more plates, in any spelling: X-99-XXX, x99xxx, X99XXX.
        #[arg(value_name = "PLATE", required = true)]
        plates: Vec<String>,
    },
    /// Defects recorded at inspection, with each code resolved to its description.
    Defects {
        /// One or more plates, in any spelling: X-99-XXX, x99xxx, X99XXX.
        #[arg(value_name = "PLATE", required = true)]
        plates: Vec<String>,
    },
    /// Fuel and emissions rows; one per fuel for a hybrid or bifuel vehicle.
    Fuel {
        /// One or more plates, in any spelling: X-99-XXX, x99xxx, X99XXX.
        #[arg(value_name = "PLATE", required = true)]
        plates: Vec<String>,
    },
    /// Recalls, open ones first, each with its defect, hazard and repair.
    Recalls {
        /// One or more plates, in any spelling: X-99-XXX, x99xxx, X99XXX.
        #[arg(value_name = "PLATE", required = true)]
        plates: Vec<String>,
    },
    /// Notifications filed by inspection bodies, newest first.
    Inspections {
        /// One or more plates, in any spelling: X-99-XXX, x99xxx, X99XXX.
        #[arg(value_name = "PLATE", required = true)]
        plates: Vec<String>,
    },
    /// Rows from any known RDW dataset, exactly as RDW returned them.
    Raw {
        /// Dataset short name or Socrata id (see `kenteken datasets`).
        #[arg(value_name = "DATASET")]
        dataset: String,
        /// One or more plates, in any spelling: X-99-XXX, x99xxx, X99XXX.
        #[arg(value_name = "PLATE", required = true)]
        plates: Vec<String>,
    },
    /// List the RDW datasets this build knows. Makes no network request.
    Datasets,
    /// Print the machine-readable contract (clispec.dev) as JSON.
    Schema,
    /// Generate a shell completion script.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CliOutput {
    Auto,
    Json,
    Text,
    Yaml,
    Ndjson,
}

impl CliOutput {
    fn resolve(self) -> OutputFormat {
        match self {
            CliOutput::Json => OutputFormat::Json,
            CliOutput::Text => OutputFormat::Text,
            CliOutput::Yaml => OutputFormat::Yaml,
            CliOutput::Ndjson => OutputFormat::Ndjson,
            CliOutput::Auto => match std::io::stdout().is_terminal() {
                true => OutputFormat::Text,
                false => OutputFormat::Json,
            },
        }
    }
}

/// Which language the card speaks.
///
/// Dutch by default: the register is Dutch, the values RDW returns are Dutch,
/// and so is the document the card stands in for. English is there because the
/// vocabulary exists and still reads well, not because the data is neutral.
#[derive(Clone, Copy, ValueEnum)]
enum CliLang {
    Nl,
    En,
}

impl CliLang {
    fn resolve(self) -> Lang {
        match self {
            CliLang::Nl => Lang::Nl,
            CliLang::En => Lang::En,
        }
    }
}

/// Whether to colour the output, from the environment alone.
///
/// Split from the environment lookups so the decision itself is testable.
fn style_for(is_tty: bool, no_color: Option<&str>, term: Option<&str>) -> Style {
    // no-color.org: any value, including an empty one, disables colour. Only an
    // unset variable leaves it on.
    if no_color.is_some() || term == Some("dumb") || !is_tty {
        return Style::Plain;
    }
    Style::Colour
}

fn style() -> Style {
    style_for(
        std::io::stdout().is_terminal(),
        std::env::var("NO_COLOR").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
    )
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => return handle_clap_error(e),
    };

    match dispatch(&cli) {
        Ok(code) => code,
        Err(err) => {
            emit_error(&err);
            ExitCode::from(err.exit_code())
        }
    }
}

fn dispatch(cli: &Cli) -> Result<ExitCode, KentekenError> {
    let op = match &cli.command {
        Command::Schema => {
            println!("{}", schema::contract_json());
            return Ok(ExitCode::SUCCESS);
        }
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(*shell, &mut cmd, name, &mut std::io::stdout());
            return Ok(ExitCode::SUCCESS);
        }
        Command::Datasets => Op::Datasets,
        Command::Lookup { plates } => Op::Lookup {
            plates: parse_plates(plates)?,
        },
        Command::Defects { plates } => Op::Defects {
            plates: parse_plates(plates)?,
        },
        Command::Fuel { plates } => Op::Fuel {
            plates: parse_plates(plates)?,
        },
        Command::Recalls { plates } => Op::Recalls {
            plates: parse_plates(plates)?,
        },
        Command::Inspections { plates } => Op::Inspections {
            plates: parse_plates(plates)?,
        },
        Command::Raw { dataset, plates } => Op::Raw {
            dataset: rdw::datasets::resolve(dataset).ok_or(KentekenError::UnknownDataset {
                dataset: dataset.clone(),
            })?,
            plates: parse_plates(plates)?,
        },
    };

    if cli.limit == 0 {
        return Err(KentekenError::Usage {
            message: "--limit must be at least 1; a limit of 0 returns nothing at all".into(),
        });
    }
    if cli.timeout == 0 {
        return Err(KentekenError::Usage {
            message: "--timeout must be at least 1 second".into(),
        });
    }
    if cli.concurrency == 0 {
        return Err(KentekenError::Usage {
            message: "--concurrency must be at least 1".into(),
        });
    }
    if let Some(fields) = &cli.fields
        && fields.iter().any(|f| f.trim().is_empty())
    {
        return Err(KentekenError::Usage {
            message: "--fields contains an empty name".into(),
        });
    }

    let request = Request {
        command: op,
        format: cli.output.resolve(),
        style: style(),
        lang: cli.lang.resolve(),
        limit: cli.limit,
        offset: cli.offset,
        fields: cli.fields.clone(),
        concurrency: cli.concurrency,
    };

    let source = HttpSource::new(Duration::from_secs(cli.timeout))?;
    let outcome = run(&source, &request)?;

    // stdout carries only the result; anything explanatory goes to stderr, so a
    // pipe stays machine-readable.
    if let Some(code) = write_result(&mut std::io::stdout(), &outcome)? {
        return Ok(code);
    }

    if !cli.quiet {
        warn_about(&request, &outcome);
    }
    // Last, so the final line of stderr is always the structured one, and
    // ungated: the metadata line is the envelope rather than a warning, and it is
    // the only place an NDJSON consumer can learn that rows were withheld or a
    // plate was missing.
    emit_metadata(&request, &outcome);

    Ok(ExitCode::from(outcome.exit_code))
}

/// Write the result to stdout, or say why the run should stop.
///
/// Returns `Some(code)` when the consumer closed the pipe, which is not a
/// failure: `kenteken datasets | head -3` got exactly what it asked for. Any
/// other write failure is an error, because exiting zero on a half-written
/// answer is how a truncated file gets mistaken for a complete one.
fn write_result(
    out: &mut impl Write,
    outcome: &kenteken::Outcome,
) -> Result<Option<ExitCode>, KentekenError> {
    let written = if outcome.stdout.is_empty() {
        out.flush()
    } else {
        writeln!(out, "{}", outcome.stdout).and_then(|()| out.flush())
    };
    match written {
        Ok(()) => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(Some(ExitCode::SUCCESS)),
        Err(e) => Err(KentekenError::Io {
            message: e.to_string(),
        }),
    }
}

/// Put the envelope on stderr for the formats that cannot carry it on stdout.
///
/// NDJSON is one object per line by design, so `total`, `truncated`,
/// `not_found` and `no_rows` have nowhere in-band to live. Dropping them would
/// make a page cut short by `--limit` look like the complete answer.
fn emit_metadata(request: &Request, outcome: &kenteken::Outcome) {
    if request.format.has_envelope() {
        return;
    }
    eprintln!(
        "{}",
        json!({
            "total": outcome.total,
            "truncated": outcome.truncated,
            "not_found": outcome.not_found,
            "no_rows": outcome.no_rows,
        })
    );
}

/// Explain in words what stdout could not say, unless `--quiet`.
///
/// Text renders only the rows, so a page cut short by `--limit` would read as
/// the whole answer. JSON and YAML print the envelope and need no note. These
/// lines are prose for a reader, so they follow `--lang`; the machine surfaces
/// that stay English are the error envelope and the ndjson metadata line.
fn warn_about(request: &Request, outcome: &kenteken::Outcome) {
    let lang = request.lang;
    if !outcome.not_found.is_empty() {
        eprintln!(
            "{}",
            lang.fill(&WARNING_NOT_REGISTERED, &outcome.not_found.join(", "))
        );
    }
    if !outcome.no_rows.is_empty() {
        eprintln!(
            "{}",
            lang.fill(&NOTE_NO_ROWS_IN_DATASET, &outcome.no_rows.join(", "))
        );
    }
    if outcome.truncated && !request.format.states_counts() {
        eprintln!(
            "{}",
            truncation_note(lang, request.offset, outcome.shown, outcome.total)
        );
    }
}

/// Which rows of how many the reader is looking at.
///
/// The counts are grouped the way the language groups them, like every other
/// number this binary renders, so a listing of thousands of rows does not read
/// as one long digit string.
fn truncation_note(lang: Lang, offset: usize, shown: usize, total: usize) -> String {
    lang.fill_all(
        &NOTE_SHOWING_ROWS,
        &[
            &lang.thousands((offset + 1) as i64),
            &lang.thousands((offset + shown) as i64),
            &lang.thousands(total as i64),
        ],
    )
}

/// Normalize every plate argument, reporting the first that is not a plate.
///
/// Validation happens before any request: a malformed plate cannot match
/// anything at RDW, so asking would only spend someone else's bandwidth to
/// return an empty result that looks like "not registered".
fn parse_plates(raw: &[String]) -> Result<Vec<Plate>, KentekenError> {
    raw.iter()
        .map(|input| {
            Plate::parse(input).map_err(|source| KentekenError::InvalidPlate {
                input: input.clone(),
                source,
            })
        })
        .collect()
}

/// Help and version print normally and exit 0; every other clap failure becomes
/// a structured `usage` error envelope (so a bad invocation stays parseable).
fn handle_clap_error(e: clap::Error) -> ExitCode {
    match e.kind() {
        ClapErrorKind::DisplayHelp
        | ClapErrorKind::DisplayVersion
        | ClapErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            let _ = e.print();
            ExitCode::SUCCESS
        }
        _ => {
            let err = KentekenError::Usage {
                message: e.to_string().trim().to_string(),
            };
            emit_error(&err);
            ExitCode::from(err.exit_code())
        }
    }
}

/// Write the clispec error envelope as the last line of stderr.
fn emit_error(err: &KentekenError) {
    let mut error = serde_json::Map::new();
    error.insert("kind".into(), json!(err.kind()));
    error.insert("message".into(), json!(err.to_string()));
    error.insert("exit_code".into(), json!(err.exit_code()));
    error.insert("retryable".into(), json!(err.retryable()));
    if let Some(hint) = err.hint() {
        error.insert("hint".into(), json!(hint));
    }
    if let Some(details) = err.details() {
        error.insert("details".into(), details);
    }
    eprintln!("{}", json!({ "error": error }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    /// A destination that fails every write the same way.
    ///
    /// The real failures this stands in for (a full disk, a vanished mount) are
    /// not reproducible from a test on every platform, and the branch that
    /// decides between "the consumer left" and "the answer was lost" is exactly
    /// the part worth pinning down.
    struct Failing(ErrorKind);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(Error::from(self.0))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(Error::from(self.0))
        }
    }

    fn outcome(stdout: &str) -> kenteken::Outcome {
        kenteken::Outcome {
            stdout: stdout.to_string(),
            not_found: Vec::new(),
            no_rows: Vec::new(),
            shown: 1,
            total: 1,
            truncated: false,
            exit_code: 0,
        }
    }

    #[test]
    fn a_written_result_ends_in_exactly_one_newline() {
        let mut buffer = Vec::new();
        let code = write_result(&mut buffer, &outcome("{\"a\":1}")).unwrap();
        assert!(code.is_none(), "a successful write must not end the run");
        assert_eq!(String::from_utf8(buffer).unwrap(), "{\"a\":1}\n");
    }

    #[test]
    fn a_consumer_closing_the_pipe_is_not_a_failure() {
        // `kenteken datasets | head -1` got what it asked for.
        let code = write_result(&mut Failing(ErrorKind::BrokenPipe), &outcome("row")).unwrap();
        assert!(code.is_some(), "a broken pipe must end the run quietly");
    }

    #[test]
    fn a_lost_result_is_reported_rather_than_exiting_successfully() {
        // Exiting 0 here is how a truncated file gets mistaken for a complete
        // answer, so every other write failure has to surface.
        for kind in [
            ErrorKind::StorageFull,
            ErrorKind::PermissionDenied,
            ErrorKind::Other,
        ] {
            let err = write_result(&mut Failing(kind), &outcome("row"))
                .expect_err("a failed write must not report success");
            assert_eq!(err.kind(), "io", "for {kind:?}");
            assert_eq!(err.exit_code(), 8, "for {kind:?}");
        }
    }

    #[test]
    fn colour_is_only_for_a_terminal_that_asked_for_it() {
        assert_eq!(style_for(true, None, Some("xterm-256color")), Style::Colour);
    }

    #[test]
    fn anything_that_is_not_an_interactive_terminal_gets_plain_output() {
        // An escape sequence written into a pipe, a file or a log corrupts
        // whatever reads it next.
        let cases = [
            (false, None, Some("xterm-256color"), "not a terminal"),
            (true, Some("1"), Some("xterm"), "NO_COLOR is set"),
            (true, Some(""), Some("xterm"), "NO_COLOR is set but empty"),
            (true, None, Some("dumb"), "a dumb terminal"),
            (false, None, None, "no terminal at all"),
        ];
        for (tty, no_color, term, why) in cases {
            assert_eq!(style_for(tty, no_color, term), Style::Plain, "{why}");
        }
    }

    #[test]
    fn every_plate_argument_documents_itself() {
        // An empty description column in `--help` is the tell that a positional
        // argument was declared without one.
        for sub in ["lookup", "defects", "fuel", "recalls", "inspections", "raw"] {
            let help = Cli::command()
                .get_subcommands_mut()
                .find(|c| c.get_name() == sub)
                .expect("subcommand exists")
                .render_help()
                .to_string();
            // The usage line names <PLATE> too, so read the Arguments section,
            // where the line begins with the argument itself.
            let line = help
                .lines()
                .map(str::trim)
                .find(|l| l.starts_with("<PLATE>"))
                .unwrap_or_else(|| panic!("{sub} lists no PLATE argument:\n{help}"));
            let description = line.split_once("  ").map_or("", |(_, d)| d.trim());
            assert!(
                description.len() > 20,
                "{sub} documents PLATE as {description:?}"
            );
        }
    }

    #[test]
    fn the_truncation_note_counts_the_page_the_reader_is_looking_at() {
        // Off by one here misreports which rows someone just read: the first row
        // of an unoffset page is row 1, and the last is the one after the offset.
        assert_eq!(
            truncation_note(Lang::En, 0, 3, 11),
            "note: showing rows 1-3 of 11; raise --limit or page with --offset"
        );
        assert_eq!(
            truncation_note(Lang::En, 10, 5, 20),
            "note: showing rows 11-15 of 20; raise --limit or page with --offset"
        );
        assert_eq!(
            truncation_note(Lang::Nl, 0, 3, 11),
            "let op: toont rijen 1-3 van 11; verhoog --limit of blader met --offset"
        );
    }

    #[test]
    fn the_truncation_note_groups_its_thousands_the_way_the_language_does() {
        // A five digit row count read as one string is the reason every other
        // rendered number is grouped, and the two languages group it differently.
        assert!(
            truncation_note(Lang::En, 0, 10, 21_938).contains("of 21,938"),
            "{}",
            truncation_note(Lang::En, 0, 10, 21_938)
        );
        assert!(
            truncation_note(Lang::Nl, 0, 10, 21_938).contains("van 21.938"),
            "{}",
            truncation_note(Lang::Nl, 0, 10, 21_938)
        );
    }

    #[test]
    fn an_empty_result_is_still_flushed_and_still_checked() {
        // An empty answer is a real answer; failing to deliver it is a failure
        // like any other.
        let err = write_result(&mut Failing(ErrorKind::StorageFull), &outcome(""))
            .expect_err("an unflushable empty result must not report success");
        assert_eq!(err.kind(), "io");
    }
}

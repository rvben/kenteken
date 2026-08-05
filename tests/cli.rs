//! End-to-end tests of the compiled binary: the clispec error envelope, the
//! exit-code contract, and the stdout/stderr split.
//!
//! Every test here reaches a decision before any request would be made, so the
//! suite never contacts RDW. The network path is covered against a local fake in
//! `tests/http.rs`.

use serde_json::Value;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_kenteken");

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Output {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("spawn kenteken");
    Output {
        code: out.status.code().expect("process exited normally"),
        stdout: String::from_utf8(out.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(out.stderr).expect("stderr is UTF-8"),
    }
}

/// The `error` object from the last line of stderr (the clispec envelope).
fn error_envelope(stderr: &str) -> Value {
    let last = stderr.lines().last().expect("stderr has an error line");
    serde_json::from_str::<Value>(last).expect("the last stderr line is the JSON envelope")["error"]
        .clone()
}

#[test]
fn schema_is_clispec_v0_2_on_stdout() {
    let out = run(&["schema"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: Value = serde_json::from_str(&out.stdout).expect("schema is JSON");
    assert_eq!(v["clispec"], "0.2");
    assert_eq!(v["name"], "kenteken");
    assert!(out.stderr.is_empty(), "stderr: {}", out.stderr);
}

#[test]
fn datasets_answers_without_touching_the_network() {
    let out = run(&["datasets"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: Value = serde_json::from_str(&out.stdout).expect("stdout is JSON when piped");
    assert!(v["total"].as_u64().unwrap() >= 18);
    let names: Vec<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    for dataset in [
        "voertuigen",
        "gebreken",
        "terugroepactie-status",
        "terugroepactie",
        "terugroepactie-risico",
        "meldingen",
    ] {
        assert!(names.contains(&dataset), "{dataset} is not listed");
    }
}

#[test]
fn output_defaults_to_json_when_stdout_is_piped() {
    // The process output is captured, so stdout is not a TTY: clispec's `auto`
    // must resolve to JSON here without being asked.
    let out = run(&["datasets"]);
    serde_json::from_str::<Value>(&out.stdout).expect("piped stdout is JSON");
}

#[test]
fn text_output_is_a_table_and_carries_no_ansi() {
    let out = run(&["--output", "text", "datasets"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("voertuigen"), "stdout:\n{}", out.stdout);
    assert!(out.stdout.starts_with("NAAM"), "stdout:\n{}", out.stdout);
    assert!(
        !out.stdout.contains('\u{1b}'),
        "text output contains escapes"
    );
}

#[test]
fn the_language_flag_reaches_the_rendered_text() {
    // Wiring, not vocabulary: the flag is parsed in `main`, resolved into a
    // `Voice` in `lib`, and only then reaches the renderer. Both directions are
    // asserted, because a build that ignored the flag would still pass one.
    let dutch = run(&["--output", "text", "datasets"]);
    assert_eq!(dutch.code, 0, "stderr: {}", dutch.stderr);
    assert!(
        dutch.stdout.starts_with("NAAM"),
        "stdout:\n{}",
        dutch.stdout
    );

    let english = run(&["--output", "text", "--lang", "en", "datasets"]);
    assert_eq!(english.code, 0, "stderr: {}", english.stderr);
    assert!(
        english.stdout.starts_with("NAME"),
        "stdout:\n{}",
        english.stdout
    );
}

#[test]
fn the_language_the_schema_declares_by_default_is_the_one_the_binary_renders() {
    // An agent reads `--lang`'s default from the contract and never passes the
    // flag. A contract that named the other language would send it the card in a
    // language it did not ask for, and nothing else here would notice.
    let schema: Value = serde_json::from_str(&run(&["schema"]).stdout).expect("schema is JSON");
    let declared = schema["global_args"]
        .as_array()
        .expect("global_args is an array")
        .iter()
        .find(|a| a["name"] == "--lang")
        .expect("--lang is declared")["default"]
        .as_str()
        .expect("the default is a string");

    let heading = run(&["--output", "text", "datasets"]);
    let expected = match declared {
        "nl" => "NAAM",
        "en" => "NAME",
        other => panic!("the contract declares an unknown language {other:?}"),
    };
    assert!(
        heading.stdout.starts_with(expected),
        "the contract says {declared:?}, stdout:\n{}",
        heading.stdout
    );
}

#[test]
fn the_machine_formats_read_the_same_in_either_language() {
    // JSON is the contract. A key or value that moved with `--lang` would break
    // every consumer that pinned the English one.
    let dutch = run(&["datasets"]);
    let english = run(&["--lang", "en", "datasets"]);
    assert_eq!(dutch.code, 0, "stderr: {}", dutch.stderr);
    assert_eq!(english.code, 0, "stderr: {}", english.stderr);
    assert_eq!(dutch.stdout, english.stdout);
}

#[test]
fn yaml_output_parses_as_yaml() {
    let out = run(&["-o", "yaml", "datasets"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: Value = serde_norway::from_str(&out.stdout).expect("stdout is YAML");
    assert!(v["items"].is_array());
}

#[test]
fn ndjson_output_is_one_object_per_line_with_the_metadata_on_stderr() {
    let out = run(&["-o", "ndjson", "datasets"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    for line in out.stdout.lines() {
        let v: Value = serde_json::from_str(line).expect("each stdout line parses alone");
        assert!(v["id"].is_string(), "line was {line}");
        assert!(v.get("items").is_none(), "the envelope leaked into stdout");
    }
    // NDJSON has nowhere in-band for the envelope, so it goes to stderr rather
    // than being dropped.
    let last: Value =
        serde_json::from_str(out.stderr.lines().last().expect("stderr has a line")).unwrap();
    assert!(last["not_found"].is_array());
    assert_eq!(
        last["total"].as_u64().unwrap(),
        out.stdout.lines().count() as u64,
        "the metadata line must account for every row"
    );
    assert_eq!(last["truncated"], false);
}

#[test]
fn an_ndjson_page_cut_short_says_so_in_its_metadata_line() {
    // Without `total` and `truncated` on stderr, a short NDJSON stream is
    // indistinguishable from a complete one.
    let out = run(&["-o", "ndjson", "--limit", "2", "datasets"]);
    assert_eq!(out.stdout.lines().count(), 2);
    let last: Value =
        serde_json::from_str(out.stderr.lines().last().expect("stderr has a line")).unwrap();
    assert_eq!(last["truncated"], true);
    assert!(last["total"].as_u64().unwrap() > 2);
}

#[test]
fn quiet_suppresses_the_stderr_note_but_not_stdout() {
    let noisy = run(&["-o", "text", "--limit", "2", "datasets"]);
    assert!(noisy.stderr.contains("--limit"), "stderr: {}", noisy.stderr);

    let quiet = run(&["-o", "text", "--limit", "2", "--quiet", "datasets"]);
    assert_eq!(quiet.code, 0);
    assert!(quiet.stderr.is_empty(), "stderr: {}", quiet.stderr);
    assert_eq!(quiet.stdout, noisy.stdout, "--quiet changed the result");
}

#[test]
fn quiet_never_takes_away_the_ndjson_metadata_line() {
    // The metadata line is the envelope, not a warning. Suppressing it would
    // leave an NDJSON consumer no way at all to learn that rows were withheld,
    // turning a partial stream into a confident complete-looking answer.
    let quiet = run(&["-o", "ndjson", "--limit", "2", "--quiet", "datasets"]);
    assert_eq!(quiet.code, 0, "stderr: {}", quiet.stderr);
    assert_eq!(quiet.stdout.lines().count(), 2);

    let lines: Vec<&str> = quiet.stderr.lines().collect();
    assert_eq!(lines.len(), 1, "stderr: {}", quiet.stderr);
    let meta: Value = serde_json::from_str(lines[0]).expect("the metadata line is JSON");
    assert_eq!(meta["truncated"], true);
    assert!(meta["total"].as_u64().unwrap() > 2);
}

#[test]
fn a_pipeline_consumer_taking_one_line_gets_a_clean_exit() {
    // This output fits in a pipe buffer, so the write completes before `head`
    // leaves and no broken pipe arises: what this pins down is that a piped run
    // is line-oriented and exits 0. The broken-pipe branch itself is covered by
    // unit tests in `src/main.rs`, which can force the error.
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("{BIN} -o ndjson datasets | head -1"))
        .output()
        .expect("spawn a pipeline");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8(out.stdout).unwrap().lines().count(), 1);
}

/// A result that never reaches its destination must not exit 0, or an empty
/// file looks like a complete answer.
///
/// Only Linux can stage this honestly: `/dev/full` fails every write with
/// ENOSPC. macOS has no equivalent (a closed or read-only stdout is quietly
/// reopened or accepted), so the mapping from a write failure to the `io` error
/// is pinned down by unit tests in `src/main.rs` instead.
#[cfg(target_os = "linux")]
#[test]
fn a_result_that_cannot_be_written_is_an_error_rather_than_a_silent_success() {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("{BIN} datasets > /dev/full"))
        .output()
        .expect("spawn against /dev/full");
    assert_eq!(out.status.code(), Some(8), "a lost result exited cleanly");
    let err = error_envelope(&String::from_utf8(out.stderr).unwrap());
    assert_eq!(err["kind"], "io");
    assert_eq!(err["retryable"], false);
}

#[test]
fn an_invalid_plate_is_a_usage_error_with_a_structured_envelope() {
    let out = run(&["lookup", "NOPE"]);
    assert_eq!(out.code, 3, "stderr: {}", out.stderr);
    let err = error_envelope(&out.stderr);
    assert_eq!(err["kind"], "invalid_plate");
    assert_eq!(err["exit_code"], 3);
    assert_eq!(err["retryable"], false);
    assert_eq!(err["details"]["input"], "NOPE");
    assert!(err["hint"].is_string());
    assert!(out.stdout.is_empty(), "stdout: {}", out.stdout);
}

#[test]
fn a_plate_is_rejected_before_any_request_is_made() {
    // A plate RDW cannot match must not cost RDW a request. The proof that no
    // request happened is the speed and the error kind: a network attempt to a
    // dataset would surface as network/timeout, never invalid_plate.
    let out = run(&["defects", "X99XX/"]);
    assert_eq!(out.code, 3);
    assert_eq!(error_envelope(&out.stderr)["kind"], "invalid_plate");
}

#[test]
fn an_unknown_dataset_is_reported_with_the_name_that_was_asked_for() {
    let out = run(&["raw", "nonsense", "X99XXX"]);
    assert_eq!(out.code, 3, "stderr: {}", out.stderr);
    let err = error_envelope(&out.stderr);
    assert_eq!(err["kind"], "unknown_dataset");
    assert_eq!(err["details"]["dataset"], "nonsense");
    assert!(
        err["hint"].as_str().unwrap().contains("datasets"),
        "hint was {}",
        err["hint"]
    );
}

#[test]
fn a_bad_flag_becomes_a_usage_envelope_rather_than_claps_own_format() {
    let out = run(&["--frobnicate", "datasets"]);
    assert_eq!(out.code, 3, "stderr: {}", out.stderr);
    assert_eq!(error_envelope(&out.stderr)["kind"], "usage");
}

#[test]
fn a_missing_plate_argument_is_a_usage_error() {
    let out = run(&["lookup"]);
    assert_eq!(out.code, 3, "stderr: {}", out.stderr);
    assert_eq!(error_envelope(&out.stderr)["kind"], "usage");
}

#[test]
fn a_zero_limit_is_refused_rather_than_returning_nothing() {
    // `--limit 0` returning an empty result with exit 0 would be
    // indistinguishable from a genuine empty answer.
    let out = run(&["--limit", "0", "datasets"]);
    assert_eq!(out.code, 3, "stderr: {}", out.stderr);
    assert_eq!(error_envelope(&out.stderr)["kind"], "usage");
}

#[test]
fn a_zero_timeout_and_a_zero_concurrency_are_refused() {
    for args in [
        ["--timeout", "0", "datasets"],
        ["--concurrency", "0", "datasets"],
    ] {
        let out = run(&args);
        assert_eq!(out.code, 3, "args {args:?}, stderr: {}", out.stderr);
        assert_eq!(
            error_envelope(&out.stderr)["kind"],
            "usage",
            "args {args:?}"
        );
    }
}

#[test]
fn an_unknown_field_is_an_error_not_an_empty_object() {
    let out = run(&["--fields", "bogus", "datasets"]);
    assert_eq!(out.code, 3, "stderr: {}", out.stderr);
    let err = error_envelope(&out.stderr);
    assert_eq!(err["kind"], "usage");
    assert!(err["message"].as_str().unwrap().contains("bogus"));
}

#[test]
fn fields_projects_the_items() {
    let out = run(&["--fields", "name,id", "datasets"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: Value = serde_json::from_str(&out.stdout).unwrap();
    let item = v["items"][0].as_object().unwrap();
    assert_eq!(item.len(), 2);
    assert!(item.contains_key("name") && item.contains_key("id"));
}

#[test]
fn limit_and_offset_page_without_lying_about_the_total() {
    let all: Value = serde_json::from_str(&run(&["datasets"]).stdout).unwrap();
    let total = all["total"].as_u64().unwrap();

    let out = run(&["--limit", "2", "--offset", "1", "datasets"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(
        v["total"], total,
        "total must count every row, not the page"
    );
    assert_eq!(v["items"].as_array().unwrap().len(), 2);
    assert_eq!(v["truncated"], true);
    assert_eq!(v["items"][0]["name"], all["items"][1]["name"]);
}

#[test]
fn a_text_page_cut_short_by_limit_says_so_on_stderr() {
    // Text output is just the rows. A human given two of eighteen datasets with
    // nothing said would take those two for the whole list.
    let total = serde_json::from_str::<Value>(&run(&["datasets"]).stdout).unwrap()["total"]
        .as_u64()
        .unwrap();
    let out = run(&["-o", "text", "--limit", "2", "datasets"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains(&format!("showing rows 1-2 of {total}")),
        "stderr was: {}",
        out.stderr
    );
}

#[test]
fn the_final_page_of_a_text_listing_is_not_announced_as_cut_short() {
    let total = serde_json::from_str::<Value>(&run(&["datasets"]).stdout).unwrap()["total"]
        .as_u64()
        .unwrap();
    let out = run(&[
        "-o",
        "text",
        "--offset",
        &(total - 1).to_string(),
        "datasets",
    ]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("showing rows"),
        "the last page carries every remaining row, stderr was: {}",
        out.stderr
    );
}

#[test]
fn help_and_version_print_normally_and_exit_zero() {
    for arg in ["--help", "-h", "--version", "-V"] {
        let out = run(&[arg]);
        assert_eq!(out.code, 0, "{arg} exited {}", out.code);
        assert!(!out.stdout.is_empty(), "{arg} printed nothing");
        assert!(out.stderr.is_empty(), "{arg} wrote to stderr");
    }
}

#[test]
fn help_lists_every_command() {
    let out = run(&["--help"]);
    for command in [
        "lookup",
        "defects",
        "fuel",
        "recalls",
        "inspections",
        "raw",
        "datasets",
        "schema",
    ] {
        assert!(
            out.stdout.contains(command),
            "--help does not mention {command}"
        );
    }
}

#[test]
fn completions_are_generated_for_every_supported_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let out = run(&["completions", shell]);
        assert_eq!(out.code, 0, "{shell}: {}", out.stderr);
        assert!(
            out.stdout.contains("kenteken"),
            "{shell} script looks empty"
        );
    }
}

#[test]
fn the_binary_never_prompts_and_never_reads_stdin() {
    // clispec: non-interactive. Running with stdin closed must change nothing.
    use std::process::Stdio;
    let out = Command::new(BIN)
        .args(["datasets"])
        .stdin(Stdio::null())
        .output()
        .expect("spawn kenteken");
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}

#[test]
fn every_declared_error_kind_appears_in_the_schema() {
    // A consumer branches on `kind`; an envelope carrying a kind the schema does
    // not declare breaks an exhaustive handler.
    let schema: Value = serde_json::from_str(&run(&["schema"]).stdout).unwrap();
    let declared: Vec<String> = schema["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();

    let observed = [
        run(&["lookup", "NOPE"]),
        run(&["raw", "nonsense", "X99XXX"]),
        run(&["--frobnicate", "datasets"]),
    ];
    for out in observed {
        let kind = error_envelope(&out.stderr)["kind"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(declared.contains(&kind), "undeclared error kind {kind}");
    }
}

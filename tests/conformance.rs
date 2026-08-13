//! `kenteken schema` must validate against the published clispec v0.3 JSON
//! Schema (vendored at schemas/clispec-v0.3.json).

use serde_json::Value;

fn contract() -> Value {
    kenteken::schema::contract()
}

#[test]
fn schema_conforms_to_clispec_v0_3() {
    let schema: Value = serde_json::from_str(include_str!("../schemas/clispec-v0.3.json"))
        .expect("vendored clispec schema is valid JSON");

    let instance = contract();
    let validator = jsonschema::validator_for(&schema).expect("compile clispec schema");

    if !validator.is_valid(&instance) {
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        panic!(
            "kenteken schema does not conform to clispec v0.3:\n{}",
            errors.join("\n")
        );
    }
}

#[test]
fn the_vendored_schema_rejects_a_document_it_should_reject() {
    // Negative control. Without it, a validator that accepts everything would
    // make the test above pass no matter what the contract said.
    let schema: Value = serde_json::from_str(include_str!("../schemas/clispec-v0.3.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();

    let mut broken = contract();
    // An error kind must be snake_case per the schema pattern.
    broken["errors"][0]["kind"] = Value::from("Not Snake Case");
    assert!(
        !validator.is_valid(&broken),
        "the validator accepts a contract that violates the schema"
    );
}

#[test]
fn schema_declares_the_expected_shape() {
    let v = contract();
    assert_eq!(v["clispec"], "0.3");
    assert_eq!(v["name"], "kenteken");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));

    let commands = v["commands"].as_array().unwrap();
    let names: Vec<&str> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    for expected in [
        "lookup",
        "defects",
        "fuel",
        "recalls",
        "inspections",
        "raw",
        "datasets",
        "schema",
        "completions",
    ] {
        assert!(
            names.contains(&expected),
            "schema omits the {expected} command"
        );
    }

    // Every command reads; nothing here changes anything at RDW or on disk.
    for command in commands {
        assert_eq!(
            command["mutating"], false,
            "{} is not declared read-only",
            command["name"]
        );
    }

    assert!(v["errors"].as_array().is_some_and(|e| !e.is_empty()));
    assert!(v["global_args"].as_array().is_some_and(|g| !g.is_empty()));
    assert!(v["outcomes"].as_array().is_some_and(|o| !o.is_empty()));
}

#[test]
fn the_partial_outcome_is_declared_so_exit_one_is_not_read_as_a_failure() {
    let v = contract();
    let partial = v["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "partial")
        .expect("the partial outcome is declared");
    assert_eq!(partial["code"], kenteken::EXIT_PARTIAL);
}

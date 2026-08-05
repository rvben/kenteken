.PHONY: build release test lint fmt check conformance clean install update-deps update-gebreken update-tellerstand release-patch release-minor release-major

build:
	cargo build

release:
	cargo build --release

test:
	cargo nextest run

lint:
	cargo fmt -- --check
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt

check: lint test

# Score the binary against The CLI Spec (clispec.dev). Requires `clispec`
# (cargo install clispec). The schema's conformance to clispec v0.2 is also
# verified hermetically by `make test`.
#
# `datasets` is the probe: kenteken has no default command, so scoring the bare
# binary measures the help output rather than a data path. `datasets` answers
# offline, so the score costs RDW nothing.
conformance: release
	clispec score ./target/release/kenteken datasets

# Refresh the embedded defect-code table from RDW. Writes data/gebreken.json so
# the change lands as a reviewable diff rather than a silent behaviour change.
update-gebreken:
	./scripts/update-gebreken.sh

# Refresh the embedded odometer-judgement explanations from RDW, the same way.
update-tellerstand:
	./scripts/update-tellerstand.sh

clean:
	cargo clean

install: release
	mkdir -p ~/.local/bin
	cp target/release/kenteken ~/.local/bin/kenteken

update-deps:
	upd --apply --max-bump minor --lang rust,actions

release-patch:
	vership bump patch

release-minor:
	vership bump minor

release-major:
	vership bump major

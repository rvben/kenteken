#!/usr/bin/env bash
#
# Refresh data/gebreken.json, the RDW defect-code table embedded in the binary.
#
# Writes a code -> description object with sorted keys, so a refresh shows up as
# a readable diff instead of a reshuffled file. Makes exactly one request.
set -euo pipefail

DATASET="hx2c-gt7k"
OUT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/data/gebreken.json"
TMP="$(mktemp -t gebreken)"
trap 'rm -f "$TMP"' EXIT

echo "fetching ${DATASET} from opendata.rdw.nl"
curl --fail --silent --show-error \
	--user-agent "kenteken-update-gebreken" \
	-o "$TMP" \
	"https://opendata.rdw.nl/resource/${DATASET}.json?\$limit=50000"

python3 - "$TMP" "$OUT" <<'PY'
import json
import sys

raw_path, out_path = sys.argv[1], sys.argv[2]
with open(raw_path, encoding="utf-8") as fh:
    rows = json.load(fh)

table = {}
dropped = []
for row in rows:
    code = (row.get("gebrek_identificatie") or "").strip()
    description = (row.get("gebrek_omschrijving") or "").strip()
    # A code with no description would render as a blank column that reads like
    # "no defect". Drop it loudly rather than embedding an empty string.
    if not code or not description:
        dropped.append(row)
        continue
    table[code] = description

if dropped:
    print(f"warning: dropped {len(dropped)} row(s) with no code or no description",
          file=sys.stderr)
if len(table) < 900:
    raise SystemExit(f"refusing to write only {len(table)} codes; RDW returned {len(rows)} rows")

with open(out_path, "w", encoding="utf-8") as fh:
    json.dump(table, fh, ensure_ascii=False, sort_keys=True, indent=1)
    fh.write("\n")

print(f"wrote {len(table)} codes to {out_path}")
PY

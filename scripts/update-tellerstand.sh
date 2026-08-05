#!/usr/bin/env bash
#
# Refresh data/tellerstand.json, the RDW odometer-judgement explanations
# embedded in the binary.
#
# Writes a code -> explanation object with sorted keys, so a refresh shows up as
# a readable diff instead of a reshuffled file. Makes exactly one request.
set -euo pipefail

DATASET="jqs4-4kvw"
OUT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/data/tellerstand.json"
TMP="$(mktemp -t tellerstand)"
trap 'rm -f "$TMP"' EXIT

echo "fetching ${DATASET} from opendata.rdw.nl"
curl --fail --silent --show-error \
	--user-agent "kenteken-update-tellerstand" \
	-o "$TMP" \
	"https://opendata.rdw.nl/resource/${DATASET}.json?\$limit=1000"

python3 - "$TMP" "$OUT" <<'PY'
import json
import sys

raw_path, out_path = sys.argv[1], sys.argv[2]
with open(raw_path, encoding="utf-8") as fh:
    rows = json.load(fh)

table = {}
dropped = []
for row in rows:
    code = (row.get("code_toelichting_tellerstandoordeel") or "").strip()
    explanation = (row.get("toelichting_tellerstandoordeel") or "").strip()
    # A code with no explanation would render as a blank line under a confident
    # label. Drop it loudly rather than embedding an empty string.
    if not code or not explanation:
        dropped.append(row)
        continue
    table[code] = explanation

if dropped:
    print(f"warning: dropped {len(dropped)} row(s) with no code or no explanation",
          file=sys.stderr)
if len(table) < 8:
    raise SystemExit(f"refusing to write only {len(table)} codes; RDW returned {len(rows)} rows")

with open(out_path, "w", encoding="utf-8") as fh:
    json.dump(table, fh, ensure_ascii=False, sort_keys=True, indent=1)
    fh.write("\n")

print(f"wrote {len(table)} codes to {out_path}")
PY

#!/usr/bin/env bash
# Runs every check the project relies on: formatting, lints, generated-code
# freshness, the full test suite and the external corpus (if configured).
#
#   scripts/check.sh                     # standard checks
#   OPENXML_CORPUS=/path scripts/check.sh # also round-trip a local corpus
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> rustfmt"
cargo fmt --all -- --check

echo "==> clippy"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> generated code is up to date"
cargo run -q -p openxml-codegen -- --check

echo "==> tests"
OPENXML_REQUIRE_XMLLINT="${OPENXML_REQUIRE_XMLLINT:-0}" cargo test --workspace

if [[ -n "${OPENXML_CORPUS:-}" ]]; then
  echo "==> external corpus: $OPENXML_CORPUS"
  cargo test --release -p openxml-schema --test corpus -- --ignored --nocapture
fi
echo "all checks passed"

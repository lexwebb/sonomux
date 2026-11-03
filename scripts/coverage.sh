#!/usr/bin/env bash
# Sonomux Code Coverage Generator & Badge Synchronizer
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

COVERAGE_DIR="target/coverage"
TARGET_BOUNDARY="85.0"
STRICT_MODE=false

if [[ "${1:-}" == "--strict" || "${CI:-}" == "true" ]]; then
  STRICT_MODE=true
fi

rm -rf "${COVERAGE_DIR}"
mkdir -p "${COVERAGE_DIR}"

echo "🔨 Building instrumented test binaries..."
CARGO_INCREMENTAL=0 RUSTFLAGS="-C instrument-coverage" \
  cargo test --workspace --no-run --quiet

echo "🧪 Running tests and collecting profile data..."
TEST_BINS=()
while IFS= read -r bin; do
  if [ -x "$bin" ] && [ ! -d "$bin" ] && [[ "$bin" != *.so ]]; then
    TEST_BINS+=("$bin")
    LLVM_PROFILE_FILE="${COVERAGE_DIR}/test-%p-%m.profraw" "$bin" --quiet >/dev/null 2>&1 || true
  fi
done < <(find target/debug/deps -maxdepth 1 -type f -perm /111 \( -name "*test*" -o -name "sonomux_*" \) -not -name "*.so")

echo "📊 Merging profile data..."
llvm-profdata merge -sparse "${COVERAGE_DIR}"/*.profraw -o "${COVERAGE_DIR}/sonomux.profdata"

FIRST_BIN="${TEST_BINS[0]}"
OBJECT_ARGS=()
for ((i=1; i<${#TEST_BINS[@]}; i++)); do
  OBJECT_ARGS+=("--object=${TEST_BINS[$i]}")
done

echo ""
echo "📈 Code Coverage Summary:"
echo "================================================================================"

REPORT_OUTPUT=$(llvm-cov report \
  --instr-profile="${COVERAGE_DIR}/sonomux.profdata" \
  "${FIRST_BIN}" \
  "${OBJECT_ARGS[@]}" \
  --ignore-filename-regex="(/rustc/|/\.cargo/registry/|/tests/|/src/bin/|/main\.rs)")

echo "${REPORT_OUTPUT}"
echo "================================================================================"

# Extract line coverage percentage from the TOTAL row
COVERAGE_PCT=$(echo "${REPORT_OUTPUT}" | awk '/^TOTAL/ {gsub("%","",$10); print $10}')

if [[ -z "${COVERAGE_PCT}" ]]; then
  echo "⚠️ Could not parse coverage percentage from report output."
  exit 1
fi

ROUNDED_PCT=$(printf "%.0f" "${COVERAGE_PCT}")

# Determine badge color based on threshold (using awk for portable float comparison)
MEETS_TARGET=$(awk -v cov="${COVERAGE_PCT}" -v target="${TARGET_BOUNDARY}" 'BEGIN {print (cov >= target) ? 1 : 0}')
MEETS_ACCEPTABLE=$(awk -v cov="${COVERAGE_PCT}" 'BEGIN {print (cov >= 80.0) ? 1 : 0}')

if [[ "${MEETS_TARGET}" -eq 1 ]]; then
  COLOR="brightgreen"
  STATUS_MSG="✅ Coverage target met: ${COVERAGE_PCT}% (Soft boundary: >= ${TARGET_BOUNDARY}%)"
elif [[ "${MEETS_ACCEPTABLE}" -eq 1 ]]; then
  COLOR="yellow"
  STATUS_MSG="⚠️ Notice: Coverage ${COVERAGE_PCT}% is below the ${TARGET_BOUNDARY}% soft boundary (acceptable range)."
else
  COLOR="red"
  STATUS_MSG="❌ Warning: Coverage ${COVERAGE_PCT}% is critically low (< 80%)."
fi

echo "${STATUS_MSG}"

# Update README.md badge
README_FILE="${ROOT_DIR}/README.md"
if [ -f "${README_FILE}" ]; then
  sed -i -E "s|https://img.shields.io/badge/Coverage-[0-9]+%25-[a-z]+.svg|https://img.shields.io/badge/Coverage-${ROUNDED_PCT}%25-${COLOR}.svg|g" "${README_FILE}"
  sed -i -E "s|Coverage: [0-9]+%|Coverage: ${ROUNDED_PCT}%|g" "${README_FILE}"
  echo "📝 Synchronized README.md badge -> ${ROUNDED_PCT}% (${COLOR})"
fi

# Generate HTML report
HTML_DIR="${COVERAGE_DIR}/html"
llvm-cov show \
  --format=html \
  --output-dir="${HTML_DIR}" \
  --instr-profile="${COVERAGE_DIR}/sonomux.profdata" \
  "${FIRST_BIN}" \
  "${OBJECT_ARGS[@]}" \
  --ignore-filename-regex="(/rustc/|/\.cargo/registry/|/tests/|/src/bin/|/main\.rs)" >/dev/null 2>&1 || true

echo "🌐 HTML report: file://${ROOT_DIR}/${HTML_DIR}/index.html"

# If in strict mode and below target boundary, fail
if [[ "${STRICT_MODE}" == "true" ]] && [[ "${MEETS_TARGET}" -eq 0 ]]; then
  echo "❌ Strict mode failure: Coverage ${COVERAGE_PCT}% is below boundary ${TARGET_BOUNDARY}%."
  exit 1
fi

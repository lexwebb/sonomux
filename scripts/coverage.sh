#!/usr/bin/env bash
# Sonomux Code Coverage Generator & Badge Synchronizer
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

COVERAGE_DIR="target/coverage"
# Ratchet: raise this whenever coverage rises; never lower it.
TARGET_BOUNDARY="60.0"
STRICT_MODE=false

if [[ "${1:-}" == "--strict" || "${CI:-}" == "true" ]]; then
  STRICT_MODE=true
fi

# Ensure llvm tools are discovered (either from system or rustlib)
if ! command -v llvm-profdata &>/dev/null; then
  SYSROOT_BIN="$(rustc --print sysroot 2>/dev/null)/lib/rustlib/$(rustc -vV 2>/dev/null | sed -n 's/host: //p')/bin"
  if [ -d "$SYSROOT_BIN" ]; then
    export PATH="${SYSROOT_BIN}:${PATH}"
  fi
fi

# Not measured: process entry points that need a running GUI, AppKit or the daemon
# event loop. Keep logic out of these files so it counts.
UNTESTABLE_ENTRYPOINTS=(
  "crates/sonomux-gui/src/main.rs"      # Slint window + callbacks
  "crates/sonomux-tray/src/main.rs"     # tray process entry
  "crates/sonomux-tray/src/mac.rs"      # AppKit menu bar (main thread only)
  "crates/sonomux-daemon/src/main.rs"   # daemon event loop
)
ENTRY_RE=$(printf '|%s' "${UNTESTABLE_ENTRYPOINTS[@]}" | sed 's/\./\\./g')
# Generated code (Slint UI, skia bindings) lives under target/.
IGNORE_REGEX="(/rustc/|/\.cargo/registry/|\.rustup/|/target/|/tests/${ENTRY_RE})"

rm -rf "${COVERAGE_DIR}"
mkdir -p "${COVERAGE_DIR}"
rm -f "${ROOT_DIR}"/default_*.profraw

# Proc-macros and build scripts are instrumented too; send their profiles to a
# throwaway directory (absolute path: they run from varying working directories).
BUILD_PROFILE_DIR="${ROOT_DIR}/target/coverage-build"
rm -rf "${BUILD_PROFILE_DIR}"

echo "🔨 Building instrumented test binaries..."
TEST_BIN_LIST="${COVERAGE_DIR}/test-bins.txt"
CARGO_INCREMENTAL=0 RUSTFLAGS="-C instrument-coverage" \
  LLVM_PROFILE_FILE="${BUILD_PROFILE_DIR}/%p-%m.profraw" \
  cargo test --workspace --no-run --quiet --message-format=json \
  | python3 -c '
import json, sys
for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") == "compiler-artifact" and m.get("profile", {}).get("test") and m.get("executable"):
        print(m["executable"])
' | sort -u > "${TEST_BIN_LIST}"

echo "🧪 Running tests and collecting profile data..."
TEST_BINS=()
while IFS= read -r bin; do
  TEST_BINS+=("$bin")
  if ! LLVM_PROFILE_FILE="${COVERAGE_DIR}/test-%p-%m.profraw" "$bin" --quiet >/dev/null 2>&1; then
    echo "❌ Test binary failed: $bin (run it directly to see why)"
    exit 1
  fi
done < "${TEST_BIN_LIST}"

if [ "${#TEST_BINS[@]}" -eq 0 ]; then
  echo "❌ No test executables found."
  exit 1
fi
rm -rf "${BUILD_PROFILE_DIR}"

echo "📊 Merging profile data..."
llvm-profdata merge -sparse "${COVERAGE_DIR}"/*.profraw -o "${COVERAGE_DIR}/sonomux.profdata"

FIRST_BIN="${TEST_BINS[0]}"
OBJECT_ARGS=()
for ((i=1; i<${#TEST_BINS[@]}; i++)); do
  OBJECT_ARGS+=("--object=${TEST_BINS[$i]}")
done

echo "ℹ️  Not measured (entry points): ${UNTESTABLE_ENTRYPOINTS[*]}"
echo ""
echo "📈 Code Coverage Summary:"
echo "================================================================================"

REPORT_OUTPUT=$(llvm-cov report \
  --instr-profile="${COVERAGE_DIR}/sonomux.profdata" \
  "${FIRST_BIN}" \
  "${OBJECT_ARGS[@]}" \
  --ignore-filename-regex="${IGNORE_REGEX}")

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
  # -i.bak works with both GNU and BSD (macOS) sed.
  sed -i.bak -E \
    -e "s|https://img.shields.io/badge/Coverage-[0-9]+%25-[a-z]+.svg|https://img.shields.io/badge/Coverage-${ROUNDED_PCT}%25-${COLOR}.svg|g" \
    -e "s|Coverage: [0-9]+%|Coverage: ${ROUNDED_PCT}%|g" "${README_FILE}"
  rm -f "${README_FILE}.bak"
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
  --ignore-filename-regex="${IGNORE_REGEX}" >/dev/null 2>&1 || true

echo "🌐 HTML report: file://${ROOT_DIR}/${HTML_DIR}/index.html"

# If in strict mode and below target boundary, fail
if [[ "${STRICT_MODE}" == "true" ]] && [[ "${MEETS_TARGET}" -eq 0 ]]; then
  echo "❌ Strict mode failure: Coverage ${COVERAGE_PCT}% is below boundary ${TARGET_BOUNDARY}%."
  exit 1
fi

#!/usr/bin/env bash
# Sonomux Code Coverage Generator using LLVM Source-Based Coverage
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

COVERAGE_DIR="target/coverage"
rm -rf "${COVERAGE_DIR}"
mkdir -p "${COVERAGE_DIR}"

echo "🔨 Building instrumented test binaries..."
CARGO_INCREMENTAL=0 RUSTFLAGS="-C instrument-coverage" \
  cargo test --workspace --no-run --quiet

echo "🧪 Running tests and collecting profile data..."
# Run each compiled test binary directly with LLVM_PROFILE_FILE
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
llvm-cov report \
  --instr-profile="${COVERAGE_DIR}/sonomux.profdata" \
  "${FIRST_BIN}" \
  "${OBJECT_ARGS[@]}" \
  --ignore-filename-regex="(/rustc/|/\.cargo/registry/|/tests/|/src/bin/|/main\.rs)"

# Optional HTML export
HTML_DIR="${COVERAGE_DIR}/html"
llvm-cov show \
  --format=html \
  --output-dir="${HTML_DIR}" \
  --instr-profile="${COVERAGE_DIR}/sonomux.profdata" \
  "${FIRST_BIN}" \
  "${OBJECT_ARGS[@]}" \
  --ignore-filename-regex="(/rustc/|/\.cargo/registry/|/tests/|/src/bin/|/main\.rs)" >/dev/null 2>&1 || true

echo "================================================================================"
echo "HTML report generated at: file://${ROOT_DIR}/${HTML_DIR}/index.html"

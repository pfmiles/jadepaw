# jadepaw development task runner
# Run `just --list` to see all available recipes

default:
    @just --list

# Build the entire workspace (debug)
build:
    @cargo build --workspace

# Build the entire workspace (release)
build-release:
    @cargo build --workspace --release

# Run all tests (nextest + doc-tests)
test:
    @cargo nextest run --workspace && cargo test --doc --workspace

# Run unit and integration tests only (no doc-tests)
test-unit:
    @cargo nextest run --workspace

# Lint: clippy with zero-warnings policy
lint:
    @cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format all source files
fmt:
    @cargo fmt --all

# Check formatting (read-only, used in CI and pre-commit)
fmt-check:
    @cargo fmt --all -- --check

# Run cargo-deny license and security audit
deny:
    @cargo deny check

# Run cargo-audit vulnerability scan
audit:
    @cargo audit

# Pre-push check: format + lint + deny
check-all: fmt-check lint deny
    @echo "==> All checks passed."

# Build Wasm guest modules (placeholder — Wasm builds added in Phase 2)
wasm-build:
    @echo "Wasm builds added in Phase 2"

# Clean all build artifacts
clean:
    @cargo clean

# Build and open crate documentation
doc:
    @cargo doc --workspace --no-deps --document-private-items --open
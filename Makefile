# ──────────────────────────────────────────────────────────────
# MobKit — single source of truth for build / test / lint
# ──────────────────────────────────────────────────────────────

GREEN  := \033[0;32m
YELLOW := \033[0;33m
RED    := \033[0;31m
NC     := \033[0m

# ── meta ──────────────────────────────────────────────────────

.PHONY: all build release test test-python test-all lint fmt fmt-check \
        audit ci ci-smoke check doc doc-open coverage clean \
        install-hooks uninstall-hooks pre-commit-all update outdated \
        verify-version-parity bump-sdk-versions publish-dry-run-python \
        release-preflight help

all: ci

# ── build ─────────────────────────────────────────────────────

build: ## Build all workspace crates (debug)
	@echo "$(YELLOW)Building workspace (debug)…$(NC)"
	cargo build --workspace
	@echo "$(GREEN)Build succeeded.$(NC)"

release: ## Build all workspace crates (release)
	@echo "$(YELLOW)Building workspace (release)…$(NC)"
	cargo build --workspace --release
	@echo "$(GREEN)Release build succeeded.$(NC)"

# ── test ──────────────────────────────────────────────────────

test: ## Run Rust tests via cargo-nextest
	@echo "$(YELLOW)Running Rust tests…$(NC)"
	cargo nextest run --workspace -E 'not test(governance_contracts)' --no-fail-fast
	@echo "$(GREEN)Rust tests passed.$(NC)"

test-python: ## Run Python SDK tests
	@echo "$(YELLOW)Running Python SDK tests…$(NC)"
	PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests/ -q
	@echo "$(GREEN)Python SDK tests passed.$(NC)"

test-all: test test-python ## Run all tests (Rust + Python)

# ── lint / format ─────────────────────────────────────────────

lint: ## Run clippy with warnings-as-errors
	@echo "$(YELLOW)Running clippy…$(NC)"
	cargo clippy --workspace --all-targets -- -D warnings
	@echo "$(GREEN)Clippy passed.$(NC)"

fmt: ## Format all Rust code
	@echo "$(YELLOW)Formatting code…$(NC)"
	cargo fmt --all
	@echo "$(GREEN)Formatting complete.$(NC)"

fmt-check: ## Verify Rust formatting (CI)
	@echo "$(YELLOW)Checking formatting…$(NC)"
	cargo fmt --all -- --check
	@echo "$(GREEN)Formatting OK.$(NC)"

# ── audit / CI ────────────────────────────────────────────────

audit: ## Run cargo-deny licence / advisory checks
	@echo "$(YELLOW)Running cargo deny…$(NC)"
	cargo deny check
	@echo "$(GREEN)Audit passed.$(NC)"

ci: fmt-check verify-version-parity lint test-all audit ## Full CI pipeline
	@echo "$(GREEN)CI pipeline passed.$(NC)"

ci-smoke: fmt-check lint test test-python ## Quick smoke test (no audit / version parity)
	@echo "$(GREEN)CI smoke passed.$(NC)"

# ── misc cargo ────────────────────────────────────────────────

check: ## cargo check (fast compile check)
	@echo "$(YELLOW)Running cargo check…$(NC)"
	cargo check --workspace --all-targets
	@echo "$(GREEN)Check succeeded.$(NC)"

doc: ## Build rustdoc for all crates
	@echo "$(YELLOW)Building docs…$(NC)"
	cargo doc --workspace --no-deps
	@echo "$(GREEN)Docs built.$(NC)"

doc-open: ## Build and open rustdoc
	@echo "$(YELLOW)Building and opening docs…$(NC)"
	cargo doc --workspace --no-deps --open

coverage: ## Generate HTML coverage report (cargo-tarpaulin)
	@echo "$(YELLOW)Generating coverage report…$(NC)"
	cargo tarpaulin --workspace --timeout 120 --out Html
	@echo "$(GREEN)Coverage report generated.$(NC)"

clean: ## Remove build artefacts
	@echo "$(YELLOW)Cleaning…$(NC)"
	cargo clean
	@echo "$(GREEN)Clean complete.$(NC)"

# ── git hooks ─────────────────────────────────────────────────

install-hooks: ## Install pre-commit hooks
	@echo "$(YELLOW)Installing hooks…$(NC)"
	pre-commit install && pre-commit install --hook-type pre-push
	@echo "$(GREEN)Hooks installed.$(NC)"

uninstall-hooks: ## Uninstall pre-commit hooks
	@echo "$(YELLOW)Uninstalling hooks…$(NC)"
	pre-commit uninstall && pre-commit uninstall --hook-type pre-push
	@echo "$(GREEN)Hooks uninstalled.$(NC)"

pre-commit-all: ## Run pre-commit on all files
	@echo "$(YELLOW)Running pre-commit on all files…$(NC)"
	pre-commit run --all-files

# ── dependency management ─────────────────────────────────────

update: ## Update Cargo.lock to latest compatible versions
	@echo "$(YELLOW)Updating dependencies…$(NC)"
	cargo update
	@echo "$(GREEN)Dependencies updated.$(NC)"

outdated: ## Show outdated crates
	@echo "$(YELLOW)Checking for outdated crates…$(NC)"
	cargo outdated

# ── version / release ─────────────────────────────────────────

verify-version-parity: ## Verify version strings are in sync
	@scripts/verify-version-parity.sh

bump-sdk-versions: ## Bump SDK version strings
	@scripts/bump-sdk-versions.sh

verify-version: ## Verify Cargo.toml version matches git tag
	@VERSION=$$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "meerkat-mobkit") | .version'); \
	TAG=$$(git describe --tags --exact-match 2>/dev/null | sed 's/^v//'); \
	if [ -z "$$TAG" ]; then \
		echo "$(YELLOW)No tag found on current commit$(NC)"; \
	elif [ "$$VERSION" != "$$TAG" ]; then \
		echo "$(RED)Version mismatch: Cargo.toml has $$VERSION but tag is $$TAG$(NC)"; \
		exit 1; \
	else \
		echo "$(GREEN)Version $$VERSION matches tag$(NC)"; \
	fi

publish-dry-run: ## Dry-run cargo publish
	@echo "$(YELLOW)Dry-run cargo publish…$(NC)"
	cargo publish -p meerkat-mobkit --dry-run
	@echo "$(GREEN)Cargo dry-run succeeded.$(NC)"

publish-dry-run-python: ## Dry-run Python package build + twine check
	@echo "$(YELLOW)Building Python package (dry run)…$(NC)"
	@cd sdk/python && \
		python3 -m pip install --quiet build twine && \
		rm -rf dist && \
		python3 -m build && \
		python3 -m twine check dist/* && \
		rm -rf dist build *.egg-info
	@echo "$(GREEN)Python dry-run publish succeeded.$(NC)"

publish-dry-run-typescript: ## Dry-run TypeScript SDK build + npm pack
	@echo "$(YELLOW)Building TypeScript SDK (dry run)…$(NC)"
	@cd sdk/typescript && \
		npm install --ignore-scripts && \
		npm run build && \
		npm publish --access public --dry-run && \
		rm -rf dist
	@echo "$(GREEN)TypeScript dry-run publish succeeded.$(NC)"

release-preflight: ci ## Pre-release checks (full CI + CHANGELOG)
	@grep -q '\[Unreleased\]' CHANGELOG.md || \
		(echo "$(RED)CHANGELOG.md missing [Unreleased] section$(NC)" && exit 1)
	@echo "$(GREEN)Release preflight passed — ready to ship.$(NC)"

release-preflight-smoke: ci-smoke ## Smoke pre-release checks
	@grep -q '\[Unreleased\]' CHANGELOG.md || \
		(echo "$(RED)CHANGELOG.md missing [Unreleased] section$(NC)" && exit 1)
	@echo "$(GREEN)Smoke preflight passed.$(NC)"

release-dry-run: release-preflight publish-dry-run publish-dry-run-python publish-dry-run-typescript ## Full dry-run release (no uploads)
	@echo "$(GREEN)Full release dry-run passed.$(NC)"

release-dry-run-smoke: release-preflight-smoke publish-dry-run publish-dry-run-python publish-dry-run-typescript ## Smoke dry-run release
	@echo "$(GREEN)Smoke release dry-run passed.$(NC)"

# ── help ──────────────────────────────────────────────────────

help: ## Show this help
	@echo "$(GREEN)MobKit Makefile targets:$(NC)"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  $(YELLOW)%-24s$(NC) %s\n", $$1, $$2}'

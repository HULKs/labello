#!/usr/bin/env bash

set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

usage() {
    cat <<'EOF'
Usage: ./scripts/verify.sh <command> [comparison-base]

Commands:
  changed [base]  Run the required profile for changes since base plus local changes.
  ci <base>       CI alias for changed; requires an explicit pull-request base SHA.
  baseline        Run the required non-browser repository baseline.
  all             Run the baseline and the release browser build.
  docs            Run the documentation-only checks.
  audit           Audit workflow/docs synchronization and path classification.
  classify [base] Print the risk profiles selected for the changed paths.
EOF
}

run() {
    printf '+ '
    printf '%q ' "$@"
    printf '\n'
    "$@"
}

require_literal() {
    local file="$1"
    local literal="$2"

    if ! rg --fixed-strings --quiet "$literal" "$file"; then
        printf 'quality-gate audit: %s must contain: %s\n' "$file" "$literal" >&2
        return 1
    fi
}

profiles_for_path() {
    local path="$1"

    case "$path" in
        *.md|LICENSE|docs/*|.agents/*|.github/pull_request_template.md)
            printf '%s\n' docs
            ;;
        crates/labello-domain/*)
            printf '%s\n' domain
            ;;
        crates/labello-storage/*)
            printf '%s\n' storage
            ;;
        crates/labello-api/*|apps/labello-server/*|labello.server.example.toml)
            printf '%s\n' api
            ;;
        crates/labello-client/*)
            printf '%s\n' api browser
            ;;
        crates/labello-ui/*|assets/*)
            printf '%s\n' ui browser
            ;;
        apps/labello-wasm/*)
            printf '%s\n' browser
            ;;
        apps/egui-mcp-inspector/*|opencode.json)
            printf '%s\n' ui
            ;;
        Cargo.toml|Cargo.lock|.gitignore|scripts/*|.github/workflows/*)
            printf '%s\n' infrastructure browser
            ;;
        *)
            return 1
            ;;
    esac
}

default_comparison_base() {
    if git rev-parse --verify --quiet origin/main >/dev/null; then
        printf '%s\n' origin/main
    elif git rev-parse --verify --quiet main >/dev/null; then
        printf '%s\n' main
    elif git rev-parse --verify --quiet HEAD^ >/dev/null; then
        printf '%s\n' HEAD^
    else
        printf '%s\n' HEAD
    fi
}

changed_paths() {
    local base="$1"

    if ! git rev-parse --verify --quiet "$base^{commit}" >/dev/null; then
        printf 'verification base is not a commit: %s\n' "$base" >&2
        return 1
    fi

    {
        git diff --name-only --diff-filter=ACMRTUXB "$base...HEAD"
        git diff --name-only --diff-filter=ACMRTUXB
        git diff --cached --name-only --diff-filter=ACMRTUXB
        git ls-files --others --exclude-standard
    } | sed '/^$/d' | sort -u
}

classify_changes() {
    local base="$1"
    local path
    local path_profiles
    local paths

    paths="$(changed_paths "$base")"
    if [[ -z "$paths" ]]; then
        printf 'No changed paths found relative to %s.\n' "$base"
        printf '%s\n' baseline
        return
    fi

    while IFS= read -r path; do
        if ! path_profiles="$(profiles_for_path "$path")"; then
            printf 'quality-gate classification: unclassified path: %s\n' "$path" >&2
            return 1
        fi
        printf '%s\n' "$path_profiles"
    done <<<"$paths" | sort -u
}

audit() {
    run bash -n scripts/verify.sh
    run git diff --check

    require_literal README.md './scripts/verify.sh changed origin/main'
    require_literal CONTRIBUTING.md './scripts/verify.sh changed origin/main'
    require_literal AGENTS.md './scripts/verify.sh changed origin/main'
    require_literal docs/verification.md './scripts/verify.sh changed origin/main'
    require_literal .github/workflows/quality-gate.yml './scripts/verify.sh ci "$QUALITY_BASE_SHA"'
    require_literal .github/workflows/quality-gate.yml 'name: Quality gate / Canonical verification'
    require_literal docs/verification.md 'Quality gate / Canonical verification'
    require_literal .github/pull_request_template.md '## Acceptance criteria and evidence'

    local baseline_command
    while IFS= read -r baseline_command; do
        require_literal scripts/verify.sh "$baseline_command"
        require_literal docs/verification.md "$baseline_command"
    done <<'EOF'
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo test --locked -p labello-ui --features inspector-presets
cargo check --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml
cargo check --locked -p labello-wasm --target wasm32-unknown-unknown
trunk build --release --locked
EOF

    local tracked_path
    while IFS= read -r tracked_path; do
        if ! profiles_for_path "$tracked_path" >/dev/null; then
            printf 'quality-gate audit: tracked path has no risk profile: %s\n' "$tracked_path" >&2
            return 1
        fi
    done < <(git ls-files)
}

docs_checks() {
    audit
    printf '%s\n' 'Documentation-only profile: complete the content, local-link, and anchor review recorded in the handoff.'
}

baseline() {
    audit
    run cargo fmt --all -- --check
    run cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    run cargo test --locked --workspace --all-features
    run cargo test --locked -p labello-ui --features inspector-presets
    run cargo check --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml
    run cargo check --locked -p labello-wasm --target wasm32-unknown-unknown
}

browser_build() {
    (
        cd apps/labello-wasm
        # Trunk 0.21.14 parses NO_COLOR as a boolean and rejects the standard
        # presence-only value used by some shells and automation harnesses.
        run env -u NO_COLOR trunk build --release --locked
    )
}

changed() {
    local base="$1"
    local profiles

    profiles="$(classify_changes "$base")"
    printf 'Selected risk profiles relative to %s:\n%s\n' "$base" "$profiles"

    if [[ "$profiles" == docs ]]; then
        docs_checks
        return
    fi

    baseline
    if grep -qx browser <<<"$profiles"; then
        browser_build
    fi

    printf '%s\n' 'Machine checks passed. Complete the profile-specific review and evidence in docs/verification.md.'
}

command="${1:-}"
case "$command" in
    changed)
        changed "${2:-$(default_comparison_base)}"
        ;;
    ci)
        if [[ $# -ne 2 ]]; then
            usage >&2
            exit 2
        fi
        changed "$2"
        ;;
    baseline)
        baseline
        ;;
    all)
        baseline
        browser_build
        ;;
    docs)
        docs_checks
        ;;
    audit)
        audit
        ;;
    classify)
        classify_changes "${2:-$(default_comparison_base)}"
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac

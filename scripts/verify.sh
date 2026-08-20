#!/usr/bin/env bash

set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

test_runner="cargo"

usage() {
    cat <<'EOF'
Usage: ./scripts/verify.sh <command> [comparison-base] [ci-stage]

Commands:
  changed [base]  Run the required profile for changes since base plus local changes.
  ci <base>       Run the complete CI profile sequentially with cargo-nextest.
  ci <base> <stage>
                  Run one CI stage: audit, plan, format, clippy,
                  workspace-tests, ui-tests, doctests, inspector, wasm, or browser.
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

    if ! grep -Fq -- "$literal" "$file"; then
        printf 'ci audit: %s must contain: %s\n' "$file" "$literal" >&2
        return 1
    fi
}

reject_literal() {
    local file="$1"
    local literal="$2"

    if grep -Fq -- "$literal" "$file"; then
        printf 'ci audit: %s must not contain: %s\n' "$file" "$literal" >&2
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
        Cargo.toml|Cargo.lock|.gitignore|scripts/*|.github/workflows/*|deployment/*|tools/*)
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
            printf 'ci classification: unclassified path: %s\n' "$path" >&2
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
    require_literal .github/workflows/ci.yml './scripts/verify.sh ci "$CI_BASE_SHA" plan'
    require_literal .github/workflows/ci.yml 'name: Testing'
    require_literal .github/workflows/ci.yml 'if: always()'
    require_literal .github/workflows/ci.yml 'require_result browser "$BROWSER_REQUIRED" "$BROWSER_RESULT"'
    require_literal .github/workflows/ci.yml 'uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6'
    require_literal .github/workflows/ci.yml 'apps/egui-mcp-inspector -> target'
    require_literal .github/workflows/ci.yml 'cache-bin: false'
    require_literal .github/workflows/ci.yml 'uses: jetli/trunk-action@1346cc09eace4beb84e403e199a471346d4684c9'
    require_literal .github/workflows/ci.yml 'version: v0.21.14'
    require_literal .github/workflows/ci.yml 'uses: taiki-e/install-action@5b4d68e2e660441203ab128a23676f1e4faf1532'
    require_literal .github/workflows/ci.yml 'tool: cargo-nextest@0.9.143'
    require_literal .github/workflows/ci.yml 'fallback: none'
    require_literal docs/verification.md 'required `Testing` status check'
    require_literal .github/pull_request_template.md '## Acceptance criteria and evidence'
    require_literal .github/workflows/release.yml 'workflow_dispatch:'
    require_literal .github/workflows/release.yml "if: github.ref == 'refs/heads/main'"
    require_literal .github/workflows/release.yml 'group: Default'
    require_literal .github/workflows/release.yml 'labels: [self-hosted, linux, x64]'
    reject_literal .github/workflows/release.yml 'LABELLO_RELEASE_RUNNER'
    require_literal .github/workflows/release.yml 'test "$GITHUB_SHA" = "$source_commit"'
    require_literal .github/workflows/release.yml 'actions/attest-build-provenance@977bb373ede98d70efdf65b84cb5f73e068dcc2a'
    require_literal .github/workflows/release.yml 'isImmutable == true'
    require_literal .github/workflows/release.yml 'event_type=stable-release-published'
    require_literal .github/workflows/deploy.yml 'repository_dispatch:'
    require_literal .github/workflows/deploy.yml 'gh_2.97.0_linux_amd64.tar.gz'
    require_literal .github/workflows/deploy.yml 'a2c9b8497e1f85b1ad0dfcb78b5a622e098801b8e461e459e88e1ee12f018112'
    require_literal .github/workflows/deploy.yml '/var/lib/labello/bin/labello-deploy receive "$request_id"'
    require_literal .github/workflows/deploy.yml '/var/lib/labello/bin/labello-deploy status "$request_id"'
    reject_literal .github/workflows/deploy.yml 'ssh'
    reject_literal .github/workflows/deploy.yml 'sudo'
    reject_literal .github/workflows/deploy.yml 'apt-get'
    reject_literal .github/workflows/deploy.yml 'container:'
    reject_literal .github/workflows/deploy.yml 'python'
    reject_literal .github/workflows/deploy.yml 'Python'
    require_literal docs/deployment.md 'Routine deployment is rootless.'
    require_literal docs/deployment.md '`candidate_data_access_started`'
    require_literal docs/deployment.md '`admission_started`'

    local baseline_command
    while IFS= read -r baseline_command; do
        require_literal scripts/verify.sh "$baseline_command"
        require_literal docs/verification.md "$baseline_command"
    done <<'EOF'
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features --exclude labello-ui
cargo test --locked -p labello-ui --all-features
cargo check --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml
cargo check --locked -p labello-wasm --target wasm32-unknown-unknown
trunk build --release --locked
EOF

    local ci_test_command
    while IFS= read -r ci_test_command; do
        require_literal scripts/verify.sh "$ci_test_command"
        require_literal docs/verification.md "$ci_test_command"
    done <<'EOF'
cargo nextest run --locked --workspace --all-features --exclude labello-ui
cargo nextest run --locked -p labello-ui --all-features
cargo test --locked --workspace --all-features --doc
EOF

    local ci_stage
    while IFS= read -r ci_stage; do
        require_literal .github/workflows/ci.yml "./scripts/verify.sh ci \"\$CI_BASE_SHA\" $ci_stage"
    done <<'EOF'
audit
plan
format
clippy
workspace-tests
ui-tests
doctests
inspector
wasm
browser
EOF

    local tracked_path
    while IFS= read -r tracked_path; do
        if ! profiles_for_path "$tracked_path" >/dev/null; then
            printf 'ci audit: tracked path has no risk profile: %s\n' "$tracked_path" >&2
            return 1
        fi
    done < <(git ls-files)
}

docs_checks() {
    audit
    printf '%s\n' 'Documentation-only profile: complete the content, local-link, and anchor review recorded in the handoff.'
}

format_check() {
    run cargo fmt --all -- --check
}

clippy_check() {
    run cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
}

workspace_tests() {
    case "$test_runner" in
        cargo)
            run cargo test --locked --workspace --all-features --exclude labello-ui
            ;;
        nextest)
            run cargo nextest run --locked --workspace --all-features --exclude labello-ui
            ;;
        *)
            printf 'unsupported verification test runner: %s\n' "$test_runner" >&2
            return 1
            ;;
    esac
}

ui_tests() {
    case "$test_runner" in
        cargo)
            run cargo test --locked -p labello-ui --all-features
            ;;
        nextest)
            run cargo nextest run --locked -p labello-ui --all-features
            ;;
        *)
            printf 'unsupported verification test runner: %s\n' "$test_runner" >&2
            return 1
            ;;
    esac
}

doctests() {
    # Nextest does not execute rustdoc test binaries on stable Rust.
    run cargo test --locked --workspace --all-features --doc
}

inspector_check() {
    run cargo check --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml
}

wasm_check() {
    run cargo check --locked -p labello-wasm --target wasm32-unknown-unknown
}

baseline() {
    audit
    format_check
    clippy_check
    workspace_tests
    ui_tests
    if [[ "$test_runner" == nextest ]]; then
        doctests
    fi
    inspector_check
    wasm_check
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

ci_plan() {
    local base="$1"
    local profiles
    local baseline_required=true
    local browser_required=false

    profiles="$(classify_changes "$base")"
    if [[ "$profiles" == docs ]]; then
        baseline_required=false
    fi
    if grep -qx browser <<<"$profiles"; then
        browser_required=true
    fi

    printf 'baseline=%s\n' "$baseline_required"
    printf 'browser=%s\n' "$browser_required"
}

ci_stage() {
    local base="$1"
    local stage="$2"

    test_runner="nextest"
    case "$stage" in
        audit)
            audit
            ;;
        plan)
            ci_plan "$base"
            ;;
        format)
            format_check
            ;;
        clippy)
            clippy_check
            ;;
        workspace-tests)
            workspace_tests
            ;;
        ui-tests)
            ui_tests
            ;;
        doctests)
            doctests
            ;;
        inspector)
            inspector_check
            ;;
        wasm)
            wasm_check
            ;;
        browser)
            browser_build
            ;;
        *)
            printf 'unsupported CI verification stage: %s\n' "$stage" >&2
            usage >&2
            return 2
            ;;
    esac
}

command="${1:-}"
case "$command" in
    changed)
        changed "${2:-$(default_comparison_base)}"
        ;;
    ci)
        if [[ $# -lt 2 || $# -gt 3 ]]; then
            usage >&2
            exit 2
        fi
        if [[ $# -eq 3 ]]; then
            ci_stage "$2" "$3"
        else
            test_runner="nextest"
            changed "$2"
        fi
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

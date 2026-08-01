# scarce-studio task runner. CI calls these recipes — never raw cargo.

default:
    @just --list

build:
    cargo build --workspace

# Format check (CI) — use `just fmt-fix` to apply.
fmt:
    cargo fmt --all --check

fmt-fix:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

ci: fmt lint test

run:
    cargo run --bin scarced

# Install a target: `just install scarce [cargo install args...]`
[positional-arguments]
install *args:
    #!/usr/bin/env bash
    set -euo pipefail

    if [ "$#" -eq 0 ]; then
        echo "Usage: just install scarce [cargo install args...]"
        exit 1
    fi
    target="$1"
    shift

    case "${target}" in
        scarce)
            if [ "$#" -gt 0 ]; then
                cargo install "$@"
            else
                cargo install --path . --locked
            fi
            ;;
        *)
            echo "Unknown target: ${target}"
            echo "Usage: just install scarce [cargo install args...]"
            exit 1
            ;;
    esac

# Regenerate schemas/*.json from the studio-types derives. CI fails (drift
# test) when a type change lands without re-running this.
schemas:
    cargo run -p studio-types --bin gen-schemas

# Env-gated integration suites (real relay / devnet) land in M3/M5.
# Kept separate from `ci` by design: nightly, not per-push (PLAN.md §4).
integration-test:
    @echo "no integration suites yet (arrives in M3: SCARCE_RELAY_TESTS=1)"

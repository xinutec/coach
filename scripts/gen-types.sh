#!/usr/bin/env bash
# Generate the frontend TS interfaces from the Rust API types via ts-rs, so the
# backend↔frontend wire shapes are consistent by construction, not transcribed.
#
#   nix develop --command scripts/gen-types.sh            # regenerate + install
#   nix develop --command scripts/gen-types.sh --check    # report drift, write nothing
#
# The second form is what the gate's "generated types are current" row runs, so
# the cargo invocation below is stated once and both paths use it.
#
# This file holds only the part that is coach's: where the bindings live and how
# to make cargo emit them. The rest — generate into a scratch directory and
# install only on success, refuse a generation that emitted nothing, copy the
# types and not whatever else landed beside them, compare by content rather than
# by asking git — is dev-lint#gen-types, shared across the fleet.
#
# `--features ts` turns on ts-rs (off by default: normal builds carry none and
# the pacing core stays no_std). `--workspace` so the pacing core's own
# #[ts(export)] types — PacingInput, the domain enums — export alongside coach's.
# The export tests are named export_bindings_*, so the filter runs generation
# only and needs no database.
set -euo pipefail
cd "$(dirname "$0")/.."

# Pinned to dev-lint's committed HEAD, like every gate row that reaches for a
# dev-lint tool: a path flake builds the NEIGHBOUR'S WORKING TREE, so a session
# mid-edit next door fails this row, and the row names this repository. The
# reasoning is written out once, at `withTestDb` in dev-lint/gate/schema.dhall.
exec nix run "git+file:../dev-lint?ref=HEAD#gen-types" -- "$@" \
  --out frontend/src/app/generated \
  -- cargo test --workspace --features ts export_bindings

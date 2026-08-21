#!/usr/bin/env bash
# SPDX-FileCopyrightText: Copyright 2026 NVIDIA CORPORATION & AFFILIATES
# SPDX-License-Identifier: Apache-2.0

# Install and verify the exact release analyzers selected by repository policy.

set -euo pipefail

# The approved bootstrap Action must provide the reviewed cargo-binstall build.
cargo_binstall_version="$(cargo-binstall --version)"
# Do not analyze release history with an unexpected installer implementation.
if [[ "${cargo_binstall_version}" != "cargo-binstall 1.20.1" ]]; then
  echo "Expected cargo-binstall 1.20.1; found ${cargo_binstall_version}." >&2
  exit 1
fi

# Install both analyzers from their locked crate graphs before release analysis.
cargo binstall \
  --force \
  --locked \
  --no-confirm \
  --strategies=crate-meta-data,compile \
  release-plz@0.3.160 \
  cargo-semver-checks@0.48.0

# Bind every release-plz invocation in later steps to the reviewed CLI version.
release_plz_version="$(release-plz --version)"
# Refuse to continue when the release transaction analyzer is not exact.
if [[ "${release_plz_version}" != "release-plz 0.3.160" ]]; then
  echo "Expected release-plz 0.3.160; found ${release_plz_version}." >&2
  exit 1
fi

# Bind release-plz's compatibility analysis to the reviewed helper version.
semver_checks_version="$(cargo semver-checks --version)"
# Refuse to continue when compatibility analysis would use another version.
if [[ "${semver_checks_version}" != "cargo-semver-checks 0.48.0" ]]; then
  echo "Expected cargo-semver-checks 0.48.0; found ${semver_checks_version}." >&2
  exit 1
fi

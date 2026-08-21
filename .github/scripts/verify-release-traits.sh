#!/usr/bin/env bash
# SPDX-FileCopyrightText: Copyright 2026 NVIDIA CORPORATION & AFFILIATES
# SPDX-License-Identifier: Apache-2.0

# Require one exact yaml-sigil-traits package from the named crates.io index.

set -euo pipefail

expected_source="registry+https://github.com/rust-lang/crates.io-index"
dependency_records="$(
  cargo metadata --no-deps --format-version 1 \
    | jq --compact-output \
      '[.packages[].dependencies[]
        | select(.name == "yaml-sigil-traits")
        | {req, source, registry, rename}] | unique'
)"
dependency_count="$(jq 'length' <<<"${dependency_records}")"
# Multiple declarations could resolve the same crate name from distinct sources.
if [[ "${dependency_count}" != "1" ]]; then
  echo "Expected one exact yaml-sigil-traits dependency identity." >&2
  exit 1
fi

requirement="$(jq --raw-output '.[0].req' <<<"${dependency_records}")"
source="$(jq --raw-output '.[0].source' <<<"${dependency_records}")"
registry="$(jq --raw-output '.[0].registry' <<<"${dependency_records}")"
rename="$(jq --raw-output '.[0].rename' <<<"${dependency_records}")"
# The workspace must use an exact SemVer requirement with no renamed identity.
if [[ ! "${requirement}" =~ ^=[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ \
  || "${source}" != "${expected_source}" \
  || "${registry}" != "null" \
  || "${rename}" != "null" ]]; then
  echo "yaml-sigil-traits must use one exact crates.io package identity." >&2
  exit 1
fi
traits_version="${requirement#=}"

# The exact split-crate dependency must already exist and remain non-yanked.
bash .github/scripts/verify-crates-io-packages.sh \
  --check-version "${traits_version}" yaml-sigil-traits
# Named-index resolution prevents an implicit alternate registry configuration.
cargo info --quiet --registry crates-io \
  "yaml-sigil-traits@${traits_version}" >/dev/null

resolved="$(
  cargo metadata --format-version 1 \
    | jq --compact-output \
      '[.packages[]
        | select(.name == "yaml-sigil-traits")
        | {version, source}]
      | unique'
)"
# Cargo's complete resolved graph must contain exactly that registry package.
if ! jq --exit-status \
  --arg version "${traits_version}" \
  --arg source "${expected_source}" \
  'length == 1 and .[0].version == $version and .[0].source == $source' \
  <<<"${resolved}" >/dev/null; then
  echo "Cargo did not resolve the exact yaml-sigil-traits crates.io source." >&2
  exit 1
fi

echo "Verified yaml-sigil-traits ${traits_version} from the named crates.io index."

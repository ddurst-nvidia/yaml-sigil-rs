# Release the YamlSigil Rust crates

This repository publishes these crates.io `.crate` source packages as one
versioned release transaction:

- `yaml-sigil-core`;
- `yaml-sigil-transcription`;
- `yaml-sigil-signing`; and
- `yaml-sigil-verification`.

Official publications also create one version tag and source-only GitHub
Release per crate from its reviewed changelog. Neither release-plz nor any
other release step builds or attaches binary assets. The workflow retains no
build artifacts or separately generated archives. GitHub's automatic source
archives are expected. Keep the workspace default, conformance, test-key, and
xtask packages unpublished.

Cargo disables implicit binary targets in every publishable crate, and release
validation rejects an explicit binary target. Do not distribute compiled
executables from this repository.

## Release authorization

The `Release proposal` workflow owns the `release-plz-next` branch and opens or
updates its pull request against `main`. Release-plz analyzes Conventional
Commits and generates each crate's changelog content. The repository xtask
applies the RC policy and synchronizes the workspace versions. A
least-privilege GitHub App creates the commit through GitHub, so GitHub reports
the commit as Verified. The commit also includes a DCO trailer for the App bot
identity.

Do not add human commits to `release-plz-next`. The workflow refuses to replace
the branch if it contains a commit by another identity. Review and merge the
release pull request through the normal protected-branch path. Its exact head
must pass `Required CI`, including all three Rust platform jobs. Merging that
pull request is the authorization signal for release-plz because
`.release-plz.toml` sets `release_always = false` and the branch uses the
`release-plz-` prefix.

The workflow remains a successful no-op while the GitHub App configuration is
absent. It also waits without advancing the train until the shared version on
`main` is available and non-yanked for all four crates on crates.io.

Release proposals enter `protected-automation`, which is restricted to exact
`main` and supplies only the App credential. Official publication enters
`crates-io`, whose configured approval gates the OIDC-enabled publication job.
Validation enters neither environment and receives no OIDC permission.

Every proposal resolves one workspace comparison baseline from the last
official annotated four-tag release. All four tags must match origin, resolve
to one commit, and be ancestors of current remote `main`; the exact shared
version must be non-yanked for all four crates. Registry prereleases that have
no complete official tag set never become release-analysis baselines.

### Manual release-proposal fallback

> [!IMPORTANT]
> This fallback changes proposal authorship only. It does not authorize local
> publication, a crates.io token, a protected-environment bypass, or binary
> artifacts. Official publication still uses the protected Trusted Publishing
> workflow.

Use this procedure when the App is unavailable or cannot safely update its
owned proposal. A repository writer may prepare the same release transaction
on a human-authored branch. Use cargo-binstall `1.20.1`, release-plz `0.3.160`,
and cargo-semver-checks `0.48.0`. Create a same-repository branch named
`release-plz-manual-<target>` from exact current `main`; do not reuse the
workflow-owned `release-plz-next` branch.

Before creating the manual branch, inspect any existing `release-plz-next`
proposal. Do not append a human commit to it or replace its App-owned head.
Finish or close that proposal and verify current `main` and crates.io state, or
leave it intact while using the distinctly named manual branch. Do not run the
two proposal paths concurrently.

Before either proposal mode, fetch current main and tags, verify the analyzer
versions and split-crate identity, and prepare the detached official baseline:

```shell
git fetch origin main --tags
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)"
test "$(cargo-binstall --version)" = "cargo-binstall 1.20.1"
bash .github/scripts/install-release-tools.sh
published_version="$(cargo xtask release-version show)"
bash .github/scripts/verify-crates-io-packages.sh \
  --check-version "${published_version}" \
  yaml-sigil-core \
  yaml-sigil-transcription \
  yaml-sigil-signing \
  yaml-sigil-verification
CARGO_REGISTRIES_CRATES_IO_INDEX=https://github.com/rust-lang/crates.io-index \
  bash .github/scripts/verify-release-traits.sh
baseline_parent="$(mktemp -d)"
baseline_root="${baseline_parent}/official-release"
fetch_url="$(git remote get-url origin)"
GIT_CONFIG_COUNT=1 \
GIT_CONFIG_KEY_0=remote.origin.pushurl \
GIT_CONFIG_VALUE_0=disabled://yaml-sigil-release-proposal \
  python3 .github/scripts/prepare_release_baseline.py \
    --repository NVIDIA/yaml-sigil-rs \
    --version "${published_version}" \
    --head "$(git rev-parse HEAD)" \
    --output "${baseline_root}" \
    --expected-fetch-url "${fetch_url}"
registry_manifest_path="${baseline_root}/Cargo.toml"
```

Stop if current main, a registry record, traits identity, tag type, tag target,
tag agreement, ancestry, or remote ref differs. For the next substantive RC
proposal, set the reviewed intent and run:

```shell
release_date="$(date -u +%F)"
bump="auto"
# Generate each Conventional Commit changelog and preliminary version change.
GIT_CONFIG_COUNT=1 \
GIT_CONFIG_KEY_0=remote.origin.pushurl \
GIT_CONFIG_VALUE_0=disabled://yaml-sigil-release-proposal \
  release-plz update \
    --config .release-plz.toml \
    --registry-manifest-path "${registry_manifest_path}"
git diff --name-only -- \
  crates/yaml-sigil-core/CHANGELOG.md \
  crates/yaml-sigil-transcription/CHANGELOG.md \
  crates/yaml-sigil-signing/CHANGELOG.md \
  crates/yaml-sigil-verification/CHANGELOG.md
```

The command must list at least one expected crate changelog as changed. If it
does not, stop before advancing the version: a manual proposal must not create
an empty seed. Once an expected changelog change is present, complete the
candidate transaction; the xtask adds a section for every crate so the shared
version remains a single release transaction:

```shell
cargo xtask release-version candidate \
  --published "${published_version}" \
  --bump "${bump}" \
  --date "${release_date}" \
  --release-notes
cargo xtask sync-workspace-versions
cargo xtask sync-workspace-versions --check
```

Leave `bump` as `auto` unless review explicitly requires a `patch`, `minor`, or
`major` version-line advance. Never run per-package baseline generation or
infer the baseline from a higher registry prerelease.

For stable promotion, use the same baseline preparation and require all four
tag commits to equal exact current `main`. Then create the manual branch and
run:

```shell
release_date="$(date -u +%F)"
for crate in core transcription signing verification; do
  test "$(git rev-parse "yaml-sigil-${crate}-v${published_version}^{commit}")" \
    = "$(git rev-parse HEAD)"
done
cargo xtask release-version promote-stable --date "${release_date}"
cargo xtask sync-workspace-versions
cargo xtask sync-workspace-versions --check
```

Stable promotion also removes `rc.N` from the exact split-crate requirement,
changing `yaml-sigil-traits = "=X.Y.Z-rc.N"` to `"=X.Y.Z"`. It preserves an
already-stable exact pin and rejects any other prerelease form.

For either path, review the generated transaction and run:

```shell
cargo xtask release-version check
cargo xtask ci
CARGO_REGISTRIES_CRATES_IO_INDEX=https://github.com/rust-lang/crates.io-index \
  bash .github/scripts/verify-release-traits.sh
bash .github/scripts/check-release-packages.sh \
  yaml-sigil-core \
  yaml-sigil-transcription \
  yaml-sigil-signing \
  yaml-sigil-verification
git diff --check
git status --short
```

The complete diff must contain only the intended root `Cargo.toml` and four
crate changelogs. It must include every version and internal dependency change
from `cargo xtask sync-workspace-versions` and, for stable promotion, the exact
stable `yaml-sigil-traits` requirement. The release-package helper is the Cargo
metadata, ordered-package, and library-only gate. Do not commit a generated
`Cargo.lock` or package archive. Commit the complete transaction with an SSH
signature and DCO sign-off, then confirm that the exact commit leaves the
worktree clean. Push the branch and open its pull request against `main`.
The pull request association is required for a useful release-plz dry run
because `release_always = false` authorizes only commits from a
`release-plz-*` branch. The validation-only Cargo home patches the unpublished
implementation crates to their reviewed workspace paths; never use it for
publication. After the pull request exists, run:

```shell
validation_cargo_home="$(mktemp -d)"
bash .github/scripts/prepare-unpublished-workspace-dependencies.sh \
  "${validation_cargo_home}"
# Use the existing gh credential only for read-only forge discovery.
CARGO_HOME="${validation_cargo_home}" \
  CARGO_REGISTRIES_CRATES_IO_INDEX=https://github.com/rust-lang/crates.io-index \
  GIT_TOKEN="$(gh auth token)" \
  release-plz release --dry-run --forge github --config .release-plz.toml
git status --short
```

The process-scoped `GIT_TOKEN` must not be echoed, pasted, or persisted. Verify
that the dry run plans all four crates in dependency order and does not report
that the current commit is not from a release pull request. It must not publish
or create tags or GitHub Releases.

If either `release-plz-next` or the selected manual branch already exists,
inspect its owner, exact ref, open pull request, and commits before proceeding.
Never overwrite a foreign or unexpected branch. Resolve the collision by
finishing, closing, or deliberately renaming the human branch, then rerun all
main, tag, registry, traits, and baseline checks.

Review and integrate the exact head through the ordinary protected path only
after `Required CI` and all three Rust platform jobs pass. If a repair is
needed, amend the signed commit while retaining one DCO trailer, force-push
with lease, and repeat the clean-commit and dry-run checks. Merging that
`release-plz-*` pull request is the authorization signal for the protected
official publication workflow, whose `validate` job confirms all four crates
again before publishing them in dependency order. Do not run a local
non-dry-run release command.

After the manual proposal is integrated or closed, delete only its manual
branch, confirm the exact current `main` and crates.io state, and dispatch a
fresh `Release proposal` run. Let the workflow recreate or update its own
branch from that state. Do not copy the human-authored commit onto
`release-plz-next`.

## RC progression and synchronized versions

The default release progression is:

- a published stable `MAJOR.MINOR.PATCH` starts the next patch train as
  `MAJOR.MINOR.(PATCH+1)-rc.1`;
- a published `MAJOR.MINOR.PATCH-rc.N` advances to
  `MAJOR.MINOR.PATCH-rc.(N+1)`; and
- release-plz updates the active proposal when later Conventional Commits add
  release notes or imply a discoverable version-line change.

An empty next-version seed remains a draft pull request. A proposal with
release notes is marked ready for review.

When the required `major`, `minor`, or `patch` advance is not discoverable from
the commits, a repository writer can dispatch `Release proposal` with mode
`next-candidate` and the intended bump. The workflow records that override in
the pull-request body and retains it across later automatic updates. Dispatch
the same mode with `auto` to clear the override.

All four crates use `[workspace.package].version`. Every official RC or stable
proposal runs both commands, and commits their complete result on the release
branch:

```shell
cargo xtask sync-workspace-versions
cargo xtask sync-workspace-versions --check
```

Never change one member version independently. Official publication rejects an
unsynchronized or dirty source tree.

## Promote an RC to stable

Stable promotion is an explicit review operation. First publish and verify all
four RC crates from `main`. Every `yaml-sigil-<crate>-vMAJOR.MINOR.PATCH-rc.N`
tag must resolve to the exact current `main` commit. Then dispatch
`Release proposal` with mode `promote-stable`. The workflow creates a pull
request that removes the prerelease component, synchronizes internal dependency
requirements, promotes the exact `yaml-sigil-traits` RC requirement to its
stable version, and copies each reviewed RC changelog section to the stable
version. The release-version check rejects a stable implementation workspace
that retains a prerelease traits requirement.

Review and merge that exact proposal before publishing the stable version. Do
not edit a contributor branch to remove `rc.N`, and do not promote source that
differs from the tagged RC.

## Validate an official release

Before validation or publication, confirm:

- the required `yaml-sigil-traits` version is available on crates.io;
- `main` is the exact merged head of the intended `release-plz-*` pull request;
- the head is GitHub Verified, DCO-compliant, and green under required and
  platform CI;
- all four manifests, dependency requirements, and changelogs are synchronized;
- each crates.io Trusted Publisher matches `.github/workflows/publish.yml` and
  the `crates-io` environment;
- the `crates-io` environment requires its configured approval and has no
  long-lived registry token; and
- no intended version, tag, or GitHub Release exists, except when deliberately
  recovering a partial run.

Run validation from `main`:

```shell
gh workflow run publish.yml --ref main -f operation=validate
```

Validation gives Cargo a temporary home that patches only the unpublished
implementation crates to their reviewed workspace paths. It does not patch
`yaml-sigil-traits`, so ordinary package validation must resolve that version
from crates.io. It then runs `cargo package` in publication order and a
release-plz dry run. Validation has no OIDC permission, uploads nothing, and
does not enter the publication environment.

## Publish an official release

Dispatch publication from `main`:

```shell
gh workflow run publish.yml --ref main -f operation=publish
```

The validation job runs first. The publication job starts only after
validation succeeds and the `crates-io` environment is approved. Only that job
receives `id-token: write` and `contents: write`. Release-plz exchanges the job
identity for a short-lived crates.io credential, publishes the four source
packages in dependency order, and creates each configured tag and source-only
GitHub Release. Prerelease versions become GitHub prereleases.

The publication invocation deliberately omits release-plz's `dry_run` input.
Any nonempty value, including the string `false`, enables dry-run behavior.
Release-plz skips exact versions already on crates.io, which supports a
carefully reviewed partial-run retry. After all four packages verify, the
workflow requests the next release proposal.

## Verify and recover

The workflow waits for crates.io to expose all four versions as non-yanked and
confirms Cargo can resolve each one. Afterward, verify every package owner list,
exact tag target, changelog-based Release body, and absence of attached assets.
Record the workflow run, packages, tags, and Releases in the workspace release
records.

Never blindly retry a failed publication. Inspect crates.io, every tag, and
every GitHub Release first. An existing crate version cannot be overwritten,
even if yanked. Determine which later dependencies or repository release
objects remain absent before a reviewed retry. Do not replace Trusted
Publishing with a long-lived token, bypass an environment, reuse a version, or
attach binary assets as a recovery shortcut.

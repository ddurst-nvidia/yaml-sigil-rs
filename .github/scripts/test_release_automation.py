#!/usr/bin/env python3
# SPDX-FileCopyrightText: Copyright 2026 NVIDIA CORPORATION & AFFILIATES
# SPDX-License-Identifier: Apache-2.0

"""Regression tests for official release baseline and registry checks."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT_DIR = Path(__file__).resolve().parent
BASELINE_PATH = SCRIPT_DIR / "prepare_release_baseline.py"
VERIFY_PATH = SCRIPT_DIR / "verify-crates-io-packages.sh"
UPDATE_PR_PATH = SCRIPT_DIR / "update-release-pull-request.sh"
INSTALL_PATH = SCRIPT_DIR / "install-release-tools.sh"
CONFIGURE_GIT_PATH = SCRIPT_DIR / "configure-release-git-user.sh"
VERIFY_TRAITS_PATH = SCRIPT_DIR / "verify-release-traits.sh"
SPEC = importlib.util.spec_from_file_location("prepare_release_baseline", BASELINE_PATH)
assert SPEC is not None and SPEC.loader is not None
BASELINE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASELINE)


def command(*args: str, cwd: Path, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        check=check,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


class GitFixture:
    def __init__(
        self,
        repository: str,
        *,
        lightweight: bool = False,
        mismatched_workspace_tags: bool = False,
        nonancestor: bool = False,
        higher_snapshot: bool = False,
    ) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="release-baseline-")
        self.root = Path(self.temporary.name)
        self.remote = self.root / "remote.git"
        self.source = self.root / "source"
        self.checkout = self.root / "checkout"
        self.output = self.root / "baseline"
        self.repository = repository
        self.version = "0.4.0-rc.1"
        command("git", "init", "--bare", "--initial-branch=main", str(self.remote), cwd=self.root)
        command("git", "init", "--initial-branch=main", str(self.source), cwd=self.root)
        command("git", "config", "user.name", "Release Test", cwd=self.source)
        command("git", "config", "user.email", "release-test@example.com", cwd=self.source)
        self.write_commit("version = \"0.4.0-rc.1\"\n", "baseline")
        baseline = command("git", "rev-parse", "HEAD", cwd=self.source).stdout.strip()

        if nonancestor:
            command("git", "switch", "--orphan", "tag-source", cwd=self.source)
            self.write_commit("version = \"0.4.0-rc.1\"\n", "unrelated baseline")

        tags = tuple(
            template.format(version=self.version)
            for template in BASELINE.REPOSITORY_TAGS[repository]
        )
        for index, tag in enumerate(tags):
            if mismatched_workspace_tags and index == len(tags) - 1:
                self.write_commit("version = \"0.4.0-rc.1\"\n# split\n", "split tag")
            tag_args = ("git", "tag", tag) if lightweight else (
                "git",
                "tag",
                "-a",
                tag,
                "-m",
                f"Release {tag}",
            )
            command(*tag_args, cwd=self.source)

        if nonancestor:
            command("git", "switch", "main", cwd=self.source)
            self.write_commit("version = \"0.4.0-rc.1\"\n# main\n", "main work")
        elif not mismatched_workspace_tags:
            self.write_commit("version = \"0.4.0-rc.1\"\n# current\n", "current main")

        if higher_snapshot:
            snapshot_tag = "v99.0.0-0.pr.99.commit.sha0123456789ab"
            if repository == "NVIDIA/yaml-sigil-rs":
                snapshot_tag = (
                    "yaml-sigil-core-v99.0.0-0.pr.99.commit.sha0123456789ab"
                )
            command(
                "git",
                "tag",
                "-a",
                snapshot_tag,
                "-m",
                "Unrelated snapshot marker",
                cwd=self.source,
            )

        command("git", "remote", "add", "origin", str(self.remote), cwd=self.source)
        command("git", "push", "origin", "main", cwd=self.source)
        command("git", "push", "origin", "--tags", cwd=self.source)
        command("git", "clone", str(self.remote), str(self.checkout), cwd=self.root)
        command(
            "git",
            "config",
            "remote.origin.pushurl",
            BASELINE.READ_ONLY_PUSH_URL,
            cwd=self.checkout,
        )
        self.head = command("git", "rev-parse", "HEAD", cwd=self.checkout).stdout.strip()
        self.baseline = baseline

    def write_commit(self, contents: str, message: str) -> None:
        (self.source / "Cargo.toml").write_text(contents, encoding="utf-8")
        command("git", "add", "Cargo.toml", cwd=self.source)
        command("git", "commit", "-m", message, cwd=self.source)

    def prepare(self) -> tuple[str, Path, tuple[str, ...]]:
        return BASELINE.prepare_baseline(
            self.checkout,
            self.repository,
            self.version,
            self.head,
            self.output,
            str(self.remote),
            BASELINE.READ_ONLY_PUSH_URL,
        )

    def close(self) -> None:
        self.temporary.cleanup()


class ReleaseBaselineTests(unittest.TestCase):
    def test_traits_uses_tagged_commit_and_ignores_snapshot_marker(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-traits", higher_snapshot=True)
        self.addCleanup(fixture.close)
        commit, manifest, tags = fixture.prepare()
        self.assertEqual(commit, fixture.baseline)
        self.assertEqual(manifest.parent, fixture.output)
        self.assertEqual(tags, ("v0.4.0-rc.1",))

    def test_workspace_requires_all_tags_at_one_commit(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-rs")
        self.addCleanup(fixture.close)
        commit, _, tags = fixture.prepare()
        self.assertEqual(commit, fixture.baseline)
        self.assertEqual(len(tags), 4)

    def test_workspace_ignores_higher_unofficial_snapshot_marker(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-rs", higher_snapshot=True)
        self.addCleanup(fixture.close)
        commit, _, _ = fixture.prepare()
        self.assertEqual(commit, fixture.baseline)

    def test_current_workspace_baseline_ignores_older_split_tag_layout(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-rs")
        self.addCleanup(fixture.close)
        prefixes = ("core", "transcription", "signing", "verification")
        for index, prefix in enumerate(prefixes):
            target = fixture.baseline if index < 2 else fixture.head
            tag = f"yaml-sigil-{prefix}-v0.3.0-rc.1"
            command("git", "tag", "-a", tag, "-m", f"Release {tag}", target, cwd=fixture.source)
        command("git", "push", "origin", "--tags", cwd=fixture.source)
        command("git", "fetch", "origin", "--tags", cwd=fixture.checkout)
        commit, _, _ = fixture.prepare()
        self.assertEqual(commit, fixture.baseline)

    def test_workspace_rejects_mismatched_tags(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-rs", mismatched_workspace_tags=True)
        self.addCleanup(fixture.close)
        with self.assertRaises(BASELINE.BaselineError):
            fixture.prepare()

    def test_missing_tag_fails_closed(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-traits")
        self.addCleanup(fixture.close)
        command("git", "tag", "-d", "v0.4.0-rc.1", cwd=fixture.checkout)
        with self.assertRaises(BASELINE.BaselineError):
            fixture.prepare()

    def test_lightweight_tag_fails_closed(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-traits", lightweight=True)
        self.addCleanup(fixture.close)
        with self.assertRaises(BASELINE.BaselineError):
            fixture.prepare()

    def test_unreachable_tag_fails_closed(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-traits", nonancestor=True)
        self.addCleanup(fixture.close)
        with self.assertRaises(BASELINE.BaselineError):
            fixture.prepare()

    def test_remote_main_advance_fails_closed(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-traits")
        self.addCleanup(fixture.close)
        fixture.write_commit("version = \"0.4.0-rc.1\"\n# later\n", "later main")
        command("git", "push", "origin", "main", cwd=fixture.source)
        with self.assertRaises(BASELINE.BaselineError):
            fixture.prepare()

    def test_push_url_must_be_disabled(self) -> None:
        fixture = GitFixture("NVIDIA/yaml-sigil-traits")
        self.addCleanup(fixture.close)
        command(
            "git",
            "config",
            "remote.origin.pushurl",
            str(fixture.remote),
            cwd=fixture.checkout,
        )
        with self.assertRaises(BASELINE.BaselineError):
            fixture.prepare()


class RegistryVersionTests(unittest.TestCase):
    def run_check(self, state: str) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory(prefix="registry-version-") as temporary:
            fake_bin = Path(temporary)
            fake_curl = fake_bin / "curl"
            fake_curl.write_text(
                """#!/usr/bin/env bash
set -eu
output=''
while [[ \"$#\" -gt 0 ]]; do
  if [[ \"$1\" == '--output' ]]; then
    shift
    output=\"$1\"
  fi
  shift
done
case \"${FAKE_CRATES_STATE}\" in
  available)
    printf '%s' '{\"version\":{\"num\":\"1.2.3\",\"yanked\":false}}' >\"${output}\"
    printf '200'
    ;;
  yanked)
    printf '%s' '{\"version\":{\"num\":\"1.2.3\",\"yanked\":true}}' >\"${output}\"
    printf '200'
    ;;
  missing)
    : >\"${output}\"
    printf '404'
    ;;
esac
""",
                encoding="utf-8",
            )
            fake_curl.chmod(0o755)
            environment = os.environ.copy()
            environment["FAKE_CRATES_STATE"] = state
            environment["PATH"] = f"{fake_bin}:{environment['PATH']}"
            return subprocess.run(
                [
                    "bash",
                    str(VERIFY_PATH),
                    "--check-version",
                    "1.2.3",
                    "yaml-sigil-test",
                ],
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
            )

    def test_available_exact_version_passes(self) -> None:
        self.assertEqual(self.run_check("available").returncode, 0)

    def test_missing_exact_version_is_wait_state(self) -> None:
        self.assertEqual(self.run_check("missing").returncode, 3)

    def test_yanked_exact_version_is_hard_failure(self) -> None:
        self.assertEqual(self.run_check("yanked").returncode, 1)


class WorkflowBoundaryTests(unittest.TestCase):
    def test_workflows_have_no_pull_request_publication_path(self) -> None:
        workflow_root = SCRIPT_DIR.parent / "workflows"
        bodies = {
            path.name: path.read_text(encoding="utf-8")
            for path in workflow_root.glob("*.yml")
        }
        combined = "\n".join(bodies.values())
        for forbidden in (
            "validate-pr",
            "publish-pr",
            "pr_number",
            "crates-io-pr",
            "release-plz-snapshot",
        ):
            self.assertNotIn(forbidden, combined)
        self.assertEqual(combined.count("id-token: write"), 1)
        self.assertIn("environment: crates-io\n", bodies["publish.yml"])

    def test_release_workflows_pin_the_exact_analyzer_bootstrap(self) -> None:
        action = (
            "cargo-bins/cargo-binstall@"
            "732870f031d2fb36309d0deaf36abcc704a7be65 # v1.20.1"
        )
        for workflow in ("release-pr.yml", "publish.yml"):
            body = (SCRIPT_DIR.parent / "workflows" / workflow).read_text(encoding="utf-8")
            self.assertIn(action, body)
            self.assertNotIn("release-plz/action@", body)
        release_proposal = (
            SCRIPT_DIR.parent / "workflows" / "release-pr.yml"
        ).read_text(encoding="utf-8")
        self.assertIn("disabled://yaml-sigil-release-proposal", release_proposal)
        generator = (SCRIPT_DIR / "generate-release-proposal.sh").read_text(encoding="utf-8")
        self.assertIn("--registry-manifest-path", generator)

    def test_rs_release_paths_require_exact_traits_identity(self) -> None:
        if not VERIFY_TRAITS_PATH.exists():
            self.skipTest("single-crate repository has no traits dependency")
        publish = (SCRIPT_DIR.parent / "workflows" / "publish.yml").read_text(
            encoding="utf-8"
        )
        proposal = (SCRIPT_DIR.parent / "workflows" / "release-pr.yml").read_text(
            encoding="utf-8"
        )
        self.assertEqual(publish.count("verify-release-traits.sh"), 2)
        self.assertEqual(proposal.count("verify-release-traits.sh"), 2)

    def test_candidate_and_trusted_runner_labels_remain_separate(self) -> None:
        protected = (SCRIPT_DIR.parent / "workflows" / "pr-ci.yml").read_text(
            encoding="utf-8"
        )
        trusted = (SCRIPT_DIR.parent / "workflows" / "ci.yml").read_text(
            encoding="utf-8"
        )
        for runner in ("ubuntu-latest", "macos-latest", "windows-latest"):
            self.assertIn(f"runner: {runner}", protected)
        self.assertNotIn("linux-amd64-cpu4", protected)
        self.assertNotIn("linux-amd64-cpu8", protected)
        self.assertIn("runs-on: linux-amd64-cpu4", trusted)
        self.assertIn("runner: linux-amd64-cpu8", trusted)


class ReleaseToolVersionTests(unittest.TestCase):
    def run_versions(
        self,
        *,
        binstall: str = "cargo-binstall 1.20.1",
        release_plz: str = "release-plz 0.3.160",
        semver_checks: str = "cargo-semver-checks 0.48.0",
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory(prefix="release-tools-") as temporary:
            fake_bin = Path(temporary)
            (fake_bin / "cargo-binstall").write_text(
                "#!/usr/bin/env bash\nprintf '%s\\n' \"${FAKE_BINSTALL_VERSION}\"\n",
                encoding="utf-8",
            )
            (fake_bin / "release-plz").write_text(
                "#!/usr/bin/env bash\nprintf '%s\\n' \"${FAKE_RELEASE_PLZ_VERSION}\"\n",
                encoding="utf-8",
            )
            (fake_bin / "cargo").write_text(
                """#!/usr/bin/env bash
set -eu
if [[ "${1:-}" == "binstall" ]]; then
  exit 0
fi
if [[ "${1:-}" == "semver-checks" && "${2:-}" == "--version" ]]; then
  printf '%s\n' "${FAKE_SEMVER_CHECKS_VERSION}"
  exit 0
fi
exit 2
""",
                encoding="utf-8",
            )
            for executable in ("cargo-binstall", "release-plz", "cargo"):
                (fake_bin / executable).chmod(0o755)
            environment = os.environ.copy()
            environment.update(
                {
                    "FAKE_BINSTALL_VERSION": binstall,
                    "FAKE_RELEASE_PLZ_VERSION": release_plz,
                    "FAKE_SEMVER_CHECKS_VERSION": semver_checks,
                    "PATH": f"{fake_bin}:{environment['PATH']}",
                }
            )
            return subprocess.run(
                ["bash", str(INSTALL_PATH)],
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
            )

    def test_exact_release_tool_versions_pass(self) -> None:
        self.assertEqual(self.run_versions().returncode, 0)

    def test_wrong_cargo_binstall_version_fails_before_install(self) -> None:
        self.assertNotEqual(self.run_versions(binstall="cargo-binstall 1.20.0").returncode, 0)

    def test_wrong_release_plz_version_fails(self) -> None:
        self.assertNotEqual(self.run_versions(release_plz="release-plz 0.3.159").returncode, 0)

    def test_wrong_semver_checks_version_fails(self) -> None:
        self.assertNotEqual(
            self.run_versions(semver_checks="cargo-semver-checks 0.47.0").returncode,
            0,
        )


class ReleaseGitIdentityTests(unittest.TestCase):
    def run_identity(
        self, *, login: str = "github-actions[bot]", database_id: str = "41898282"
    ) -> tuple[subprocess.CompletedProcess[str], str, str]:
        with tempfile.TemporaryDirectory(prefix="release-git-identity-") as temporary:
            root = Path(temporary)
            command("git", "init", "--initial-branch=main", cwd=root)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            fake_gh = fake_bin / "gh"
            fake_gh.write_text(
                """#!/usr/bin/env bash
set -eu
printf '{\"name\":\"\",\"login\":\"%s\",\"databaseId\":%s}\n' \
  \"${FAKE_LOGIN}\" \"${FAKE_DATABASE_ID}\"
""",
                encoding="utf-8",
            )
            fake_gh.chmod(0o755)
            environment = os.environ.copy()
            environment.update(
                {
                    "FAKE_DATABASE_ID": database_id,
                    "FAKE_LOGIN": login,
                    "GITHUB_TOKEN": "fixture-token",
                    "PATH": f"{fake_bin}:{environment['PATH']}",
                }
            )
            result = subprocess.run(
                ["bash", str(CONFIGURE_GIT_PATH)],
                cwd=root,
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
            )
            name = command(
                "git", "config", "--local", "user.name", cwd=root, check=False
            ).stdout.strip()
            email = command(
                "git", "config", "--local", "user.email", cwd=root, check=False
            ).stdout.strip()
            return result, name, email

    def test_workflow_token_identity_is_configured_exactly(self) -> None:
        result, name, email = self.run_identity()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(name, "github-actions[bot]")
        self.assertEqual(
            email,
            "41898282+github-actions[bot]@users.noreply.github.com",
        )

    def test_invalid_workflow_token_identity_fails_closed(self) -> None:
        result, _, _ = self.run_identity(login="unexpected login", database_id="null")
        self.assertNotEqual(result.returncode, 0)


@unittest.skipUnless(VERIFY_TRAITS_PATH.exists(), "rs-only exact traits preflight")
class ExactTraitsIdentityTests(unittest.TestCase):
    def run_preflight(self, mode: str) -> tuple[subprocess.CompletedProcess[str], str]:
        with tempfile.TemporaryDirectory(prefix="traits-identity-") as temporary:
            fake_bin = Path(temporary)
            calls = fake_bin / "cargo.log"
            fake_cargo = fake_bin / "cargo"
            fake_cargo.write_text(
                """#!/usr/bin/env bash
set -eu
printf '%s\n' "$*" >>"${FAKE_CARGO_LOG}"
if [[ "${1:-}" == "metadata" && " $* " == *" --no-deps "* ]]; then
  source='registry+https://github.com/rust-lang/crates.io-index'
  if [[ "${FAKE_TRAITS_MODE}" == "bad-source" ]]; then
    source='git+https://example.invalid/traits'
  fi
  jq --null-input --arg source "${source}" '{packages:[{dependencies:[{
    name:"yaml-sigil-traits", req:"=0.4.0-rc.1", source:$source,
    registry:null, rename:null}]}]}'
  exit 0
fi
if [[ "${1:-}" == "info" && "$*" == \
  "info --quiet --registry crates-io yaml-sigil-traits@0.4.0-rc.1" ]]; then
  exit 0
fi
if [[ "${1:-}" == "metadata" ]]; then
  if [[ "${FAKE_TRAITS_MODE}" == "extra-source" ]]; then
    jq --null-input '{packages:[
      {name:"yaml-sigil-traits", version:"0.4.0-rc.1",
       source:"registry+https://github.com/rust-lang/crates.io-index"},
      {name:"yaml-sigil-traits", version:"9.9.9",
       source:"git+https://example.invalid/traits"}]}'
  else
    jq --null-input '{packages:[{
      name:"yaml-sigil-traits", version:"0.4.0-rc.1",
      source:"registry+https://github.com/rust-lang/crates.io-index"}]}'
  fi
  exit 0
fi
exit 2
""",
                encoding="utf-8",
            )
            fake_curl = fake_bin / "curl"
            fake_curl.write_text(
                """#!/usr/bin/env bash
set -eu
output=''
while [[ "$#" -gt 0 ]]; do
  if [[ "$1" == '--output' ]]; then
    shift
    output="$1"
  fi
  shift
done
printf '%s' '{"version":{"num":"0.4.0-rc.1","yanked":false}}' >"${output}"
printf '200'
""",
                encoding="utf-8",
            )
            fake_cargo.chmod(0o755)
            fake_curl.chmod(0o755)
            environment = os.environ.copy()
            environment.update(
                {
                    "FAKE_CARGO_LOG": str(calls),
                    "FAKE_TRAITS_MODE": mode,
                    "PATH": f"{fake_bin}:{environment['PATH']}",
                }
            )
            result = subprocess.run(
                ["bash", str(VERIFY_TRAITS_PATH)],
                cwd=SCRIPT_DIR.parent.parent,
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
            )
            log = calls.read_text(encoding="utf-8") if calls.exists() else ""
            return result, log

    def test_exact_named_registry_traits_identity_passes(self) -> None:
        result, log = self.run_preflight("exact")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            "info --quiet --registry crates-io yaml-sigil-traits@0.4.0-rc.1",
            log,
        )

    def test_alternate_traits_source_fails_closed(self) -> None:
        result, _ = self.run_preflight("bad-source")
        self.assertNotEqual(result.returncode, 0)

    def test_additional_resolved_traits_identity_fails_closed(self) -> None:
        result, _ = self.run_preflight("extra-source")
        self.assertNotEqual(result.returncode, 0)


class ReleasePullRequestFixtureTests(unittest.TestCase):
    FAKE_GH = r'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

args = sys.argv[1:]
method = "GET"
if "--method" in args:
    method = args[args.index("--method") + 1]
endpoint = next(
    value for value in args if value == "graphql" or value.startswith(("repos/", "users/"))
)
payload = sys.stdin.read() if "--input" in args else ""
state_path = Path(os.environ["GH_FIXTURE_STATE_FILE"])
state = json.loads(state_path.read_text()) if state_path.exists() else {}
with Path(os.environ["GH_FIXTURE_LOG"]).open("a", encoding="utf-8") as log:
    log.write(json.dumps({
        "method": method,
        "endpoint": endpoint,
        "args": args,
        "payload": payload,
        "expected_base_tree": os.environ["GH_FIXTURE_BASE_TREE"],
    }) + "\n")

def save():
    state_path.write_text(json.dumps(state), encoding="utf-8")

def form(name):
    prefix = f"{name}="
    return next((value[len(prefix):] for value in args if value.startswith(prefix)), "")

repo = os.environ["GITHUB_REPOSITORY"]
head = os.environ["GITHUB_SHA"]
commit = os.environ["GH_FIXTURE_COMMIT"]
other = os.environ["GH_FIXTURE_OTHER"]
mode = os.environ["GH_FIXTURE_MODE"]
bot = os.environ["GH_FIXTURE_BOT"]
bot_id = os.environ["GH_FIXTURE_BOT_ID"]
bot_email = f"{bot_id}+{bot}@users.noreply.github.com"
dco = f"Signed-off-by: {bot} <{bot_email}>"
message = f"{os.environ['RELEASE_TITLE']}\n\n{dco}"
main_endpoint = f"repos/{repo}/git/ref/heads/main"
target_endpoint = f"repos/{repo}/git/ref/heads/{os.environ['RELEASE_BRANCH']}"
matching_endpoint = (
    f"repos/{repo}/git/matching-refs/heads/{os.environ['RELEASE_BRANCH']}"
)

if endpoint == main_endpoint:
    state["main_reads"] = state.get("main_reads", 0) + 1
    value = other if mode == "stale" and state["main_reads"] > 1 else head
    save()
    print(value if "--jq" in args else json.dumps({"object": {"sha": value}}))
elif endpoint == f"users/{bot}":
    print(json.dumps({"login": bot, "id": int(bot_id)}))
elif endpoint == matching_endpoint:
    if mode == "ref-lookup-failure":
        sys.exit(1)
    if mode in ("foreign", "wrong-pr-base"):
        print(json.dumps([{
            "ref": f"refs/heads/{os.environ['RELEASE_BRANCH']}",
            "object": {"type": "commit", "sha": commit},
        }]))
    else:
        print("[]")
elif endpoint == target_endpoint and method == "GET":
    if state.get("target_created"):
        value = other if mode == "wrong-ref" and state.get("target_created") else commit
        print(value if "--jq" in args else json.dumps({
            "ref": f"refs/heads/{os.environ['RELEASE_BRANCH']}",
            "object": {"type": "commit", "sha": value},
        }))
    else:
        sys.exit(1)
elif "/compare/main..." in endpoint:
    author = "someone-else" if mode == "foreign" else bot
    print(json.dumps({"ahead_by": 1, "commits": [{"author": {"login": author}}]}))
elif endpoint == f"repos/{repo}/git/refs" and method == "POST":
    ref = form("ref")
    if ref == f"refs/heads/{os.environ['RELEASE_BRANCH']}":
        state["target_created"] = True
    save()
    print(json.dumps({"ref": ref, "object": {"type": "commit", "sha": form("sha")}}))
elif f"repos/{repo}/git/refs/heads/" in endpoint and method == "PATCH":
    ref = "refs/heads/" + endpoint.split("/git/refs/heads/", 1)[1]
    print(json.dumps({"ref": ref, "object": {"type": "commit", "sha": form("sha")}}))
elif endpoint == f"repos/{repo}/git/trees" and method == "POST":
    print(json.dumps({"sha": os.environ["GH_FIXTURE_TREE"]}))
elif endpoint == f"repos/{repo}/git/commits" and method == "POST":
    if mode == "create-failure":
        sys.exit(1)
    print(json.dumps({
        "sha": commit,
        "author": {"name": bot, "email": bot_email},
        "committer": {"name": bot, "email": bot_email},
        "verification": {"verified": True, "reason": "valid"},
        "tree": {"sha": os.environ["GH_FIXTURE_TREE"]},
        "parents": [{"sha": head}],
        "message": message,
    }))
elif endpoint == f"repos/{repo}/commits/{commit}":
    if mode == "unreachable":
        sys.exit(1)
    print(json.dumps({
        "author": {"login": bot},
        "committer": {"login": bot},
        "commit": {
            "message": message,
            "verification": {"verified": True, "reason": "valid"},
        },
        "parents": [{"sha": head}],
    }))
elif endpoint == f"repos/{repo}/pulls" and method == "GET":
    if mode == "wrong-pr-base":
        print(json.dumps([{
            "number": 8,
            "state": "open",
            "user": {"login": bot},
            "head": {
                "repo": {"full_name": repo},
                "ref": os.environ["RELEASE_BRANCH"],
                "sha": commit,
            },
            "base": {"repo": {"full_name": repo}, "ref": "develop"},
        }]))
    else:
        print("[]")
elif endpoint == f"repos/{repo}/pulls" and method == "POST":
    print(json.dumps({"number": 7}))
elif endpoint == f"repos/{repo}/pulls/7" and method == "GET":
    print(json.dumps({
        "number": 7,
        "state": "open",
        "user": {"login": bot},
        "head": {
            "repo": {"full_name": repo},
            "ref": os.environ["RELEASE_BRANCH"],
            "sha": commit,
        },
        "base": {"repo": {"full_name": repo}, "ref": "main"},
        "title": os.environ["RELEASE_TITLE"],
        "body": "Release body.",
        "commits": 1,
        "draft": os.environ["RELEASE_DRAFT"] == "true",
    }))
elif endpoint == "graphql":
    print(json.dumps({"data": {}}))
else:
    print("{}")
'''

    def run_fixture(self, mode: str) -> tuple[subprocess.CompletedProcess[str], list[dict[str, object]]]:
        with tempfile.TemporaryDirectory(prefix="release-pr-api-") as temporary:
            root = Path(temporary)
            repository = root / "repository"
            repository.mkdir()
            command("git", "init", "--initial-branch=main", cwd=repository)
            command("git", "config", "user.name", "Release Test", cwd=repository)
            command("git", "config", "user.email", "release-test@example.com", cwd=repository)
            (repository / "Cargo.toml").write_text("version = \"1.0.0\"\n", encoding="utf-8")
            command("git", "add", "Cargo.toml", cwd=repository)
            command("git", "commit", "-m", "baseline", cwd=repository)
            head = command("git", "rev-parse", "HEAD", cwd=repository).stdout.strip()
            base_tree = command(
                "git", "rev-parse", "HEAD^{tree}", cwd=repository
            ).stdout.strip()
            (repository / "Cargo.toml").write_text("version = \"1.0.1\"\n", encoding="utf-8")
            if mode == "added-file":
                (repository / "CHANGELOG.md").write_text("# Added\n", encoding="utf-8")

            fake_bin = root / "bin"
            fake_bin.mkdir()
            fake_gh = fake_bin / "gh"
            fake_gh.write_text(self.FAKE_GH, encoding="utf-8")
            fake_gh.chmod(0o755)
            body = root / "body.md"
            body.write_text("Release body.\n", encoding="utf-8")
            output = root / "github-output"
            output.write_text("", encoding="utf-8")
            log = root / "gh.log"
            log.touch()

            environment = os.environ.copy()
            environment.update(
                {
                    "APP_SLUG": "nvidia-yamlsigil-release-pr",
                    "GH_FIXTURE_BOT": "nvidia-yamlsigil-release-pr[bot]",
                    "GH_FIXTURE_BOT_ID": "12345",
                    "GH_FIXTURE_BASE_TREE": base_tree,
                    "GH_FIXTURE_COMMIT": "2" * 40,
                    "GH_FIXTURE_LOG": str(log),
                    "GH_FIXTURE_MODE": mode,
                    "GH_FIXTURE_OTHER": "3" * 40,
                    "GH_FIXTURE_STATE_FILE": str(root / "state.json"),
                    "GH_FIXTURE_TREE": "4" * 40,
                    "GH_TOKEN": "fixture-token",
                    "GITHUB_OUTPUT": str(output),
                    "GITHUB_REPOSITORY": "NVIDIA/yaml-sigil-traits",
                    "GITHUB_RUN_ATTEMPT": "1",
                    "GITHUB_RUN_ID": "99",
                    "GITHUB_SHA": head,
                    "PATH": f"{fake_bin}:{environment['PATH']}",
                    "RELEASE_BODY_FILE": str(body),
                    "RELEASE_BRANCH": "release-plz-next",
                    "RELEASE_DRAFT": "false",
                    "RELEASE_TITLE": "chore(release): prepare test 1.0.1",
                }
            )
            result = subprocess.run(
                ["bash", str(UPDATE_PR_PATH)],
                cwd=repository,
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
            )
            calls = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]
            if result.returncode == 0:
                self.assertIn("commit_sha=" + "2" * 40, output.read_text(encoding="utf-8"))
                self.assertIn("pr_number=7", output.read_text(encoding="utf-8"))
            return result, calls

    def test_app_git_objects_become_reachable_before_durable_ref(self) -> None:
        result, calls = self.run_fixture("success")
        self.assertEqual(result.returncode, 0, result.stderr)
        endpoints = [(call["method"], call["endpoint"]) for call in calls]
        commit_index = endpoints.index(("POST", "repos/NVIDIA/yaml-sigil-traits/git/commits"))
        reachability_index = endpoints.index(("GET", "repos/NVIDIA/yaml-sigil-traits/commits/" + "2" * 40))
        target_index = endpoints.index(("POST", "repos/NVIDIA/yaml-sigil-traits/git/refs"), reachability_index)
        self.assertLess(commit_index, reachability_index)
        self.assertLess(reachability_index, target_index)
        tree_call = next(
            call
            for call in calls
            if (call["method"], call["endpoint"])
            == ("POST", "repos/NVIDIA/yaml-sigil-traits/git/trees")
        )
        self.assertEqual(
            json.loads(tree_call["payload"])["base_tree"],
            tree_call["expected_base_tree"],
        )

    def test_added_release_file_is_rejected_before_api_writes(self) -> None:
        result, calls = self.run_fixture("added-file")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("only modify existing files", result.stderr)
        self.assertFalse(any(call["method"] != "GET" for call in calls))

    def test_stale_main_never_moves_the_durable_ref(self) -> None:
        result, calls = self.run_fixture("stale")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Main advanced", result.stderr)
        self.assertNotIn(("POST", "repos/NVIDIA/yaml-sigil-traits/pulls"), [(c["method"], c["endpoint"]) for c in calls])

    def test_foreign_release_branch_is_preserved(self) -> None:
        result, calls = self.run_fixture("foreign")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("non-App commit", result.stderr)
        self.assertNotIn(("POST", "repos/NVIDIA/yaml-sigil-traits/git/trees"), [(c["method"], c["endpoint"]) for c in calls])

    def test_release_ref_lookup_failure_fails_before_api_writes(self) -> None:
        result, calls = self.run_fixture("ref-lookup-failure")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call["method"] != "GET" for call in calls))

    def test_existing_pr_ref_collision_fails_before_git_object_writes(self) -> None:
        result, calls = self.run_fixture("wrong-pr-base")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected ownership or refs", result.stderr)
        self.assertNotIn(
            ("POST", "repos/NVIDIA/yaml-sigil-traits/git/trees"),
            [(call["method"], call["endpoint"]) for call in calls],
        )

    def test_unreachable_commit_never_moves_the_durable_ref(self) -> None:
        result, calls = self.run_fixture("unreachable")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("did not resolve", result.stderr)
        self.assertNotIn(("POST", "repos/NVIDIA/yaml-sigil-traits/pulls"), [(c["method"], c["endpoint"]) for c in calls])

    def test_wrong_explicit_release_ref_never_opens_a_pull_request(self) -> None:
        result, calls = self.run_fixture("wrong-ref")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not identify", result.stderr)
        self.assertNotIn(("POST", "repos/NVIDIA/yaml-sigil-traits/pulls"), [(c["method"], c["endpoint"]) for c in calls])


if __name__ == "__main__":
    unittest.main()

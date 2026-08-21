#!/usr/bin/env python3
# SPDX-FileCopyrightText: Copyright 2026 NVIDIA CORPORATION & AFFILIATES
# SPDX-License-Identifier: Apache-2.0

"""Prepare the last official tagged release as release-plz's baseline."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys


SHA_RE = re.compile(r"[0-9a-f]{40}")
VERSION_RE = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:-rc\.[0-9]+)?")
READ_ONLY_PUSH_URL = "disabled://yaml-sigil-release-proposal"
REPOSITORY_TAGS = {
    "NVIDIA/yaml-sigil-traits": ("v{version}",),
    "NVIDIA/yaml-sigil-rs": (
        "yaml-sigil-core-v{version}",
        "yaml-sigil-transcription-v{version}",
        "yaml-sigil-signing-v{version}",
        "yaml-sigil-verification-v{version}",
    ),
}
TAG_PATTERNS = {
    "NVIDIA/yaml-sigil-traits": re.compile(
        r"v(?P<version>[0-9]+\.[0-9]+\.[0-9]+(?:-rc\.[0-9]+)?)"
    ),
    "NVIDIA/yaml-sigil-rs": re.compile(
        r"yaml-sigil-(?:core|transcription|signing|verification)-v"
        r"(?P<version>[0-9]+\.[0-9]+\.[0-9]+(?:-rc\.[0-9]+)?)"
    ),
}


class BaselineError(RuntimeError):
    """A release baseline invariant was not satisfied."""


def git(root: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        ["git", "-C", str(root), *args],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if check and result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        raise BaselineError(f"git {' '.join(args)} failed: {detail}")
    return result


def exact_ref(root: Path, ref: str) -> str:
    result = git(root, "show-ref", "--verify", "--hash", ref)
    value = result.stdout.strip()
    if not SHA_RE.fullmatch(value):
        raise BaselineError(f"{ref} did not resolve to one full commit identifier")
    return value


def remote_refs(root: Path, *refs: str) -> dict[str, str]:
    result = git(root, "ls-remote", "--exit-code", "origin", *refs)
    parsed: dict[str, str] = {}
    for line in result.stdout.splitlines():
        fields = line.split("\t", 1)
        if len(fields) != 2 or not SHA_RE.fullmatch(fields[0]):
            raise BaselineError("origin returned an invalid ref response")
        if fields[1] in parsed:
            raise BaselineError(f"origin returned duplicate state for {fields[1]}")
        parsed[fields[1]] = fields[0]
    if set(parsed) != set(refs):
        missing = sorted(set(refs) - set(parsed))
        raise BaselineError(f"origin lacks required refs: {', '.join(missing)}")
    return parsed


def official_tag_versions(root: Path, repository: str, head: str) -> dict[str, str]:
    pattern = TAG_PATTERNS[repository]
    result = git(
        root,
        "for-each-ref",
        "--format=%(refname:strip=2) %(objecttype)",
        "refs/tags",
    )
    grouped: dict[str, dict[str, str]] = {}
    for line in result.stdout.splitlines():
        try:
            tag, object_type = line.rsplit(" ", 1)
        except ValueError as error:
            raise BaselineError("git returned an invalid tag inventory") from error
        match = pattern.fullmatch(tag)
        if match is None:
            continue
        if object_type != "tag":
            raise BaselineError(f"official tag {tag} is not annotated")
        commit = git(root, "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}").stdout.strip()
        if git(root, "merge-base", "--is-ancestor", commit, head, check=False).returncode == 0:
            grouped.setdefault(match.group("version"), {})[tag] = commit
    versions: dict[str, str] = {}
    for version, tagged_commits in grouped.items():
        expected = {
            template.format(version=version) for template in REPOSITORY_TAGS[repository]
        }
        commits = set(tagged_commits.values())
        if set(tagged_commits) == expected and len(commits) == 1:
            versions[version] = commits.pop()
    return versions


def require_last_official_version(
    root: Path, repository: str, version: str, head: str, baseline: str
) -> None:
    versions = official_tag_versions(root, repository, head)
    if versions.get(version) != baseline:
        raise BaselineError(f"{version} is not a complete official tag set")
    distances = {
        candidate: int(git(root, "rev-list", "--count", f"{commit}..{head}").stdout)
        for candidate, commit in versions.items()
    }
    if not distances:
        raise BaselineError("no reachable official annotated release tag exists")
    nearest_distance = min(distances.values())
    nearest_versions = sorted(
        candidate for candidate, distance in distances.items() if distance == nearest_distance
    )
    if nearest_versions != [version]:
        raise BaselineError(
            "the requested version is not the unique last official annotated release"
        )


def prepare_baseline(
    root: Path,
    repository: str,
    version: str,
    head: str,
    output: Path,
    expected_fetch_url: str,
    expected_push_url: str,
) -> tuple[str, Path, tuple[str, ...]]:
    if repository not in REPOSITORY_TAGS:
        raise BaselineError(f"unsupported release repository: {repository}")
    if VERSION_RE.fullmatch(version) is None:
        raise BaselineError(f"unsupported official release version: {version}")
    if SHA_RE.fullmatch(head) is None:
        raise BaselineError("the expected main commit must be a lowercase full SHA")

    root = root.resolve()
    if git(root, "rev-parse", "HEAD").stdout.strip() != head:
        raise BaselineError("the checkout is not at the exact expected main commit")
    if git(root, "remote", "get-url", "origin").stdout.strip() != expected_fetch_url:
        raise BaselineError("origin does not use the expected read-only fetch URL")
    push_urls = git(root, "config", "--get-all", "remote.origin.pushurl").stdout.splitlines()
    if push_urls != [expected_push_url]:
        raise BaselineError("origin does not have the exact disabled push URL")

    main_ref = "refs/heads/main"
    if remote_refs(root, main_ref)[main_ref] != head:
        raise BaselineError("origin/main advanced beyond the checked-out commit")

    tags = tuple(template.format(version=version) for template in REPOSITORY_TAGS[repository])
    commits: set[str] = set()
    for tag in tags:
        ref = f"refs/tags/{tag}"
        tag_object = exact_ref(root, ref)
        if git(root, "cat-file", "-t", tag_object).stdout.strip() != "tag":
            raise BaselineError(f"official tag {tag} is not annotated")
        commit = git(root, "rev-parse", "--verify", f"{ref}^{{commit}}").stdout.strip()
        remote = remote_refs(root, ref, f"{ref}^{{}}")
        if remote[ref] != tag_object or remote[f"{ref}^{{}}"] != commit:
            raise BaselineError(f"local tag {tag} differs from origin")
        commits.add(commit)

    if len(commits) != 1:
        raise BaselineError("official workspace tags resolve to different commits")
    baseline = commits.pop()
    if git(root, "merge-base", "--is-ancestor", baseline, head, check=False).returncode != 0:
        raise BaselineError("the official release tag is not an ancestor of current main")
    require_last_official_version(root, repository, version, head, baseline)

    output = output.resolve()
    if output.exists():
        raise BaselineError(f"baseline output already exists: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    git(root, "worktree", "add", "--detach", "--quiet", str(output), baseline)
    if git(output, "rev-parse", "HEAD").stdout.strip() != baseline:
        raise BaselineError("the detached baseline checkout changed commit")
    if git(output, "status", "--porcelain").stdout:
        raise BaselineError("the detached baseline checkout is not clean")
    manifest = output / "Cargo.toml"
    if not manifest.is_file():
        raise BaselineError("the detached baseline lacks Cargo.toml")
    return baseline, manifest, tags


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--root", default=Path.cwd(), type=Path)
    parser.add_argument("--expected-fetch-url", required=True)
    parser.add_argument("--expected-push-url", default=READ_ONLY_PUSH_URL)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        baseline, manifest, tags = prepare_baseline(
            args.root,
            args.repository,
            args.version,
            args.head,
            args.output,
            args.expected_fetch_url,
            args.expected_push_url,
        )
    except BaselineError as error:
        print(f"release baseline failed: {error}", file=sys.stderr)
        return 1
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with Path(github_output).open("a", encoding="utf-8") as output_file:
            output_file.write(f"commit={baseline}\n")
            output_file.write(f"manifest={manifest}\n")
            output_file.write(f"tags={','.join(tags)}\n")
    print(f"Prepared official release baseline {baseline} at {manifest}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

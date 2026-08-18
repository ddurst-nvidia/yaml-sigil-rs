// SPDX-FileCopyrightText: Copyright 2026 NVIDIA CORPORATION & AFFILIATES
// SPDX-License-Identifier: Apache-2.0

//! Compile, host-test, and dependency checks for the alloc-only public crates.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::{cargo, require_success, run as run_command};

const TOOLCHAIN: &str = "+1.95.0";
const TARGET: &str = "thumbv7em-none-eabi";
const PUBLIC_PACKAGES: &[&str] = &[
    "yaml-sigil-core",
    "yaml-sigil-transcription",
    "yaml-sigil-signing",
    "yaml-sigil-verification",
];
const DENIED_PACKAGES: &[&str] = &[
    "dirs",
    "dirs-sys",
    "filetime",
    "getrandom",
    "jsonschema",
    "pem-rfc7468",
    "pkcs8",
    "tempfile",
    "tokio",
    "tracing",
];
const DENIED_FEATURES: &[&str] = &["getrandom", "pem", "pkcs8", "std"];

pub(crate) fn run(root: &Path) -> Result<()> {
    require_success(
        run_command(cargo(
            root,
            [
                TOOLCHAIN,
                "fmt",
                "--manifest-path",
                "no-std-probe/Cargo.toml",
                "--all",
                "--check",
            ],
        ))?,
        "alloc-only consumer formatting",
    )?;

    let mut check_args = vec![TOOLCHAIN, "check"];
    for package in PUBLIC_PACKAGES {
        check_args.extend(["--package", package]);
    }
    check_args.extend(["--lib", "--no-default-features", "--target", TARGET]);
    require_success(
        run_command(cargo(root, check_args))?,
        "alloc-only public library target checks",
    )?;

    require_success(
        run_command(cargo(
            root,
            [
                TOOLCHAIN,
                "check",
                "--manifest-path",
                "no-std-probe/Cargo.toml",
                "--target",
                TARGET,
            ],
        ))?,
        "alloc-only consumer target check",
    )?;

    require_success(
        run_command(cargo(
            root,
            [TOOLCHAIN, "test", "--workspace", "--no-default-features"],
        ))?,
        "alloc-only host tests",
    )?;

    for package in PUBLIC_PACKAGES {
        let output = cargo_output(
            root,
            &[
                TOOLCHAIN,
                "tree",
                "--package",
                package,
                "--edges",
                "normal",
                "--no-default-features",
                "--target",
                TARGET,
                "--format",
                "{p}|{f}",
            ],
            "alloc-only dependency audit",
        )?;
        validate_dependency_tree(&String::from_utf8_lossy(&output.stdout))?;
    }

    Ok(())
}

fn cargo_output(root: &Path, args: &[&str], label: &str) -> Result<std::process::Output> {
    eprintln!("+ cargo {} (cwd {})", args.join(" "), root.display());
    let output = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to run cargo for {label}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        bail!(
            "{label} failed with {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

fn validate_dependency_tree(tree: &str) -> Result<()> {
    for line in tree.lines() {
        let Some((package, features)) = line.split_once('|') else {
            bail!("unexpected cargo tree line: {line}");
        };
        let package = package
            .trim_start_matches(|character: char| !character.is_ascii_alphanumeric())
            .split_whitespace()
            .next()
            .unwrap_or_default();
        if DENIED_PACKAGES.contains(&package) {
            bail!("alloc-only graph contains denied package {package}");
        }
        for feature in features.split(',').filter(|feature| !feature.is_empty()) {
            if DENIED_FEATURES.contains(&feature) {
                bail!("alloc-only graph enables denied feature {feature} on {package}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_dependency_tree;

    #[test]
    fn dependency_audit_accepts_alloc_only_features() {
        validate_dependency_tree("yaml-sigil-signing v0.0.0|alloc\n├── rand_chacha v0.3.1|simd\n")
            .unwrap();
    }

    #[test]
    fn dependency_audit_rejects_std_entropy_and_filesystem_dependencies() {
        assert!(validate_dependency_tree("crate v1.0.0|alloc,std\n").is_err());
        assert!(validate_dependency_tree("└── getrandom v0.3.0|\n").is_err());
        assert!(validate_dependency_tree("└── tempfile v3.0.0|\n").is_err());
    }
}

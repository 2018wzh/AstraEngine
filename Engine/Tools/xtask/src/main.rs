use anyhow::{bail, Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone)]
struct Package {
    name: String,
    isolated: bool,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(task) = args.first() else {
        return help();
    };
    if matches!(task.as_str(), "help" | "--help" | "-h") {
        return help();
    }
    if !matches!(
        task.as_str(),
        "docs" | "fmt" | "lint" | "build" | "test" | "check" | "check-fast"
    ) {
        bail!("unknown task {task}; use cargo xtask help");
    }
    let mut selection = "all";
    let mut packages = Vec::new();
    let mut options = args[1..].iter();
    while let Some(option) = options.next() {
        match option.as_str() {
            "--workspace" => {
                selection = options
                    .next()
                    .context("--workspace needs engine, emu, editor or all")?
            }
            "--package" | "-p" => packages.push(
                options
                    .next()
                    .context("--package needs a crate name")?
                    .clone(),
            ),
            _ => bail!("unknown option {option}"),
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .context("xtask repository root")?;
    let workspaces: Vec<_> = match selection {
        "engine" => vec![root.to_path_buf()],
        "emu" => vec![root.join("Emulator")],
        "editor" => vec![root.join("Editor")],
        "all" => {
            let mut paths = vec![root.to_path_buf(), root.join("Emulator")];
            if root.join("Editor/Cargo.toml").is_file() {
                paths.push(root.join("Editor"));
            }
            paths
        }
        _ => bail!("workspace must be engine, emu, editor or all"),
    };
    if !packages.is_empty() && selection == "all" {
        bail!("--package requires an explicit --workspace");
    }
    if matches!(task.as_str(), "docs" | "check" | "check-fast") {
        run(root, "python", &["Tools/check_docs.py".into()])?;
        run(
            root,
            "python",
            &[
                "-m".into(),
                "unittest".into(),
                "Tools.tests.test_check_docs".into(),
            ],
        )?;
    }
    if matches!(task.as_str(), "docs" | "check-fast") {
        return Ok(());
    }
    for workspace in workspaces {
        let selected = members(&workspace, &packages)?;
        if matches!(task.as_str(), "fmt" | "check") {
            let mut options = vec!["fmt".into(), "--check".into()];
            for package in &selected {
                options.extend(["--package".into(), package.name.clone()]);
            }
            run(&workspace, "cargo", &options)?;
        }
        if matches!(task.as_str(), "lint" | "check") {
            cargo(
                &workspace,
                "clippy",
                &selected,
                &["--all-targets", "--", "-D", "warnings"],
            )?;
        }
        if task == "build" || task == "check" || (task == "test" && packages.is_empty()) {
            // Complete product tests exercise installed binaries; ordinary focused tests need none.
            // The running xtask binary is already built. Windows locks it, so
            // rebuilding it in place would fail; lint and test still cover it.
            let products: Vec<_> = selected
                .iter()
                .filter(|package| package.name != "xtask")
                .cloned()
                .collect();
            if !products.is_empty() {
                cargo(&workspace, "build", &products, &[])?;
            }
        }
        if matches!(task.as_str(), "test" | "check") {
            cargo(&workspace, "test", &selected, &[])?;
        }
    }
    Ok(())
}

fn members(workspace: &PathBuf, requested: &[String]) -> Result<Vec<Package>> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace)
        .output()
        .context("read workspace metadata")?;
    if !output.status.success() {
        bail!(
            "workspace metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let ids = metadata["workspace_members"]
        .as_array()
        .context("workspace_members")?;
    let family_root = workspace.join("Source/Families");
    let packages: Vec<Package> = metadata["packages"]
        .as_array()
        .context("packages")?
        .iter()
        .filter(|package| ids.contains(&package["id"]))
        .map(|package| {
            let name = package["name"].as_str().context("package name")?;
            let manifest = package["manifest_path"]
                .as_str()
                .context("package manifest")?;
            Ok(Package {
                name: name.to_owned(),
                isolated: Path::new(manifest).starts_with(&family_root),
            })
        })
        .collect::<Result<_>>()?;
    for name in requested {
        if !packages.iter().any(|package| &package.name == name) {
            bail!("{name} is not a member of the selected workspace");
        }
    }
    Ok(if requested.is_empty() {
        packages
    } else {
        packages
            .into_iter()
            .filter(|package| requested.contains(&package.name))
            .collect()
    })
}

fn cargo(root: &Path, task: &str, packages: &[Package], extra: &[&str]) -> Result<()> {
    let run_unit = |unit: &[&Package]| -> Result<()> {
        let mut args = vec![task.into()];
        for package in unit {
            args.extend(["--package".into(), package.name.clone()]);
        }
        args.extend(extra.iter().map(|value| (*value).to_owned()));
        run(root, "cargo", &args)
    };
    let shared: Vec<_> = packages
        .iter()
        .filter(|package| !package.isolated)
        .collect();
    if !shared.is_empty() {
        run_unit(&shared)?;
    }
    // Native libraries own their runtimes. In particular Artemis Lua 5.1 and
    // Manager Luau must not have their mutually exclusive mlua features merged.
    for package in packages.iter().filter(|package| package.isolated) {
        run_unit(&[package])?;
    }
    Ok(())
}

fn run(root: &Path, program: &str, args: &[String]) -> Result<()> {
    println!("{program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| format!("start {program}"))?;
    if !status.success() {
        bail!("{program} failed ({status})");
    }
    Ok(())
}

fn help() -> Result<()> {
    println!("cargo xtask <docs|fmt|lint|build|test|check|check-fast> [--workspace engine|emu|editor|all] [-p crate]\nDefault: all active product workspaces. Focused tests never bootstrap a Headless server.");
    Ok(())
}

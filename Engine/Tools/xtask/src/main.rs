use anyhow::{bail, Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

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
                    .context("--workspace needs engine, emu or all")?
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
        "all" => {
            let mut paths = vec![root.to_path_buf(), root.join("Emulator")];
            if root.join("Editor/Cargo.toml").is_file() {
                paths.push(root.join("Editor"));
            }
            paths
        }
        _ => bail!("workspace must be engine, emu or all"),
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
                options.extend(["--package".into(), package.clone()]);
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
            cargo(&workspace, "build", &selected, &[])?;
        }
        if matches!(task.as_str(), "test" | "check") {
            cargo(&workspace, "test", &selected, &[])?;
        }
    }
    Ok(())
}

fn members(workspace: &PathBuf, requested: &[String]) -> Result<Vec<String>> {
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
    let names: Vec<String> = metadata["packages"]
        .as_array()
        .context("packages")?
        .iter()
        .filter(|package| ids.contains(&package["id"]))
        .map(|package| {
            package["name"]
                .as_str()
                .context("package name")
                .map(str::to_owned)
        })
        .collect::<Result<_>>()?;
    for name in requested {
        if !names.contains(name) {
            bail!("{name} is not a member of the selected workspace");
        }
    }
    Ok(if requested.is_empty() {
        names
    } else {
        requested.to_vec()
    })
}

fn cargo(root: &Path, task: &str, packages: &[String], extra: &[&str]) -> Result<()> {
    let mut args = vec![task.into()];
    for name in packages {
        args.extend(["--package".into(), name.clone()]);
    }
    args.extend(extra.iter().map(|value| (*value).to_owned()));
    run(root, "cargo", &args)
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
    println!("cargo xtask <docs|fmt|lint|build|test|check|check-fast> [--workspace engine|emu|all] [-p crate]\nDefault: all active product workspaces. Focused tests never bootstrap a Headless server.");
    Ok(())
}

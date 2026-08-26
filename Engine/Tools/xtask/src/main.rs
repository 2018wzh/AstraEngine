use std::process::Command;

fn main() -> anyhow::Result<()> {
    let task = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());
    match task.as_str() {
        "check" => {
            // 仅保人类可读：link + 路径泄露 + 最小 fmt/clippy
            run("python", &["Tools/check_docs.py"])?;
            // 可选：仅 link 检查的轻量模式，未来收束为 xtask 内置 Rust 实现
            println!("xtask check: human-readable gates passed");
            Ok(())
        }
        "check-fast" => {
            run("python", &["Tools/check_docs.py"])?;
            println!("xtask check-fast passed");
            Ok(())
        }
        _ => {
            println!("xtask tasks: check, check-fast");
            Ok(())
        }
    }
}

fn run(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(cmd).args(args).status()?;
    if !status.success() {
        anyhow::bail!("command {cmd} {args:?} failed");
    }
    Ok(())
}

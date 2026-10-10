//! Credential and private-artifact boundary check across worktree and Git history.
use std::{collections::BTreeSet, process::Command};
fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Git source audit command failed".into());
    }
    Ok(output.stdout)
}
fn finding(path: &str, bytes: &[u8]) -> Option<String> {
    if path.ends_with(".exe")
        || path.ends_with(".dll")
        || path.ends_with(".pdb")
        || path.ends_with(".zip")
        || path.starts_with("local/")
        || path.starts_with("artifacts/")
        || path.starts_with("dist/")
        || path.starts_with(".env")
    {
        return Some(format!("Disallowed public artifact: {path}"));
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return if path.ends_with(".ttf") || path.ends_with(".png") {
            None
        } else {
            Some(format!("Unexpected binary source: {path}"))
        };
    };
    let markers = ["ghp", "gho", "ghu", "ghs", "ghr"].map(|prefix| format!("{prefix}_"));
    for (index, line) in text.lines().enumerate() {
        let token = markers.iter().any(|marker| {
            line.split(marker).skip(1).any(|tail| {
                tail.chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .count()
                    >= 30
            })
        });
        let private_key = ["PRIVATE KEY", "RSA PRIVATE KEY", "OPENSSH PRIVATE KEY"]
            .iter()
            .any(|kind| line.contains(&format!("-----BEGIN {kind}-----")));
        let long_token = line
            .split(&format!("{}{}", "github", "_pat_"))
            .skip(1)
            .any(|tail| {
                tail.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || c == &'_')
                    .count()
                    >= 30
            });
        let aws = line
            .split(&format!("{}{}", "AK", "IA"))
            .skip(1)
            .any(|tail| {
                tail.chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                    .count()
                    >= 16
            });
        if token || private_key || long_token || aws {
            return Some(format!("Credential-shaped content: {path}:{}", index + 1));
        }
    }
    None
}
fn run() -> Result<(), String> {
    let files = git(&[
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    ])?;
    let mut checked = BTreeSet::new();
    for path in files.split(|byte| *byte == 0).filter(|p| !p.is_empty()) {
        let path = std::str::from_utf8(path).map_err(|e| e.to_string())?;
        if checked.insert(path.to_owned()) {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            if let Some(error) = finding(path, &bytes) {
                return Err(error);
            }
        }
    }
    let revisions = git(&["rev-list", "--all"])?;
    let mut count = 0;
    for revision in String::from_utf8(revisions)
        .map_err(|e| e.to_string())?
        .lines()
    {
        count += 1;
        let files = git(&["ls-tree", "-r", "--name-only", "-z", revision])?;
        for path in files.split(|byte| *byte == 0).filter(|p| !p.is_empty()) {
            let path = std::str::from_utf8(path).map_err(|e| e.to_string())?;
            let bytes = git(&["show", &format!("{revision}:{path}")])?;
            if let Some(error) = finding(path, &bytes) {
                return Err(error);
            }
        }
    }
    println!(
        "Public source boundary passed: {} worktree files, {count} revisions",
        checked.len()
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

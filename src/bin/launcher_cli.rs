//! Explicit CLI operations for package preparation and read-only launch checks.
use paladinscat_launcher::{engine, launch, performance};
use std::path::PathBuf;

fn run() -> engine::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--data-dir"] {
        println!("{}", engine::data_dir().display());
        return Ok(());
    }
    if args.len() == 2 && args[0] == "download-performance" {
        let cached =
            performance::download(&PathBuf::from(&args[1]), &|message| println!("{message}"))?;
        println!("{}", performance::manifest_path(&cached).display());
        return Ok(());
    }
    if args.len() >= 2 && matches!(args[0].as_str(), "--dry-run" | "--launch") {
        let mut options = launch::Options {
            game_root: PathBuf::from(&args[1]),
            tempest_cli: launch::default_tempest_cli(),
            language: "KOR".into(),
            homedir: "paladinslive".into(),
            performance: None,
            rollback_input: false,
        };
        let mut index = 2;
        while index < args.len() {
            let flag = &args[index];
            if flag == "--previous-input" {
                if options.rollback_input {
                    return Err("Duplicate previous-input option".into());
                }
                options.rollback_input = true;
                index += 1;
                continue;
            }
            let value = args.get(index + 1).ok_or("Missing option value")?;
            match flag.as_str() {
                "--performance" => {
                    options.performance = Some(performance::import(
                        &PathBuf::from(value),
                        &engine::data_dir(),
                    )?)
                }
                "--language" => options.language.clone_from(value),
                "--profile" => options.homedir.clone_from(value),
                "--tempest-cli" => options.tempest_cli = value.into(),
                _ => return Err("Unknown launcher option".into()),
            }
            index += 2;
        }
        if args[0] == "--dry-run" {
            let plan = launch::plan(&options)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "tempest_cli": plan.tempest_cli,
                    "game_root": plan.game_root,
                    "working_directory": plan.working_directory,
                    "args": plan.args,
                    "verified_dlls": plan.dlls,
                    "game_launched": false,
                    "hooks_verified": false
                }))
                .map_err(|e| e.to_string())?
            );
        } else {
            let result = launch::start(&options, &engine::data_dir().join("launch-logs"))?;
            println!("{}", result.status);
        }
        return Ok(());
    }
    Err("Usage: paladinscat-launcher-cli --data-dir | download-performance CACHE | --dry-run|--launch GAME_ROOT [--performance MANIFEST] [--previous-input] [--language KOR|INT] [--profile NAME] [--tempest-cli EXE]".into())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

// WD-40 CLI: scan and clean dev build artifacts from the terminal.
// Usage: wd40 [scan|clean|clean-old|help] [options]
use wd40::cleaner::{clean_all, clean_old, verified_age};
use wd40::config::Config;
use wd40::disk::sum_bytes;
use wd40::discover::scan_discover;
use wd40::scanner::{human_size, ArtifactGroup, TargetDir};
use wd40::qos::Priority;
use wd40::sizes::scan_sizes;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const SECONDS_PER_DAY: u64 = 86_400;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("scan");
    let rest = args.get(2..).unwrap_or(&[]);

    match cmd {
        "scan" | "s" => cmd_scan(rest),
        "clean" | "c" => cmd_clean(rest),
        "clean-old" | "co" => cmd_clean_old(rest),
        "help" | "-h" | "--help" => print_help(),
        other => {
            eprintln!("Unknown command: {}", other);
            print_help();
            std::process::exit(1);
        }
    }
}

fn load_targets(args: &[String]) -> (Vec<TargetDir>, HashMap<PathBuf, SystemTime>) {
    let mut config = Config::load();
    let group = parse_group_flag(args);
    // A filtered scan neither discovers nor measures the groups it will drop.
    if let Some(group) = group {
        config.scan_groups.retain(|key| key == group.key());
    }
    eprint!("Scanning...");
    let mut targets = scan_discover(&config);
    let measured_ages = scan_sizes(&mut targets, Priority::Waited);
    eprintln!(" found {} targets", targets.len());

    if let Some(group) = group {
        targets.retain(|t| t.kind.group() == group);
    }
    (targets, measured_ages)
}

fn cmd_scan(args: &[String]) {
    let (targets, _) = load_targets(args);
    print_table(&targets);
}

fn cmd_clean(args: &[String]) {
    let dry_run = has_flag(args, &["--dry-run", "-n"]);
    let (targets, _) = load_targets(args);

    if targets.is_empty() {
        println!("Nothing to clean.");
        return;
    }

    print_table(&targets);
    let total = sum_bytes(targets.iter().map(|t| t.size_bytes));

    if dry_run {
        println!("\n[dry-run] Would clean {} from {} dirs", human_size(total), targets.len());
        return;
    }

    println!("\nCleaning {} from {} dirs...", human_size(total), targets.len());
    let result = clean_all(&targets);
    print_result(result.freed_bytes, result.removed_count, &result.errors);
}

/// Say what was set aside for want of a measurement, and only when some was.
fn print_unmeasured(count: usize) {
    if count == 0 {
        return;
    }
    let sentence = if count == 1 {
        format!("{count} target could not be measured, so its age is unknown and it was left alone.")
    } else {
        format!("{count} targets could not be measured, so their ages are unknown and they were left alone.")
    };
    println!("{sentence}");
}

fn cmd_clean_old(args: &[String]) {
    let config = Config::load();
    let dry_run = has_flag(args, &["--dry-run", "-n"]);
    let days = parse_days_flag(args).unwrap_or(config.max_age_days);
    let max_age = Duration::from_secs(days.saturating_mul(SECONDS_PER_DAY));
    let (targets, measured_ages) = load_targets(args);
    let now = SystemTime::now();

    let old: Vec<(&TargetDir, Duration)> = targets
        .iter()
        .filter_map(|target| verified_age(target, &measured_ages, now).map(|age| (target, age)))
        .filter(|(_, age)| *age >= max_age)
        .collect();
    // A target whose walk produced no age is not young, it is unknown — and
    // saying nothing about it turns "none are old" into a false statement.
    let unmeasured = targets
        .iter()
        .filter(|target| verified_age(target, &measured_ages, now).is_none())
        .count();

    if old.is_empty() {
        println!("No targets older than {} days.", days);
        print_unmeasured(unmeasured);
        return;
    }

    println!("Targets older than {} days:", days);
    for (target, age) in &old {
        let age_days = age.as_secs() / SECONDS_PER_DAY;
        println!(
            "  {:>8}  {:>3}d  [{}]  {}",
            human_size(target.size_bytes),
            age_days,
            target.kind.label(),
            target.path.display()
        );
    }

    print_unmeasured(unmeasured);

    let total = sum_bytes(old.iter().map(|(target, _)| target.size_bytes));

    if dry_run {
        println!("\n[dry-run] Would clean {} from {} dirs", human_size(total), old.len());
        return;
    }

    println!("\nCleaning {} from {} dirs...", human_size(total), old.len());
    let result = clean_old(&targets, &measured_ages, max_age);
    print_result(result.freed_bytes, result.removed_count, &result.errors);
}

fn print_result(freed: u64, count: usize, errors: &[(std::path::PathBuf, String)]) {
    println!("Done: freed {} from {} dirs ({} errors)", human_size(freed), count, errors.len());
    for (path, err) in errors {
        eprintln!("  error: {}: {}", path.display(), err);
    }
}

fn has_flag(args: &[String], flags: &[&str]) -> bool {
    args.iter().any(|a| flags.contains(&a.as_str()))
}

fn parse_group_flag(args: &[String]) -> Option<ArtifactGroup> {
    for (i, arg) in args.iter().enumerate() {
        if (arg == "--group" || arg == "-g") && i + 1 < args.len() {
            return match args[i + 1].to_lowercase().as_str() {
                "rust" | "r" => Some(ArtifactGroup::Rust),
                "node" | "n" | "node_modules" => Some(ArtifactGroup::NodeModules),
                "build" | "b" => Some(ArtifactGroup::BuildOutput),
                "cache" | "caches" => Some(ArtifactGroup::Caches),
                "simulators" | "simulator" | "sim" => Some(ArtifactGroup::Simulators),
                "toolchains" | "toolchain" | "t" => Some(ArtifactGroup::Toolchains),
                _ => None,
            };
        }
    }
    None
}

fn parse_days_flag(args: &[String]) -> Option<u64> {
    for (i, arg) in args.iter().enumerate() {
        if (arg == "--days" || arg == "-d") && i + 1 < args.len() {
            let days: u64 = args[i + 1].parse().ok()?;
            if days == 0 {
                eprintln!("Error: --days must be >= 1");
                std::process::exit(1);
            }
            return Some(days);
        }
    }
    None
}

fn print_table(targets: &[TargetDir]) {
    if targets.is_empty() {
        println!("No targets found.");
        return;
    }

    let mut total: u64 = 0;
    let now = std::time::SystemTime::now();

    for &group in ArtifactGroup::ALL {
        let items: Vec<&TargetDir> = targets.iter().filter(|t| t.kind.group() == group).collect();
        if items.is_empty() {
            continue;
        }
        let group_size = sum_bytes(items.iter().map(|t| t.size_bytes));
        total = total.saturating_add(group_size);
        println!("\n{} — {}", group.label(), human_size(group_size));
        for t in &items {
            let age_days = now
                .duration_since(t.last_modified)
                .unwrap_or(Duration::ZERO)
                .as_secs()
                / SECONDS_PER_DAY;
            println!(
                "  {:>8}  {:>3}d  [{}]  {}",
                human_size(t.size_bytes),
                age_days,
                t.kind.label(),
                if matches!(t.kind, wd40::scanner::ArtifactKind::Simulator) {
                    format!("{}  {}", wd40::simulators::label(&t.path), t.path.display())
                } else { t.path.display().to_string() }
            );
        }
    }
    println!("\nTotal: {} in {} targets", human_size(total), targets.len());
}

fn print_help() {
    println!(
        "WD-40 — dev artifact cleaner

USAGE:
    wd40 [command] [options]

COMMANDS:
    scan, s            Scan and display all artifact directories (default)
    clean, c           Remove all artifact directories
    clean-old, co      Remove artifacts older than N days
    help, -h           Show this help

OPTIONS:
    -g, --group <type> Filter by group: rust, node, build, caches,
                       simulators, toolchains
    -d, --days <N>     Age threshold for clean-old (default: from config)
    -n, --dry-run      Show what would be cleaned without deleting

EXAMPLES:
    wd40                     Scan and show all artifacts
    wd40 scan -g rust        Show only Rust targets
    wd40 clean -g node       Clean all node_modules
    wd40 scan -g caches      Show only regenerable caches
    wd40 clean-old -d 14     Clean artifacts older than 14 days
    wd40 clean --dry-run     Preview what would be cleaned

CONFIG:
    ~/.config/wd-40/config.toml"
    );
}

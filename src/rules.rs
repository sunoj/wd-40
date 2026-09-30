// Name-gated artifact validation. Never probe these markers for arbitrary walk entries.
use crate::roots;
use std::path::Path;

pub(crate) fn is_dev_artifact(path: &Path, name: &str) -> bool {
    let Some(parent) = path.parent() else { return false };
    match name {
        "target" => roots::is_cargo_target(path),
        "node_modules" => {
            parent.join("package.json").is_file()
                || [".package-lock.json", ".yarn-integrity", ".modules.yaml"]
                    .iter().any(|name| path.join(name).is_file())
                || [".pnpm", ".bin"].iter().any(|name| path.join(name).is_dir())
        }
        ".next" => path.join("cache").is_dir() || path.join("static").is_dir(),
        "build" => is_build_project(parent) || has_xcodeproj(parent)
            || path.join("CMakeCache.txt").is_file(),
        "dist" => is_build_project(parent),
        ".build" => parent.join("Package.swift").is_file(),
        ".gradle" => has_any(parent, &["build.gradle", "build.gradle.kts",
            "settings.gradle", "settings.gradle.kts"]),
        ".turbo" | ".svelte-kit" | ".parcel-cache" | ".nuxt" => parent.join("package.json").is_file(),
        ".angular" => parent.join("angular.json").is_file(),
        ".mypy_cache" | ".pytest_cache" | ".ruff_cache" => has_cache_tag(path),
        ".tox" => has_any(parent, &["tox.ini", "tox.toml"]),
        ".nox" => parent.join("noxfile.py").is_file(),
        "zig-cache" | ".zig-cache" | "zig-out" => parent.join("build.zig").is_file(),
        ".dart_tool" => parent.join("pubspec.yaml").is_file(),
        "out" => parent.join("foundry.toml").is_file(),
        "artifacts" => has_any(parent, &["hardhat.config.js", "hardhat.config.ts",
            "hardhat.config.cjs", "hardhat.config.mjs"]),
        _ => false,
    }
}

fn has_any(parent: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| parent.join(name).is_file())
}

fn is_build_project(parent: &Path) -> bool {
    has_any(parent, &[
        "package.json", "Cargo.toml", "build.gradle", "build.gradle.kts",
        "platformio.ini",
    ])
}

fn has_xcodeproj(parent: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(parent) else { return false };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        !path.is_symlink() && path.is_dir() && path.extension().is_some_and(|ext| ext == "xcodeproj")
    })
}

fn has_cache_tag(path: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path.join("CACHEDIR.TAG")) else { return false };
    let mut bytes = Vec::new();
    if file.take(80).read_to_end(&mut bytes).is_err() { return false; }
    const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";
    bytes.starts_with(SIGNATURE)
        && bytes.get(SIGNATURE.len()..).is_some_and(|tail| {
            tail.is_empty() || tail.starts_with(b"\n") || tail.starts_with(b"\r\n")
        })
}

#[cfg(test)]
mod tests {
    use super::is_dev_artifact;
    use std::fs;

    #[test]
    fn every_new_name_requires_its_marker() {
        let root = std::env::temp_dir().join(format!("wd40-rules-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (index, (name, marker)) in [
            (".gradle", "build.gradle.kts"), ("build", "build.gradle"),
            (".turbo", "package.json"), (".svelte-kit", "package.json"),
            (".parcel-cache", "package.json"), (".nuxt", "package.json"),
            (".angular", "angular.json"), (".mypy_cache", ".mypy_cache/CACHEDIR.TAG"),
            (".pytest_cache", ".pytest_cache/CACHEDIR.TAG"),
            (".ruff_cache", ".ruff_cache/CACHEDIR.TAG"),
            (".tox", "tox.ini"), (".tox", "tox.toml"), (".nox", "noxfile.py"),
            ("zig-cache", "build.zig"), (".zig-cache", "build.zig"),
            ("zig-out", "build.zig"), (".dart_tool", "pubspec.yaml"),
            ("out", "foundry.toml"), ("artifacts", "hardhat.config.cjs"),
            ("build", "build/CMakeCache.txt"),
        ].iter().enumerate() {
            let project = root.join(index.to_string());
            let artifact = project.join(name);
            fs::create_dir_all(&artifact).unwrap();
            assert!(!is_dev_artifact(&artifact, name), "{name} without marker");
            let marker_path = project.join(marker);
            if *marker == format!("{name}/CACHEDIR.TAG") {
                fs::write(&marker_path, "not a cache").unwrap();
                assert!(!is_dev_artifact(&artifact, name), "{name} with invalid tag");
                fs::write(marker_path, "Signature: 8a477f597d28d172789f06886806bc55\n").unwrap();
            } else {
                fs::write(marker_path, "").unwrap();
            }
            assert!(is_dev_artifact(&artifact, name), "{name} with marker");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn python_tool_files_and_marker_variants() {
        let root = std::env::temp_dir().join(format!("wd40-python-rules-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (name, file) in [
            (".mypy_cache", "3.12/meta.json"),
            (".pytest_cache", "v/cache/nodeids"),
            (".ruff_cache", "0.13.0/hash"),
        ] {
            let path = root.join(name);
            fs::create_dir_all(path.join(file).parent().unwrap()).unwrap();
            fs::write(path.join(file), "").unwrap();
            assert!(!is_dev_artifact(&path, name), "{name} without CACHEDIR.TAG");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn alternate_project_manifests_are_recognized() {
        let root = std::env::temp_dir().join(format!("wd40-rule-variants-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (index, (name, marker)) in [
            (".gradle", "settings.gradle"), (".gradle", "settings.gradle.kts"),
            ("build", "build.gradle.kts"),
            ("artifacts", "hardhat.config.js"), ("artifacts", "hardhat.config.ts"),
            ("artifacts", "hardhat.config.mjs"),
        ].iter().enumerate() {
            let project = root.join(index.to_string());
            let artifact = project.join(name);
            fs::create_dir_all(&artifact).unwrap();
            assert!(!is_dev_artifact(&artifact, name));
            fs::write(project.join(marker), "").unwrap();
            assert!(is_dev_artifact(&artifact, name));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removed_project_markers_do_not_qualify() {
        let root = std::env::temp_dir().join(format!("wd40-rule-rejected-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (index, (name, marker)) in [
            ("build", "settings.gradle"), ("build", "settings.gradle.kts"),
            (".tox", "pyproject.toml"), (".tox", "noxfile.py"),
            (".nox", "pyproject.toml"), (".nox", "tox.ini"), (".nox", "tox.toml"),
        ].iter().enumerate() {
            let project = root.join(index.to_string());
            let artifact = project.join(name);
            fs::create_dir_all(&artifact).unwrap();
            fs::write(project.join(marker), "").unwrap();
            assert!(!is_dev_artifact(&artifact, name), "{name} with only {marker}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_tag_signature_must_be_exact() {
        let root = std::env::temp_dir().join(format!("wd40-tag-rule-{}", std::process::id()));
        let cache = root.join(".mypy_cache");
        fs::create_dir_all(&cache).unwrap();
        let tag = cache.join("CACHEDIR.TAG");
        fs::write(&tag, "Signature: 8a477f597d28d172789f06886806bc55extra").unwrap();
        assert!(!is_dev_artifact(&cache, ".mypy_cache"));
        fs::write(&tag, "Signature: 8a477f597d28d172789f06886806bc55").unwrap();
        assert!(is_dev_artifact(&cache, ".mypy_cache"));
        fs::remove_dir_all(root).unwrap();
    }
}

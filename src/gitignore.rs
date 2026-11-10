// Parent-directory .gitignore rules for build/ and dist/ when no manifest
// marks the project. No git subprocess — reads .gitignore only. Exports:
// `parent_gitignore_lists`. Deps: std only.

use std::path::Path;

/// True when `parent/.gitignore` lists `name` as an ignored entry and does not
/// re-include it or anything under it via `!name`, `!name/…` or `!/name/…`.
/// As in git, `#` starts a comment only at the start of a line.
pub(crate) fn parent_gitignore_lists(parent: &Path, name: &str) -> bool {
    let Ok(raw) = std::fs::read_to_string(parent.join(".gitignore")) else {
        return false;
    };
    let lines: Vec<&str> = raw.lines().collect();
    if !lines.iter().any(|line| ignore_line_lists_name(line, name)) {
        return false;
    }
    !lines.iter().any(|line| negation_reincludes_under(line, name))
}

fn ignore_line_lists_name(line: &str, name: &str) -> bool {
    let Some(pattern) = trimmed_pattern(line) else { return false };
    pattern == name
        || pattern == format!("{name}/")
        || pattern == format!("/{name}")
        || pattern == format!("/{name}/")
}

fn trimmed_pattern(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    Some(line)
}

fn negation_reincludes_under(line: &str, name: &str) -> bool {
    let Some(pattern) = line.trim().strip_prefix('!') else { return false };
    let pattern = pattern.trim().trim_start_matches('/');
    pattern == name || pattern.starts_with(&format!("{name}/"))
}

#[cfg(test)]
mod tests {
    use super::parent_gitignore_lists;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("wd40-gitignore-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn write_ignore(dir: &std::path::Path, text: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(".gitignore"), text).unwrap();
    }

    #[test]
    fn listed_build_name_variants_qualify_dist_and_build() {
        let root = scratch("variants");
        for (index, (artifact, line)) in [
            ("build", "build"), ("build", "build/"), ("build", "/build"), ("build", "/build/"),
            ("dist", "dist"), ("dist", "dist/"),
        ].iter().enumerate() {
            let parent = root.join(index.to_string());
            write_ignore(&parent, line);
            fs::create_dir_all(parent.join(artifact)).unwrap();
            assert!(parent_gitignore_lists(&parent, artifact), "{line}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn comments_only_start_a_line() {
        let root = scratch("comments");
        write_ignore(&root, "# build/\n  build/  \n");
        assert!(parent_gitignore_lists(&root, "build"));
        write_ignore(&root, "# build/\nbuild # not a comment in git\n");
        assert!(!parent_gitignore_lists(&root, "build"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn negation_under_the_name_disqualifies() {
        let root = scratch("negation");
        write_ignore(&root, "build/\n!build/keep/\n");
        assert!(!parent_gitignore_lists(&root, "build"));
        write_ignore(&root, "build/\n!/build/keep/\n");
        assert!(!parent_gitignore_lists(&root, "build"));
        write_ignore(&root, "build/\n!build\n");
        assert!(!parent_gitignore_lists(&root, "build"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ancestor_gitignore_does_not_qualify_nested_build() {
        let root = scratch("ancestor");
        write_ignore(&root, "build/\n!model/build/\n");
        let model = root.join("model");
        fs::create_dir_all(model.join("build")).unwrap();
        assert!(!parent_gitignore_lists(&model, "build"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_exact_name_patterns_count() {
        let root = scratch("exact");
        write_ignore(&root, "build-output/\n**/build\n");
        assert!(!parent_gitignore_lists(&root, "build"));
        fs::remove_dir_all(root).unwrap();
    }
}

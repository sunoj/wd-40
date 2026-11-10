// Regression tests for scan-coverage gap fixes. Deps: crate::{config, discover, roots, scanner}.

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::discover::scan_discover;
    use crate::roots::collect_shipit_caches;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("wd40-scan-gap-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn aid_worktree_layout_finds_node_modules_at_depth_five() {
        let root = scratch("aid-depth");
        let checkout = root.join("proj/type/desc/app");
        let node_modules = checkout.join("node_modules");
        fs::create_dir_all(node_modules.join(".bin")).unwrap();
        fs::write(checkout.join("package.json"), "{}").unwrap();
        let config = Config { scan_dirs: vec![root.clone()], ..Config::default() };
        assert!(scan_discover(&config).iter().any(|item| item.path == node_modules));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shipit_siblings_under_library_caches_are_collected() {
        let home = scratch("shipit");
        let caches = home.join("Library/Caches");
        fs::create_dir_all(caches.join("com.example.App.ShipIt")).unwrap();
        fs::create_dir_all(caches.join("org.other.ShipIt")).unwrap();
        fs::create_dir_all(caches.join("unrelated")).unwrap();
        let mut found = Vec::new();
        collect_shipit_caches(&mut found, &caches);
        let paths: Vec<_> = found.into_iter().map(|item| item.path).collect();
        assert!(paths.contains(&caches.join("com.example.App.ShipIt")));
        assert!(paths.contains(&caches.join("org.other.ShipIt")));
        assert!(!paths.contains(&caches.join("unrelated")));
        let _ = fs::remove_dir_all(home);
    }
}

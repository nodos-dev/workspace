fn main() {
    let client = nodos_store_client::StoreClient::builder().build().unwrap();
    let pkgs = client.list_packages().expect("list packages");
    let pkg_names: std::collections::HashSet<_> = pkgs.iter().map(|p| p.name.as_str()).collect();

    // Find nos.sys.vulkan versions where all deps have cross-platform artifacts matching the dep version prefix
    let dep_has_cross_platform = |dep_name: &str, dep_version_prefix: &str| -> bool {
        if let Ok(dep_rels) = client.get_releases(dep_name) {
            dep_rels.iter().any(|dr| {
                dr.version.starts_with(dep_version_prefix) && {
                    let p: Vec<_> = dr.artifacts.iter().map(|a| a.target_platform.as_str()).collect();
                    p.contains(&"x86_64-windows") && p.contains(&"x86_64-linux")
                }
            })
        } else { false }
    };

    if let Ok(mut releases) = client.get_releases("nos.sys.vulkan") {
        releases.sort_by(|a, b| b.version.cmp(&a.version));
        for r in &releases {
            if r.dependencies.is_empty() { continue; }
            let all_public = r.dependencies.iter().all(|d| pkg_names.contains(d.name.as_str()));
            if !all_public { continue; }
            let all_cross = r.dependencies.iter().all(|d| dep_has_cross_platform(&d.name, &d.version));
            if all_cross {
                println!("USABLE: nos.sys.vulkan {} deps={:?}", r.version,
                    r.dependencies.iter().map(|d| format!("{} {}", d.name, d.version)).collect::<Vec<_>>());
            }
        }
    }

    // Find any package that has at least one release with non-empty deps all on server
    for pkg in &pkgs {
        if let Ok(releases) = client.get_releases(&pkg.name) {
            for r in &releases {
                if r.dependencies.is_empty() { continue; }
                let all_available = r.dependencies.iter().all(|d| pkg_names.contains(d.name.as_str()));
                if all_available {
                    println!("FOUND: {} {}: deps={:?}", pkg.name, r.version,
                        r.dependencies.iter().map(|d| format!("{} {}", d.name, d.version)).collect::<Vec<_>>());
                    break;
                }
            }
        }
    }
}

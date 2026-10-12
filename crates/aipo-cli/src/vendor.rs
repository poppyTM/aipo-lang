//! Offline dependency export with rewritten local manifests and source inventory.
use aipo_package::{LOCK_FILE_NAME, Lockfile, MANIFEST_FILE_NAME, Manifest, PackageSource};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn run(args: &[String], out: &mut dyn Write) -> Result<u8, String> {
    if args.len() < 3 {
        return Err(
            "usage: aipo package vendor <root> --out <new-directory> [--cache <directory>]".into(),
        );
    }
    let root = std::fs::canonicalize(&args[0]).map_err(|e| e.to_string())?;
    let mut destination = None;
    let mut cache = None;
    let mut index = 1;
    while index < args.len() {
        let target = match args[index].as_str() {
            "--out" => &mut destination,
            "--cache" => &mut cache,
            _ => return Err("unknown vendor option".into()),
        };
        index += 1;
        *target = Some(PathBuf::from(
            args.get(index).ok_or("missing vendor option value")?,
        ));
        index += 1;
    }
    let destination = destination.ok_or("--out is required")?;
    if destination.symlink_metadata().is_ok() {
        return Err("vendor destination already exists".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or("destination requires a name")?,
    );
    let manifest = Manifest::from_bytes(
        &std::fs::read(root.join(MANIFEST_FILE_NAME)).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let lock =
        Lockfile::from_bytes(&std::fs::read(root.join(LOCK_FILE_NAME)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let local = aipo_package::LocalPackageResolver::new(&root)
        .resolve_with_paths()
        .map_err(|e| e.to_string());
    let paths = if let Some(cache) = cache {
        let discovered = aipo_package::LocalPackageResolver::new(&root)
            .discover_local()
            .map_err(|e| e.to_string())?;
        let input = discovered
            .inputs
            .get(&manifest.name)
            .cloned()
            .ok_or("missing root input")?;
        let store = aipo_package::GitHubCache::new(cache);
        let fetcher = aipo_package::CacheOnlyGitHubFetcher::new(&store);
        let resolved = aipo_package::resolve_mixed_package_graph(
            input,
            discovered.inputs,
            discovered.paths,
            &store,
            &fetcher,
        )
        .map_err(|e| e.to_string())?;
        if resolved.graph.to_lockfile().map_err(|e| e.to_string())? != lock {
            return Err("lockfile does not match verified package graph".into());
        }
        resolved.paths
    } else {
        let resolved = local?;
        if resolved.graph.to_lockfile().map_err(|e| e.to_string())? != lock {
            return Err("lockfile does not match local package graph".into());
        }
        resolved.paths
    };
    let staging = parent.join(format!(
        ".aipo-vendor-{}-{}",
        std::process::id(),
        crate::NEXT_LOCK_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        let mut locations = BTreeMap::new();
        for coordinate in paths.keys() {
            let relative = if coordinate == &manifest.name {
                PathBuf::from("root")
            } else {
                PathBuf::from("packages").join(coordinate.as_str())
            };
            locations.insert(coordinate.clone(), relative);
        }
        let mut inventory = BTreeMap::new();
        for (coordinate, entry) in &paths {
            let manifest_path =
                crate::find_ancestor_manifest(entry).ok_or("package entry has no manifest")?;
            let source = manifest_path
                .parent()
                .ok_or("package manifest has no directory")?;
            let target = staging.join(&locations[coordinate]);
            copy(source, &target, &staging, &destination, 0)?;
            let mut copied = Manifest::from_bytes(
                &std::fs::read(target.join(MANIFEST_FILE_NAME)).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            for dependency in &mut copied.dependencies {
                let dependent = locations
                    .get(&dependency.name)
                    .ok_or("dependency absent from verified graph")?;
                let depth = locations[coordinate].components().count();
                let mut relative = PathBuf::new();
                for _ in 0..depth {
                    relative.push("..");
                }
                relative.push(dependent);
                dependency.source = PackageSource::Path {
                    path: relative.to_string_lossy().replace('\\', "/"),
                };
            }
            std::fs::write(
                target.join(MANIFEST_FILE_NAME),
                copied.to_toml().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(target.join(LOCK_FILE_NAME));
        }
        let resolved = aipo_package::LocalPackageResolver::new(staging.join("root"))
            .resolve_with_paths()
            .map_err(|e| e.to_string())?;
        let vendored_lock = resolved.graph.to_lockfile().map_err(|e| e.to_string())?;
        std::fs::write(
            staging.join("root").join(LOCK_FILE_NAME),
            vendored_lock.to_toml().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        // Hash the final files after rewriting manifests and producing the local lock.
        inventory.clear();
        inventory_files(&staging, &staging, &mut inventory)?;
        std::fs::write(
            staging.join("vendor-manifest.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"schema":1,"original_lock":lock,"files_sha256":inventory}),
            )
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&staging, &destination).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result?;
    writeln!(
        out,
        "offline package root: {}",
        destination.join("root").display()
    )
    .map_err(|e| e.to_string())?;
    Ok(0)
}
fn copy(
    source: &Path,
    target: &Path,
    staging: &Path,
    destination: &Path,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("package directory depth exceeds 64".into());
    }
    std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.')
            || matches!(name.to_str(), Some("target" | "node_modules"))
        {
            continue;
        }
        let path = entry.path();
        if path == staging || path == destination {
            continue;
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "package symlinks are not vendored: {}",
                path.display()
            ));
        }
        let target = target.join(name);
        if kind.is_dir() {
            copy(&path, &target, staging, destination, depth + 1)?;
        } else if kind.is_file() {
            std::fs::copy(path, &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn inventory_files(
    root: &Path,
    dir: &Path,
    inventory: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            inventory_files(root, &path, inventory)?;
        } else {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            inventory.insert(
                path.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
                Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            );
        }
    }
    Ok(())
}

use std::path::PathBuf;

use package_manager::fetcher::{PackageFetcher, RepositoryClient};
use package_manager::install::InstallPipeline;
use package_manager::snapshot::SnapshotManager;

const DB_PATH: &str = "/var/lib/turnix/packages.json";
const SNAPSHOT_DIR: &str = "/var/lib/turnix/snapshots";
const STAGING_DIR: &str = "/var/tmp/turnix-staging";

fn usage() {
    eprintln!("Usage: tpkg <command> [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  install <package>    Install a package");
    eprintln!("  remove  <package>    Remove a package");
    eprintln!("  upgrade <package>    Upgrade a package");
    eprintln!("  list                 List installed packages");
    eprintln!("  search  <query>      Search installed packages");
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        std::process::exit(1);
    }

    let snap_dir = PathBuf::from(SNAPSHOT_DIR);

    // Build shared components.
    let snap_mgr = SnapshotManager::new(snap_dir);
    let pipeline = match build_pipeline() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: cannot initialise package manager: {e}");
            std::process::exit(1);
        }
    };

    match args[1].as_str() {
        "install" if args.len() >= 3 => {
            let name = &args[2];
            let manifest_path = format!("/etc/turnix/packages/{name}.toml");
            let manifest_content = match std::fs::read_to_string(&manifest_path) {
                Ok(c) => c,
                Err(_) => {
                    eprintln!("Error: no manifest found for package '{name}' at {manifest_path}");
                    std::process::exit(1);
                }
            };
            let manifest = match turnix_tpkg_format::TpkgManifest::parse(&manifest_content) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Error: invalid manifest for '{name}': {e}");
                    std::process::exit(1);
                }
            };
            let version = semver::Version::parse(&manifest.package.version).unwrap_or_else(|_| {
                eprintln!("Error: invalid version in manifest for '{name}'");
                std::process::exit(1);
            });
            let resolved = turnix_tpkg_format::ResolvedPackage {
                name: manifest.package.name.clone(),
                version,
                manifest,
                source: turnix_tpkg_format::PackageSource::Repository {
                    url: format!("https://packages.turnix.org/{name}.tpkg"),
                    checksum: String::new(),
                },
                dependencies: vec![],
            };
            match pipeline.install(&resolved).await {
                Ok(snap_id) => {
                    println!("Installed {name} (snapshot: {snap_id})");
                }
                Err(e) => {
                    eprintln!("Install failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        "remove" if args.len() >= 3 => {
            let name = &args[2];
            match pipeline.remove(name) {
                Ok(snap_id) => {
                    println!("Removed {name} (snapshot: {snap_id})");
                }
                Err(e) => {
                    eprintln!("Remove failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        "upgrade" if args.len() >= 3 => {
            let name = &args[2];
            let manifest_path = format!("/etc/turnix/packages/{name}.toml");
            let manifest_content = match std::fs::read_to_string(&manifest_path) {
                Ok(c) => c,
                Err(_) => {
                    eprintln!("Error: no manifest found for package '{name}' at {manifest_path}");
                    std::process::exit(1);
                }
            };
            let manifest = match turnix_tpkg_format::TpkgManifest::parse(&manifest_content) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Error: invalid manifest for '{name}': {e}");
                    std::process::exit(1);
                }
            };
            let version = semver::Version::parse(&manifest.package.version).unwrap_or_else(|_| {
                eprintln!("Error: invalid version in manifest for '{name}'");
                std::process::exit(1);
            });
            let resolved = turnix_tpkg_format::ResolvedPackage {
                name: manifest.package.name.clone(),
                version,
                manifest,
                source: turnix_tpkg_format::PackageSource::Repository {
                    url: format!("https://packages.turnix.org/{name}.tpkg"),
                    checksum: String::new(),
                },
                dependencies: vec![],
            };
            match pipeline.upgrade(&resolved).await {
                Ok(snap_id) => {
                    println!("Upgraded {name} (snapshot: {snap_id})");
                }
                Err(e) => {
                    eprintln!("Upgrade failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        "list" => {
            match pipeline.list_installed() {
                Ok(packages) => {
                    if packages.is_empty() {
                        println!("No packages installed.");
                    } else {
                        for pkg in &packages {
                            println!("{} v{}", pkg.name, pkg.version);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error listing packages: {e}");
                    std::process::exit(1);
                }
            }
        }
        "search" if args.len() >= 3 => {
            let query = &args[2];
            match pipeline.search(query) {
                Ok(results) => {
                    if results.is_empty() {
                        println!("No packages match '{query}'.");
                    } else {
                        for pkg in &results {
                            println!("{} v{}", pkg.name, pkg.version);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error searching packages: {e}");
                    std::process::exit(1);
                }
            }
        }
        "list-snapshots" => {
            match snap_mgr.list_snapshots() {
                Ok(snapshots) => {
                    if snapshots.is_empty() {
                        println!("No snapshots found.");
                    } else {
                        for snap_id in &snapshots {
                            println!("{snap_id}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error listing snapshots: {e}");
                    std::process::exit(1);
                }
            }
        }
        "rollback" if args.len() >= 3 => {
            let snap_id = package_manager::snapshot::SnapshotId::new(&args[2]);
            match snap_mgr.rollback(&snap_id) {
                Ok(()) => {
                    println!("Rolled back to snapshot {snap_id}");
                }
                Err(e) => {
                    eprintln!("Rollback failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}

fn build_pipeline() -> Result<InstallPipeline, String> {
    let root_metadata = std::fs::read("/etc/turnix/tuf/root.json")
        .map_err(|e| format!("cannot read TUF root metadata: {e}"))?;
    let meta_url = url::Url::parse("https://packages.turnix.org/tuf/metadata")
        .map_err(|e| format!("invalid metadata URL: {e}"))?;
    let target_url = url::Url::parse("https://packages.turnix.org/tuf/targets")
        .map_err(|e| format!("invalid targets URL: {e}"))?;

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("cannot create runtime: {e}"))?;
    let client = rt.block_on(RepositoryClient::new(root_metadata, meta_url, target_url))
        .map_err(|e| format!("cannot connect to TUF repository: {e}"))?;

    let fetcher = PackageFetcher::new(client);
    let snap_mgr = SnapshotManager::new(PathBuf::from(SNAPSHOT_DIR));
    let db_path = PathBuf::from(DB_PATH);
    let staging = PathBuf::from(STAGING_DIR);

    Ok(InstallPipeline::new(fetcher, snap_mgr, db_path, staging))
}

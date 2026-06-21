use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(serde::Deserialize, Debug)]
struct Workflow {
    #[serde(default)]
    jobs: serde_yaml::Value,
}

fn workflow_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root should exist")
        .join(".github")
        .join("workflows")
        .join("ci.yml")
}

fn load_workflow() -> Workflow {
    let path = workflow_path();
    let contents =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_yaml::from_str(&contents).unwrap_or_else(|e| panic!("failed to parse ci.yml: {e}"))
}

#[test]
fn ci_yml_parses_without_errors() {
    let wf = load_workflow();
    // If we got here, YAML parsing succeeded. Verify jobs is a mapping.
    assert!(
        wf.jobs.is_mapping(),
        "jobs should be a YAML mapping, got {:?}",
        wf.jobs
    );
}

#[test]
fn ci_yml_has_all_required_jobs() {
    let wf = load_workflow();
    let jobs = wf.jobs.as_mapping().expect("jobs must be a mapping");

    let required = [
        "build",
        "unit-tests",
        "boot-gate",
        "driver-tests",
        "security-regression",
        "performance-benchmarks",
    ];

    let present: HashSet<_> = jobs.keys().filter_map(|k| k.as_str()).collect();

    let missing: Vec<_> = required
        .iter()
        .filter(|name| !present.contains(*name))
        .copied()
        .collect();

    assert!(
        missing.is_empty(),
        "Missing required jobs: {missing:?}. Present: {present:?}"
    );
}

#[test]
fn ci_yml_build_job_has_release_and_strict_warnings() {
    let path = workflow_path();
    let contents = fs::read_to_string(&path).expect("read ci.yml");
    // Verify the build job uses RUSTFLAGS="-D warnings" and --release
    assert!(
        contents.contains("RUSTFLAGS") && contents.contains("-D warnings"),
        "build job must set RUSTFLAGS=\"-D warnings\""
    );
    assert!(
        contents.contains("cargo build --release"),
        "build job must use --release"
    );
}

#[test]
fn ci_yml_unit_tests_job_runs_workspace_tests() {
    let path = workflow_path();
    let contents = fs::read_to_string(&path).expect("read ci.yml");
    assert!(
        contents.contains("cargo test --workspace"),
        "unit-tests job must run cargo test --workspace"
    );
}

#[test]
fn ci_yml_boot_gate_uses_xtask_ci_boot() {
    let path = workflow_path();
    let contents = fs::read_to_string(&path).expect("read ci.yml");
    assert!(
        contents.contains("cargo xtask ci-boot"),
        "boot-gate job must run cargo xtask ci-boot"
    );
}

#[test]
fn ci_yml_all_jobs_run_on_ubuntu_latest() {
    let path = workflow_path();
    let contents = fs::read_to_string(&path).expect("read ci.yml");
    // Count occurrences of ubuntu-latest — one per job
    let count = contents.matches("ubuntu-latest").count();
    assert!(
        count >= 6,
        "expected at least 6 ubuntu-latest runners (one per job), found {count}"
    );
}

#[test]
fn ci_yml_all_jobs_require_qemu_for_hardware_tests() {
    let path = workflow_path();
    let contents = fs::read_to_string(&path).expect("read ci.yml");
    // boot-gate, driver-tests, security-regression, performance-benchmarks need QEMU
    for job in &["boot-gate", "driver-tests", "security-regression", "performance-benchmarks"] {
        // Find the job section and verify it installs QEMU
        let section_start = contents
            .find(&format!("{job}:"))
            .unwrap_or_else(|| panic!("job '{job}' not found in ci.yml"));
        let section = &contents[section_start..];
        assert!(
            section.contains("qemu-system-x86") || section.contains("qemu-system-x86_64"),
            "job '{job}' must install or use qemu-system-x86"
        );
    }
}

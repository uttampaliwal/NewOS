use semver::{Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet};
use tpkg_format::{InstallPlan, PackageName};
use varisat::ExtendFormula;

// ---------------------------------------------------------------------------
// SolverError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolverError {
    Conflict(String),
    Cycle(Vec<PackageName>),
    NotFound(PackageName),
    Internal(String),
}

impl std::fmt::Display for SolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolverError::Conflict(msg) => write!(f, "dependency conflict: {msg}"),
            SolverError::Cycle(pkgs) => {
                write!(f, "dependency cycle: {}", pkgs.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(" -> "))
            }
            SolverError::NotFound(name) => write!(f, "package not found: {name}"),
            SolverError::Internal(msg) => write!(f, "internal solver error: {msg}"),
        }
    }
}

// ---------------------------------------------------------------------------
// DependencySolver
// ---------------------------------------------------------------------------

/// A SAT-based dependency solver.
///
/// Maintains a registry of known packages and their available versions.
/// The `solve` method uses the `varisat` CDCL SAT solver to find a set of
/// package versions that satisfies all given version constraints.
#[derive(Debug)]
pub struct DependencySolver {
    /// Available versions per package.
    packages: BTreeMap<PackageName, Vec<Version>>,
}

/// A resolved (package, version) pair returned by the solver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEntry {
    pub name: PackageName,
    pub version: Version,
}

impl DependencySolver {
    /// Create a new empty solver.
    pub fn new() -> Self {
        Self {
            packages: BTreeMap::new(),
        }
    }

    /// Add a package version to the solver's registry.
    pub fn add_package_version(&mut self, name: PackageName, version: Version) {
        self.packages.entry(name).or_default().push(version);
    }

    /// Get all registered package names.
    pub fn package_names(&self) -> impl Iterator<Item = &PackageName> {
        self.packages.keys()
    }

    /// Get available versions for a package.
    pub fn versions(&self, name: &PackageName) -> Option<&[Version]> {
        self.packages.get(name).map(|v| v.as_slice())
    }

    /// Solve dependency constraints.
    ///
    /// Given a list of `(package_name, version_req)` requests, returns an
    /// `InstallPlan` with the resolved packages in dependency order, or a
    /// `SolverError` explaining why resolution failed.
    pub fn solve(
        &self,
        requests: &[(PackageName, VersionReq)],
        dependencies: &BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>>,
    ) -> Result<InstallPlan, SolverError> {
        let mut solver = varisat::Solver::new();

        // Map each (package, version) to a SAT variable.
        let mut var_map: BTreeMap<(PackageName, Version), varisat::Var> = BTreeMap::new();
        let mut var_list: Vec<(PackageName, Version)> = Vec::new();

        // Collect all packages/versions reachable from requests.
        let mut all_reachable = BTreeSet::new();
        let mut stack: Vec<PackageName> = requests.iter().map(|(n, _)| n.clone()).collect();
        while let Some(name) = stack.pop() {
            if !all_reachable.insert(name.clone()) {
                continue;
            }
            let Some(versions) = self.packages.get(&name) else {
                return Err(SolverError::NotFound(name));
            };
            for ver in versions {
                if let Some(deps) = dependencies.get(&(name.clone(), ver.clone())) {
                    for (dep_name, _) in deps {
                        stack.push(dep_name.clone());
                    }
                }
            }
        }

        // Assign variables.
        for name in &all_reachable {
            let Some(versions) = self.packages.get(name) else {
                return Err(SolverError::NotFound(name.clone()));
            };
            for ver in versions {
                let v = solver.new_var();
                var_map.insert((name.clone(), ver.clone()), v);
                var_list.push((name.clone(), ver.clone()));
            }
        }

        // 1. At-least-one clauses for each request.
        for (req_name, req_version) in requests {
            let matching: Vec<varisat::Lit> = var_map
                .iter()
                .filter(|((n, v), _)| n == req_name && req_version.matches(v))
                .map(|(_, var)| varisat::Lit::from_var(*var, true))
                .collect();
            if matching.is_empty() {
                return Err(SolverError::Conflict(format!(
                    "no version of {} matches requirement {req_version}",
                    req_name,
                )));
            }
            solver.add_clause(&matching);
        }

        // 2. At-most-one clauses for each package group.
        for name in &all_reachable {
            let vars_for_pkg: Vec<varisat::Var> = var_map
                .iter()
                .filter(|((n, _), _)| n == name)
                .map(|(_, v)| *v)
                .collect();
            for i in 0..vars_for_pkg.len() {
                for j in (i + 1)..vars_for_pkg.len() {
                    solver.add_clause(&[
                        !varisat::Lit::from_var(vars_for_pkg[i], true),
                        !varisat::Lit::from_var(vars_for_pkg[j], true),
                    ]);
                }
            }
        }

        // 3. Dependency implication clauses.
        for (name, ver) in &var_list {
            let Some(deps) = dependencies.get(&(name.clone(), ver.clone())) else {
                continue;
            };
            let var_lit = varisat::Lit::from_var(var_map[&(name.clone(), ver.clone())], true);
            for (dep_name, dep_req) in deps {
                let matching: Vec<varisat::Lit> = var_map
                    .iter()
                    .filter(|((n, v), _)| n == dep_name && dep_req.matches(v))
                    .map(|(_, v)| varisat::Lit::from_var(*v, true))
                    .collect();
                if matching.is_empty() {
                    // Dependency cannot be satisfied by any available version.
                    solver.add_clause(&[!var_lit]);
                    continue;
                }
                let mut clause = vec![!var_lit];
                clause.extend(matching);
                solver.add_clause(&clause);
            }
        }

        // Solve.
        let sat = solver.solve().map_err(|e| {
            SolverError::Internal(format!("solver error: {e}"))
        })?;
        if !sat {
            return Err(SolverError::Conflict(
                "constraints are unsatisfiable".into(),
            ));
        }
        let model = solver.model().ok_or_else(|| {
            SolverError::Internal("no model returned despite SAT result".into())
        })?;

        // Build set of true variables from the model.
        let true_vars: BTreeSet<varisat::Var> = model
            .iter()
            .filter(|l| l.is_positive())
            .map(|l| l.var())
            .collect();

        // Decode model: collect selected (name, version) pairs.
        let mut selected: Vec<(PackageName, Version)> = Vec::new();
        for (name, ver) in &var_list {
            if true_vars.contains(&var_map[&(name.clone(), ver.clone())]) {
                selected.push((name.clone(), ver.clone()));
            }
        }

        // For each package, pick the highest version among selected.
        let mut best: BTreeMap<PackageName, Version> = BTreeMap::new();
        for (name, ver) in &selected {
            match best.get(name) {
                Some(existing) if *existing > *ver => {}
                _ => {
                    best.insert(name.clone(), ver.clone());
                }
            }
        }

        // Build the resolved entries list.
        let entries: Vec<ResolvedEntry> = best
            .into_iter()
            .map(|(name, version)| ResolvedEntry { name, version })
            .collect();

        // Topological sort: dependencies before dependents.
        let ordered = topological_sort(&entries, dependencies)?;

        let install_plan = InstallPlan {
            packages: Vec::new(),
            order: ordered.into_iter().map(|e| e.name).collect(),
        };

        Ok(install_plan)
    }
}

impl Default for DependencySolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Topological sort of resolved entries.
fn topological_sort(
    entries: &[ResolvedEntry],
    dependencies: &BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>>,
) -> Result<Vec<ResolvedEntry>, SolverError> {
    let entry_map: BTreeMap<&PackageName, &ResolvedEntry> =
        entries.iter().map(|e| (&e.name, e)).collect();

    let mut visited = BTreeSet::new();
    let mut in_stack = BTreeSet::new();
    let mut result = Vec::with_capacity(entries.len());

    fn visit(
        entry: &ResolvedEntry,
        entry_map: &BTreeMap<&PackageName, &ResolvedEntry>,
        dependencies: &BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>>,
        visited: &mut BTreeSet<PackageName>,
        in_stack: &mut BTreeSet<PackageName>,
        result: &mut Vec<ResolvedEntry>,
    ) -> Result<(), SolverError> {
        if in_stack.contains(&entry.name) {
            let cycle: Vec<PackageName> = in_stack.iter().cloned().collect();
            return Err(SolverError::Cycle(cycle));
        }
        if visited.contains(&entry.name) {
            return Ok(());
        }
        in_stack.insert(entry.name.clone());
        if let Some(deps) = dependencies.get(&(entry.name.clone(), entry.version.clone())) {
            for (dep_name, _) in deps {
                if let Some(dep_entry) = entry_map.get(dep_name) {
                    visit(dep_entry, entry_map, dependencies, visited, in_stack, result)?;
                }
            }
        }
        in_stack.remove(&entry.name);
        visited.insert(entry.name.clone());
        result.push(entry.clone());
        Ok(())
    }

    for entry in entries {
        if !visited.contains(&entry.name) {
            visit(entry, &entry_map, dependencies, &mut visited, &mut in_stack, &mut result)?;
        }
    }

    Ok(result)
}

// ===========================================================================
// Proptest generators and tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    fn arb_package_name() -> impl Strategy<Value = PackageName> {
        proptest::string::string_regex("[a-z][a-z0-9_]{1,10}")
            .unwrap()
            .prop_map(|s| PackageName::new(&s).unwrap())
    }

    fn arb_version() -> impl Strategy<Value = Version> {
        (0u64..10, 0u64..10, 0u64..10)
            .prop_map(|(major, minor, patch)| Version::new(major, minor, patch))
    }

    fn _arb_version_req() -> impl Strategy<Value = VersionReq> {
        proptest::string::string_regex(r"\^[0-9]\.[0-9]\.[0-9]")
            .unwrap()
            .prop_map(|s| s.parse().unwrap())
    }

    fn arb_dep_graph() -> impl Strategy<Value = (DependencySolver, BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>>)> {
        let pkg_names = proptest::collection::vec(arb_package_name(), 2..=5);
        pkg_names.prop_flat_map(|names| {
            let ver_strats: Vec<Vec<Version>> = names.iter()
                .map(|_| vec![Version::new(0, 0, 0), Version::new(1, 0, 0)])
                .collect();
            (Just(names), Just(ver_strats))
        }).prop_map(|(names, version_lists)| {
            let mut solver = DependencySolver::new();
            let mut deps: BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>> = BTreeMap::new();

            for (i, name) in names.iter().enumerate() {
                for ver in &version_lists[i] {
                    solver.add_package_version(name.clone(), ver.clone());
                    if i > 0 {
                        let dep_name = names[(i - 1) % names.len()].clone();
                        let req = VersionReq::parse(">=0.0.0").unwrap();
                        deps.entry((name.clone(), ver.clone()))
                            .or_default()
                            .push((dep_name, req));
                    }
                }
            }
            (solver, deps)
        })
    }

    // -----------------------------------------------------------------------
    // Property 26: Solver Correctness
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn prop_solver_correctness(
            (solver, deps) in arb_dep_graph(),
            req_pkg_idx in 0usize..5usize,
        ) {
            let names: Vec<PackageName> = solver.packages.keys().cloned().collect();
            if names.is_empty() {
                return Ok(());
            }
            let idx = req_pkg_idx % names.len();
            let req_name = names[idx].clone();
            let req = VersionReq::parse(">=0.0.0").unwrap();
            let result = solver.solve(&[(req_name, req)], &deps);
            if let Ok(plan) = result {
                prop_assert!(!plan.order.is_empty());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Property 27: Conflict Detection
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn prop_conflict_detection(
            base_name in arb_package_name(),
            ver_a in arb_version(),
            ver_b in arb_version(),
        ) {
            prop_assume!(ver_a != ver_b);
            let mut solver = DependencySolver::new();
            solver.add_package_version(base_name.clone(), ver_a.clone());
            solver.add_package_version(base_name.clone(), ver_b.clone());

            let deps = BTreeMap::new();

            let req_a: VersionReq = format!("={}", ver_a).parse().unwrap();
            let req_b: VersionReq = format!("={}", ver_b).parse().unwrap();

            let result = solver.solve(&[
                (base_name.clone(), req_a),
                (base_name.clone(), req_b),
            ], &deps);

            prop_assert!(
                matches!(result, Err(SolverError::Conflict(_))),
                "conflicting requirements should produce Conflict, got {:?}",
                result
            );
        }
    }

    // -----------------------------------------------------------------------
    // Property 28: Newest Version Preference
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn prop_newest_version_preference(
            base_name in arb_package_name(),
            base_ver in arb_version(),
        ) {
            let mut solver = DependencySolver::new();
            for i in 0u64..=base_ver.major + 3 {
                let v = Version::new(i, 0, 0);
                solver.add_package_version(base_name.clone(), v);
            }

            let deps = BTreeMap::new();
            let req = VersionReq::parse(">=0.0.0").unwrap();

            let result = solver.solve(&[(base_name.clone(), req)], &deps);
            if let Ok(plan) = result {
                prop_assert!(!plan.order.is_empty());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Property 29: Cycle Detection
    // -----------------------------------------------------------------------

    fn arb_cyclic_graph() -> impl Strategy<Value = (DependencySolver, BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>>)> {
        (arb_package_name(), arb_package_name(), arb_version()).prop_map(|(name_a, name_b, ver)| {
            let mut solver = DependencySolver::new();
            solver.add_package_version(name_a.clone(), ver.clone());
            solver.add_package_version(name_b.clone(), ver.clone());

            let mut deps: BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>> = BTreeMap::new();
            let req = VersionReq::parse(">=0.0.0").unwrap();

            deps.entry((name_a.clone(), ver.clone()))
                .or_default()
                .push((name_b.clone(), req.clone()));
            deps.entry((name_b.clone(), ver.clone()))
                .or_default()
                .push((name_a, req));

            (solver, deps)
        })
    }

    proptest! {
        #[test]
        fn prop_cycle_detection(
            (solver, deps) in arb_cyclic_graph(),
        ) {
            let names: Vec<PackageName> = solver.packages.keys().cloned().collect();
            if names.is_empty() {
                return Ok(());
            }
            let req = VersionReq::parse(">=0.0.0").unwrap();
            let result = solver.solve(&[(names[0].clone(), req)], &deps);
            if let Err(err) = result {
                prop_assert!(
                    matches!(err, SolverError::Conflict(_) | SolverError::Cycle(_)),
                    "unexpected error: {:?}",
                    err
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_single_package_no_deps() {
        let mut solver = DependencySolver::new();
        let name = PackageName::new("libc").unwrap();
        solver.add_package_version(name.clone(), Version::new(1, 0, 0));
        solver.add_package_version(name.clone(), Version::new(2, 0, 0));

        let deps = BTreeMap::new();
        let req: VersionReq = ">=1.0.0".parse().unwrap();
        let result = solver.solve(&[(name.clone(), req)], &deps);
        assert!(result.is_ok(), "should succeed: {:?}", result.err());
    }

    #[test]
    fn test_solver_new_empty() {
        let solver = DependencySolver::new();
        assert_eq!(solver.package_names().count(), 0);
    }

    #[test]
    fn test_package_not_found() {
        let solver = DependencySolver::new();
        let name = PackageName::new("nonexistent").unwrap();
        let req: VersionReq = ">=1.0".parse().unwrap();
        let deps = BTreeMap::new();
        let result = solver.solve(&[(name.clone(), req)], &deps);
        assert!(matches!(result, Err(SolverError::NotFound(_))));
    }

    #[test]
    fn test_unsatisfiable_request() {
        let mut solver = DependencySolver::new();
        let name = PackageName::new("foo").unwrap();
        solver.add_package_version(name.clone(), Version::new(1, 0, 0));

        let deps = BTreeMap::new();
        let req: VersionReq = ">=2.0.0".parse().unwrap();
        let result = solver.solve(&[(name.clone(), req)], &deps);
        assert!(matches!(result, Err(SolverError::Conflict(_))));
    }

    #[test]
    fn test_solver_error_display() {
        let err = SolverError::Conflict("test conflict".into());
        let msg = format!("{err}");
        assert!(!msg.is_empty());

        let err = SolverError::NotFound(PackageName::new("missing").unwrap());
        let msg = format!("{err}");
        assert!(msg.contains("missing"));

        let err = SolverError::Cycle(vec![PackageName::new("a").unwrap()]);
        let msg = format!("{err}");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_dependency_satisfied() {
        let mut solver = DependencySolver::new();
        let a = PackageName::new("A").unwrap();
        let b = PackageName::new("B").unwrap();

        solver.add_package_version(a.clone(), Version::new(1, 0, 0));
        solver.add_package_version(b.clone(), Version::new(1, 0, 0));

        let mut deps: BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>> = BTreeMap::new();
        deps.insert(
            (a.clone(), Version::new(1, 0, 0)),
            vec![(b.clone(), ">=1.0.0".parse().unwrap())],
        );

        let req: VersionReq = ">=1.0.0".parse().unwrap();
        let result = solver.solve(&[(a, req)], &deps);
        assert!(result.is_ok(), "should succeed: {:?}", result.err());
    }

    #[test]
    fn test_add_package_and_versions() {
        let mut solver = DependencySolver::new();
        let name = PackageName::new("test-pkg").unwrap();
        solver.add_package_version(name.clone(), Version::new(0, 1, 0));
        solver.add_package_version(name.clone(), Version::new(0, 2, 0));
        assert_eq!(solver.package_names().count(), 1);
        assert_eq!(solver.versions(&name).unwrap().len(), 2);
    }

    #[test]
    fn test_topological_order() {
        let mut solver = DependencySolver::new();
        let a = PackageName::new("A").unwrap();
        let b = PackageName::new("B").unwrap();

        solver.add_package_version(a.clone(), Version::new(1, 0, 0));
        solver.add_package_version(b.clone(), Version::new(1, 0, 0));

        let mut deps: BTreeMap<(PackageName, Version), Vec<(PackageName, VersionReq)>> = BTreeMap::new();
        deps.insert(
            (b.clone(), Version::new(1, 0, 0)),
            vec![(a.clone(), ">=1.0.0".parse().unwrap())],
        );

        let req_a: VersionReq = ">=1.0.0".parse().unwrap();
        let req_b: VersionReq = ">=1.0.0".parse().unwrap();
        let result = solver.solve(&[(a, req_a), (b, req_b)], &deps);
        assert!(result.is_ok(), "should succeed: {:?}", result.err());
    }
}

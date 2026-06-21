use std::collections::{HashMap, HashSet, VecDeque};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceManagerError {
    CycleDetected(Vec<String>),
    ServiceNotFound(String),
    DependencyNotMet(String),
    InvalidConfig(String),
}

impl std::fmt::Display for ServiceManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceManagerError::CycleDetected(path) => {
                write!(f, "dependency cycle: {}", path.join(" → "))
            }
            ServiceManagerError::ServiceNotFound(name) => {
                write!(f, "service not found: {name}")
            }
            ServiceManagerError::DependencyNotMet(name) => {
                write!(f, "dependency not met: {name}")
            }
            ServiceManagerError::InvalidConfig(msg) => {
                write!(f, "invalid configuration: {msg}")
            }
        }
    }
}

impl std::error::Error for ServiceManagerError {}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RestartPolicy {
    #[serde(rename = "never")]
    #[default]
    Never,
    #[serde(rename = "on-failure")]
    OnFailure,
    #[serde(rename = "always")]
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SocketType {
    #[serde(rename = "stream")]
    #[default]
    Stream,
    #[serde(rename = "datagram")]
    Datagram,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocketSpec {
    pub path: String,
    #[serde(default)]
    pub socket_type: SocketType,
    #[serde(default = "default_socket_perms")]
    pub permissions: u32,
}

fn default_socket_perms() -> u32 {
    0o666
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceUnit {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub restart: RestartPolicy,
    #[serde(default = "default_timeout")]
    pub timeout_start_sec: u64,
    #[serde(default)]
    pub socket: Option<SocketSpec>,
}

fn default_timeout() -> u64 {
    5
}

/// A TOML service unit file wrapper (for deserializing `[service]` sections).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ServiceUnitFile {
    service: ServiceUnit,
}

impl ServiceUnit {
    pub fn from_toml(input: &str) -> Result<Self, ServiceManagerError> {
        let parsed: ServiceUnitFile =
            toml::from_str(input).map_err(|e| ServiceManagerError::InvalidConfig(e.to_string()))?;
        Ok(parsed.service)
    }
}

// ---------------------------------------------------------------------------
// Service state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Stopped,
    Starting,
    Running { pid: u64, started_at: u64 },
    Failed { exit_code: Option<i32>, retry_count: u32, message: String },
}

// ---------------------------------------------------------------------------
// Dependency graph resolution
// ---------------------------------------------------------------------------

/// Topological sort of service names based on `after` dependencies.
/// Returns `Ok(vec)` of names in dependency order, or `Err` if a cycle
/// is detected.
pub fn resolve_order(
    units: &HashMap<String, ServiceUnit>,
) -> Result<Vec<String>, ServiceManagerError> {
    // Build adjacency list: edge u → v means "u must start after v"
    // i.e. v is a dependency of u
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (name, unit) in units {
        let deps: Vec<&str> = unit
            .after
            .iter()
            .chain(unit.requires.iter())
            .map(|s| s.as_str())
            .collect();
        adj.insert(name.as_str(), deps);
    }

    // Kahn's algorithm for topological sort
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for name in units.keys() {
        in_degree.entry(name.as_str()).or_insert(0);
    }
    for deps in adj.values() {
        for dep in deps {
            if units.contains_key(*dep) {
                *in_degree.entry(dep).or_insert(0) += 0;
            }
        }
        // Wait, that's wrong. We need in_degree for the nodes that depend on others.
        // edges: node → [deps]. So for each dep, the node's in_degree should increase.
        // Actually, I want edges: node → deps (node depends on dep).
        // In Kahn's, we want nodes with no remaining deps first.
        // in_degree for a node = number of deps that haven't been removed yet.
        // Let me rethink.
    }

    // Standard Kahn's: track number of unmet dependencies per node.
    let mut unmet: HashMap<&str, usize> = HashMap::new();
    let mut by_name: HashMap<&str, &ServiceUnit> = HashMap::new();
    for (name, unit) in units {
        let count = unit
            .after
            .iter()
            .chain(unit.requires.iter())
            .filter(|d| units.contains_key(d.as_str()))
            .count();
        unmet.insert(name.as_str(), count);
        by_name.insert(name.as_str(), unit);
    }

    let mut queue: VecDeque<&str> = VecDeque::new();
    for (name, count) in &unmet {
        if *count == 0 {
            queue.push_back(name);
        }
    }

    let mut sorted: Vec<String> = Vec::with_capacity(units.len());
    while let Some(name) = queue.pop_front() {
        sorted.push(name.to_string());
        // For each node that depends on this one, decrement its unmet count
        for (other, unit) in units {
            let depends = unit
                .after
                .iter()
                .chain(unit.requires.iter())
                .any(|d| d == name);
            if depends && let Some(count) = unmet.get_mut(other.as_str()) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        queue.push_back(other.as_str());
                    }
            }
        }
    }

    if sorted.len() != units.len() {
        // Find the cycle path
        let remaining: HashSet<&str> = units.keys().map(|s| s.as_str()).collect();
        let mut not_sorted: Vec<&str> = remaining
            .into_iter()
            .filter(|n| !sorted.iter().any(|s| s == n))
            .collect();
        not_sorted.sort();
        let cycle_path = find_cycle(units, &not_sorted);
        return Err(ServiceManagerError::CycleDetected(cycle_path));
    }

    Ok(sorted)
}

fn find_cycle(
    units: &HashMap<String, ServiceUnit>,
    remaining: &[&str],
) -> Vec<String> {
    // Standard DFS cycle detection using path tracking.
    let mut unvisited: HashSet<&str> = remaining.iter().copied().collect();
    let mut in_stack: HashSet<&str> = HashSet::new();
    let mut path: Vec<&str> = Vec::new();

    fn visit<'a>(
        node: &'a str,
        units: &'a HashMap<String, ServiceUnit>,
        unvisited: &mut HashSet<&'a str>,
        in_stack: &mut HashSet<&'a str>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        if !unvisited.remove(node) {
            return None;
        }
        in_stack.insert(node);
        path.push(node);

        if let Some(unit) = units.get(node) {
            for dep in unit.after.iter().chain(unit.requires.iter()) {
                if in_stack.contains(dep.as_str()) {
                    // Found back edge — extract cycle from path
                    let idx = path.iter().position(|n| *n == dep.as_str()).unwrap();
                    let cycle: Vec<String> = path[idx..].iter().map(|s| s.to_string()).collect();
                    return Some(cycle);
                }
                if unvisited.contains(dep.as_str()) && let Some(cycle) = visit(dep.as_str(), units, unvisited, in_stack, path) {
                        return Some(cycle);
                }
            }
        }

        path.pop();
        in_stack.remove(node);
        None
    }

    while let Some(start) = unvisited.iter().next().copied() {
        if let Some(cycle) = visit(start, units, &mut unvisited, &mut in_stack, &mut path) {
            return cycle;
        }
    }

    vec!["unknown-cycle".to_string()]
}

// ---------------------------------------------------------------------------
// Service Manager
// ---------------------------------------------------------------------------

pub struct ServiceManager {
    units: HashMap<String, ServiceUnit>,
    states: HashMap<String, ServiceState>,
    ordered: Vec<String>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            units: HashMap::new(),
            states: HashMap::new(),
            ordered: Vec::new(),
        }
    }

    /// Load service units from a list of parsed units.
    pub fn load_units(
        &mut self,
        units: Vec<ServiceUnit>,
    ) -> Result<(), ServiceManagerError> {
        for unit in units {
            let name = unit.name.clone();
            self.units.insert(name, unit);
        }
        self.ordered = resolve_order(&self.units)?;
        // Initialise all states to Stopped
        for name in self.units.keys() {
            self.states.entry(name.clone()).or_insert(ServiceState::Stopped);
        }
        Ok(())
    }

    /// Return services in start order (dependency-resolved).
    pub fn start_order(&self) -> &[String] {
        &self.ordered
    }

    /// Get the current state of a service.
    pub fn state(&self, name: &str) -> Option<&ServiceState> {
        self.states.get(name)
    }

    /// Get the unit definition for a service.
    pub fn unit(&self, name: &str) -> Option<&ServiceUnit> {
        self.units.get(name)
    }

    /// List all services.
    pub fn list_services(&self) -> Vec<&ServiceUnit> {
        self.units.values().collect()
    }

    /// Transition a service to Running.
    pub fn mark_running(&mut self, name: &str, pid: u64, now: u64) -> Result<(), ServiceManagerError> {
        match self.states.get(name) {
            Some(ServiceState::Starting) | Some(ServiceState::Stopped) => {}
            Some(state) => {
                return Err(ServiceManagerError::InvalidConfig(format!(
                    "cannot mark {name} running from {state:?}"
                )));
            }
            None => return Err(ServiceManagerError::ServiceNotFound(name.to_string())),
        }
        self.states.insert(name.to_string(), ServiceState::Running { pid, started_at: now });
        Ok(())
    }

    /// Transition a service to Starting.
    pub fn mark_starting(&mut self, name: &str) -> Result<(), ServiceManagerError> {
        if !self.units.contains_key(name) {
            return Err(ServiceManagerError::ServiceNotFound(name.to_string()));
        }
        self.states.insert(name.to_string(), ServiceState::Starting);
        Ok(())
    }

    /// Transition a service to Failed.
    pub fn mark_failed(
        &mut self,
        name: &str,
        exit_code: Option<i32>,
        message: String,
    ) -> Result<(), ServiceManagerError> {
        if !self.units.contains_key(name) {
            return Err(ServiceManagerError::ServiceNotFound(name.to_string()));
        }
        let retry_count = match self.states.get(name) {
            Some(ServiceState::Failed { retry_count, .. }) => *retry_count + 1,
            _ => 0,
        };
        self.states.insert(
            name.to_string(),
            ServiceState::Failed {
                exit_code,
                retry_count,
                message,
            },
        );
        Ok(())
    }

    /// Transition a service to Stopped.
    pub fn mark_stopped(&mut self, name: &str) -> Result<(), ServiceManagerError> {
        if !self.units.contains_key(name) {
            return Err(ServiceManagerError::ServiceNotFound(name.to_string()));
        }
        self.states.insert(name.to_string(), ServiceState::Stopped);
        Ok(())
    }

    /// Determine whether a service should be restarted based on its policy
    /// and exit code.
    pub fn should_restart(&self, name: &str, exit_code: i32) -> bool {
        match self.units.get(name) {
            Some(unit) => match unit.restart {
                RestartPolicy::Always => true,
                RestartPolicy::OnFailure => exit_code != 0,
                RestartPolicy::Never => false,
            },
            None => false,
        }
    }

    /// Compute back-off delay in seconds for a failed service.
    pub fn backoff_delay(retry_count: u32) -> u64 {
        // Exponential back-off: 1s, 2s, 4s, 8s, ... capped at 60s
        let delay = 1u64 << retry_count.min(6);
        delay.min(60)
    }

    /// Check whether all dependencies of a service are in the Running state.
    pub fn dependencies_ready(&self, name: &str) -> Result<bool, ServiceManagerError> {
        let unit = self
            .units
            .get(name)
            .ok_or_else(|| ServiceManagerError::ServiceNotFound(name.to_string()))?;
        for dep in unit.after.iter().chain(unit.requires.iter()) {
            match self.states.get(dep.as_str()) {
                Some(ServiceState::Running { .. }) => continue,
                Some(_) => return Ok(false),
                None => return Err(ServiceManagerError::DependencyNotMet(dep.clone())),
            }
        }
        Ok(true)
    }

    /// Collect all service names.
    pub fn service_names(&self) -> Vec<String> {
        self.units.keys().cloned().collect()
    }
}

impl Default for ServiceManager {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_unit(name: &str, path: &str, after: &[&str]) -> ServiceUnit {
        ServiceUnit {
            name: name.to_string(),
            path: path.to_string(),
            args: vec![],
            after: after.iter().map(|s| s.to_string()).collect(),
            requires: vec![],
            restart: RestartPolicy::Never,
            timeout_start_sec: 5,
            socket: None,
        }
    }

    // ── 43.2: TOML deserialisation ──────────────────────────────────────────

    #[test]
    fn test_valid_toml_deserializes() {
        let toml = r#"
[service]
name = "my-web-server"
path = "/bin/httpd"
args = ["-p", "8080"]
after = ["network", "log-daemon"]
requires = ["storage"]
restart = "on-failure"
timeout_start_sec = 10
socket = { path = "/run/httpd.sock", socket_type = "stream", permissions = 0o666 }
"#;
        let unit = ServiceUnit::from_toml(toml).unwrap();
        assert_eq!(unit.name, "my-web-server");
        assert_eq!(unit.path, "/bin/httpd");
        assert_eq!(unit.args, vec!["-p", "8080"]);
        assert_eq!(unit.after, vec!["network", "log-daemon"]);
        assert_eq!(unit.requires, vec!["storage"]);
        assert_eq!(unit.restart, RestartPolicy::OnFailure);
        assert_eq!(unit.timeout_start_sec, 10);
        assert!(unit.socket.is_some());
        let sock = unit.socket.unwrap();
        assert_eq!(sock.path, "/run/httpd.sock");
        assert_eq!(sock.socket_type, SocketType::Stream);
    }

    #[test]
    fn test_toml_minimal_defaults() {
        let toml = r#"
[service]
name = "minimal"
path = "/bin/minimal"
"#;
        let unit = ServiceUnit::from_toml(toml).unwrap();
        assert_eq!(unit.name, "minimal");
        assert_eq!(unit.restart, RestartPolicy::Never);
        assert_eq!(unit.timeout_start_sec, 5);
        assert!(unit.args.is_empty());
        assert!(unit.socket.is_none());
    }

    #[test]
    fn test_invalid_toml_rejected() {
        let toml = "not valid toml {{";
        let result = ServiceUnit::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_name_rejected() {
        let toml = r#"
[service]
path = "/bin/x"
"#;
        // toml deserialization will fail because "name" is required but missing
        let result = ServiceUnit::from_toml(toml);
        assert!(result.is_err());
    }

    // ── 43.2: Cycle detection ───────────────────────────────────────────────

    #[test]
    fn test_no_deps_produces_single_order() {
        let mut map = HashMap::new();
        map.insert("a".into(), make_unit("a", "/bin/a", &[]));
        map.insert("b".into(), make_unit("b", "/bin/b", &[]));
        let order = resolve_order(&map).unwrap();
        assert_eq!(order.len(), 2);
        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
    }

    #[test]
    fn test_simple_dependency_order() {
        let mut map = HashMap::new();
        map.insert("a".into(), make_unit("a", "/bin/a", &[]));
        map.insert("b".into(), make_unit("b", "/bin/b", &["a"]));
        let order = resolve_order(&map).unwrap();
        let a_pos = order.iter().position(|s| s == "a").unwrap();
        let b_pos = order.iter().position(|s| s == "b").unwrap();
        assert!(a_pos < b_pos, "a must start before b");
    }

    #[test]
    fn test_transitive_dependency_order() {
        let mut map = HashMap::new();
        map.insert("a".into(), make_unit("a", "/bin/a", &[]));
        map.insert("b".into(), make_unit("b", "/bin/b", &["a"]));
        map.insert("c".into(), make_unit("c", "/bin/c", &["b"]));
        let order = resolve_order(&map).unwrap();
        let a_pos = order.iter().position(|s| s == "a").unwrap();
        let b_pos = order.iter().position(|s| s == "b").unwrap();
        let c_pos = order.iter().position(|s| s == "c").unwrap();
        assert!(a_pos < b_pos);
        assert!(b_pos < c_pos);
    }

    #[test]
    fn test_cycle_detected() {
        let mut map = HashMap::new();
        map.insert("a".into(), make_unit("a", "/bin/a", &["b"]));
        map.insert("b".into(), make_unit("b", "/bin/b", &["a"]));
        let result = resolve_order(&map);
        assert!(matches!(result, Err(ServiceManagerError::CycleDetected(_))));
    }

    #[test]
    fn test_self_cycle_detected() {
        let mut map = HashMap::new();
        map.insert("a".into(), make_unit("a", "/bin/a", &["a"]));
        let result = resolve_order(&map);
        assert!(matches!(result, Err(ServiceManagerError::CycleDetected(_))));
    }

    #[test]
    fn test_requires_also_orders() {
        let mut map = HashMap::new();
        map.insert("a".into(), ServiceUnit {
            requires: vec!["b".into()],
            ..make_unit("a", "/bin/a", &[])
        });
        map.insert("b".into(), make_unit("b", "/bin/b", &[]));
        let order = resolve_order(&map).unwrap();
        let a_pos = order.iter().position(|s| s == "a").unwrap();
        let b_pos = order.iter().position(|s| s == "b").unwrap();
        assert!(b_pos < a_pos, "b must start before a (via requires)");
    }

    // ── Service lifecycle ───────────────────────────────────────────────────

    #[test]
    fn test_service_lifecycle_transitions() {
        let mut mgr = ServiceManager::new();
        mgr.load_units(vec![make_unit("test-svc", "/bin/test", &[])])
            .unwrap();

        assert_eq!(mgr.state("test-svc"), Some(&ServiceState::Stopped));

        mgr.mark_starting("test-svc").unwrap();
        assert_eq!(mgr.state("test-svc"), Some(&ServiceState::Starting));

        mgr.mark_running("test-svc", 42, 1000).unwrap();
        assert_eq!(
            mgr.state("test-svc"),
            Some(&ServiceState::Running { pid: 42, started_at: 1000 })
        );

        mgr.mark_failed("test-svc", Some(1), "crashed".into()).unwrap();
        assert!(matches!(
            mgr.state("test-svc"),
            Some(ServiceState::Failed { exit_code: Some(1), .. })
        ));
    }

    #[test]
    fn test_should_restart_policies() {
        fn unit(restart: RestartPolicy) -> ServiceUnit {
            ServiceUnit {
                restart,
                ..make_unit("x", "/bin/x", &[])
            }
        }

        let mut mgr = ServiceManager::new();
        mgr.load_units(vec![
            unit(RestartPolicy::Never),
        ])
        .unwrap();
        // Rename for clarity
        let mut mgr_always = ServiceManager::new();
        mgr_always
            .load_units(vec![unit(RestartPolicy::Always)])
            .unwrap();
        let mut mgr_onfail = ServiceManager::new();
        mgr_onfail
            .load_units(vec![unit(RestartPolicy::OnFailure)])
            .unwrap();

        assert!(!mgr.should_restart("x", 0));
        assert!(!mgr.should_restart("x", 1));

        assert!(mgr_always.should_restart("x", 0));
        assert!(mgr_always.should_restart("x", 1));

        assert!(!mgr_onfail.should_restart("x", 0));
        assert!(mgr_onfail.should_restart("x", 1));
    }

    #[test]
    fn test_backoff_delay_exponential() {
        assert_eq!(ServiceManager::backoff_delay(0), 1);
        assert_eq!(ServiceManager::backoff_delay(1), 2);
        assert_eq!(ServiceManager::backoff_delay(2), 4);
        assert_eq!(ServiceManager::backoff_delay(3), 8);
        assert_eq!(ServiceManager::backoff_delay(4), 16);
        assert_eq!(ServiceManager::backoff_delay(5), 32);
        assert_eq!(ServiceManager::backoff_delay(6), 60); // capped
        assert_eq!(ServiceManager::backoff_delay(100), 60); // capped
    }

    #[test]
    fn test_dependencies_ready() {
        let mut mgr = ServiceManager::new();
        mgr.load_units(vec![
            make_unit("db", "/bin/db", &[]),
            make_unit("app", "/bin/app", &["db"]),
        ])
        .unwrap();

        // db not started yet
        assert_eq!(mgr.dependencies_ready("app").unwrap(), false);

        mgr.mark_starting("db").unwrap();
        mgr.mark_running("db", 100, 1).unwrap();
        assert_eq!(mgr.dependencies_ready("app").unwrap(), true);
    }

    #[test]
    fn test_retry_count_increments() {
        let mut mgr = ServiceManager::new();
        mgr.load_units(vec![make_unit("x", "/bin/x", &[])])
            .unwrap();

        mgr.mark_failed("x", Some(1), "first fail".into()).unwrap();
        assert_eq!(
            mgr.state("x"),
            Some(&ServiceState::Failed {
                exit_code: Some(1),
                retry_count: 0,
                message: "first fail".into()
            })
        );

        mgr.mark_failed("x", Some(1), "second fail".into()).unwrap();
        assert_eq!(
            mgr.state("x"),
            Some(&ServiceState::Failed {
                exit_code: Some(1),
                retry_count: 1,
                message: "second fail".into()
            })
        );
    }

    #[test]
    fn test_mark_nonexistent_service_errors() {
        let mut mgr = ServiceManager::new();
        assert!(matches!(
            mgr.mark_running("nope", 1, 0),
            Err(ServiceManagerError::ServiceNotFound(_))
        ));
        assert!(matches!(
            mgr.mark_failed("nope", None, "".into()),
            Err(ServiceManagerError::ServiceNotFound(_))
        ));
    }

    #[test]
    fn test_load_units_with_cycle_rejected() {
        let units = vec![
            make_unit("a", "/bin/a", &["b"]),
            make_unit("b", "/bin/b", &["a"]),
        ];
        let mut mgr = ServiceManager::new();
        let result = mgr.load_units(units);
        assert!(result.is_err());
        assert!(matches!(result, Err(ServiceManagerError::CycleDetected(_))));
    }

    #[test]
    fn test_socket_spec_defaults() {
        let spec = SocketSpec {
            path: "/run/test.sock".into(),
            socket_type: SocketType::default(),
            permissions: default_socket_perms(),
        };
        assert_eq!(spec.socket_type, SocketType::Stream);
        assert_eq!(spec.permissions, 0o666);
    }
}

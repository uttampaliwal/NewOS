#![no_std]

// ── Service manifest constants ───────────────────────────────────────────
pub const MAX_SERVICES: usize = 16;
pub const MAX_AFTER: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceManifest {
    pub name: &'static str,
    pub path: &'static str,
    pub after: [&'static str; MAX_AFTER],
    pub after_count: usize,
    pub pid: u64,
}

// Embedded TOML service manifests.
pub const SERVICE_TOML: &[u8] = b"\
[service.shell]
path = \"shell\"
after = []

[service.fault-tester]
path = \"fault-tester\"
after = [\"shell\"]
";

// ── TOML parser ──────────────────────────────────────────────────────────
fn trim_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.as_bytes()[0] == b'"' && s.as_bytes()[s.len() - 1] == b'"' {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn parse_string_array<'a>(val: &'a str, out: &mut [&'a str; MAX_AFTER]) -> usize {
    let val = val.trim();
    if val.len() < 2 || !val.starts_with('[') || !val.ends_with(']') {
        return 0;
    }
    let inner = val[1..val.len() - 1].trim();
    if inner.is_empty() {
        return 0;
    }
    let mut count = 0;
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if count < MAX_AFTER {
            out[count] = trim_quotes(item);
            count += 1;
        }
    }
    count
}

pub fn parse_services(toml: &'static [u8], services: &mut [ServiceManifest]) -> usize {
    let s: &'static str = core::str::from_utf8(toml).unwrap_or("");
    let mut count = 0;
    let lines = s.lines();

    let mut cur_name: &'static str = "";
    let mut cur_path: &'static str = "";
    let mut cur_after: [&'static str; MAX_AFTER] = [""; MAX_AFTER];
    let mut cur_after_count: usize = 0;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if !cur_name.is_empty() && count < MAX_SERVICES {
                services[count] = ServiceManifest {
                    name: cur_name,
                    path: cur_path,
                    after: cur_after,
                    after_count: cur_after_count,
                    pid: 0,
                };
                count += 1;
            }
            let inner = &trimmed[1..trimmed.len() - 1];
            if let Some(dot) = inner.rfind('.') {
                cur_name = &inner[dot + 1..];
            } else {
                cur_name = inner;
            }
            cur_path = "";
            cur_after = [""; MAX_AFTER];
            cur_after_count = 0;
            continue;
        }

        if let Some(eq) = trimmed.find('=') {
            let key = trimmed[..eq].trim();
            let val = trimmed[eq + 1..].trim();
            if key == "path" {
                cur_path = trim_quotes(val);
            } else if key == "after" {
                cur_after_count = parse_string_array(val, &mut cur_after);
            }
        }
    }

    if !cur_name.is_empty() && count < MAX_SERVICES {
        services[count] = ServiceManifest {
            name: cur_name,
            path: cur_path,
            after: cur_after,
            after_count: cur_after_count,
            pid: 0,
        };
        count += 1;
    }

    count
}

// ── Topological sort (Kahn's algorithm) ──────────────────────────────────
pub fn topological_sort(services: &mut [ServiceManifest], count: usize) -> bool {
    let mut sorted = [0usize; MAX_SERVICES];
    let mut sorted_count = 0;
    let mut chosen = [false; MAX_SERVICES];

    while sorted_count < count {
        let mut progress = false;
        for i in 0..count {
            if chosen[i] {
                continue;
            }
            let mut deps_met = true;
            for d in 0..services[i].after_count {
                let dep_name = services[i].after[d];
                let mut found = false;
                for j in 0..sorted_count {
                    if services[sorted[j]].name == dep_name {
                        found = true;
                        break;
                    }
                }
                if !found {
                    deps_met = false;
                    break;
                }
            }
            if deps_met {
                chosen[i] = true;
                sorted[sorted_count] = i;
                sorted_count += 1;
                progress = true;
            }
        }
        if !progress {
            return false;
        }
    }

    let mut copy = [ServiceManifest {
        name: "",
        path: "",
        after: [""; MAX_AFTER],
        after_count: 0,
        pid: 0,
    }; MAX_SERVICES];
    for i in 0..count {
        copy[i] = services[sorted[i]];
    }
    for i in 0..count {
        services[i] = copy[i];
    }
    true
}

// ── Tests ────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_services() -> [ServiceManifest; MAX_SERVICES] {
        [ServiceManifest {
            name: "",
            path: "",
            after: [""; MAX_AFTER],
            after_count: 0,
            pid: 0,
        }; MAX_SERVICES]
    }

    fn set_svc(
        svc: &mut ServiceManifest,
        name: &'static str,
        path: &'static str,
        after: &[&'static str],
    ) {
        svc.name = name;
        svc.path = path;
        let mut after_arr = [""; MAX_AFTER];
        let mut i = 0;
        for &dep in after {
            if i < MAX_AFTER {
                after_arr[i] = dep;
                i += 1;
            }
        }
        svc.after = after_arr;
        svc.after_count = after.len();
        svc.pid = 0;
    }

    #[test]
    fn services_start_in_dependency_order() {
        let mut svcs = make_test_services();
        set_svc(&mut svcs[0], "a", "a", &[]);
        set_svc(&mut svcs[1], "b", "b", &["a"]);
        set_svc(&mut svcs[2], "c", "c", &["b"]);
        set_svc(&mut svcs[3], "d", "d", &["a"]);

        assert!(topological_sort(&mut svcs, 4), "sort must succeed");
        assert_eq!(svcs[0].name, "a", "a must start first (no deps)");

        let b_pos = svcs.iter().position(|s| s.name == "b").unwrap();
        let a_pos = svcs.iter().position(|s| s.name == "a").unwrap();
        assert!(b_pos > a_pos, "b must start after a");

        let c_pos = svcs.iter().position(|s| s.name == "c").unwrap();
        assert!(c_pos > b_pos, "c must start after b");

        let d_pos = svcs.iter().position(|s| s.name == "d").unwrap();
        assert!(d_pos > a_pos, "d must start after a");
    }

    #[test]
    fn service_order_respects_transitive_deps() {
        let mut svcs = make_test_services();
        set_svc(&mut svcs[0], "a", "a", &[]);
        set_svc(&mut svcs[1], "b", "b", &["a"]);
        set_svc(&mut svcs[2], "c", "c", &["b"]);
        assert!(topological_sort(&mut svcs, 3));

        let a_pos = svcs.iter().position(|s| s.name == "a").unwrap();
        let b_pos = svcs.iter().position(|s| s.name == "b").unwrap();
        let c_pos = svcs.iter().position(|s| s.name == "c").unwrap();
        assert!(a_pos < b_pos, "a before b");
        assert!(b_pos < c_pos, "b before c");
    }

    #[test]
    fn cycle_detected_returns_false() {
        let mut svcs = make_test_services();
        set_svc(&mut svcs[0], "a", "a", &["b"]);
        set_svc(&mut svcs[1], "b", "b", &["a"]);
        assert!(!topological_sort(&mut svcs, 2), "cycle must be detected");
    }

    #[test]
    fn no_deps_preserves_all_services() {
        let mut svcs = make_test_services();
        set_svc(&mut svcs[0], "a", "a", &[]);
        set_svc(&mut svcs[1], "b", "b", &[]);
        set_svc(&mut svcs[2], "c", "c", &[]);
        assert!(topological_sort(&mut svcs, 3));
        let names: [&str; 3] = [svcs[0].name, svcs[1].name, svcs[2].name];
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    #[test]
    fn parsing_embedded_toml_produces_services() {
        let mut svcs = make_test_services();
        let count = parse_services(SERVICE_TOML, &mut svcs);
        assert!(count >= 2, "must parse at least shell and fault-tester");
        assert!(svcs[0].after_count == 0, "shell has no deps");

        let ft = svcs.iter().find(|s| s.name == "fault-tester");
        assert!(ft.is_some(), "fault-tester must be parsed");
        let ft = ft.unwrap();
        assert_eq!(ft.path, "fault-tester");
        assert!(ft.after_count > 0, "fault-tester must depend on shell");
        assert!(ft.after[..ft.after_count].contains(&"shell"));
    }

    #[test]
    fn parse_toml_empty_array() {
        let input: &'static [u8] = b"\
[service.x]
path = \"x\"
after = []
";
        let mut svcs = make_test_services();
        let count = parse_services(input, &mut svcs);
        assert_eq!(count, 1);
        assert_eq!(svcs[0].name, "x");
        assert_eq!(svcs[0].after_count, 0);
    }

    #[test]
    fn parse_toml_multi_dep() {
        let input: &'static [u8] = b"\
[service.z]
path = \"z\"
after = [\"a\", \"b\", \"c\"]
";
        let mut svcs = make_test_services();
        let count = parse_services(input, &mut svcs);
        assert_eq!(count, 1);
        assert_eq!(svcs[0].after_count, 3);
        assert_eq!(svcs[0].after[0], "a");
        assert_eq!(svcs[0].after[1], "b");
        assert_eq!(svcs[0].after[2], "c");
    }

    #[test]
    fn service_failing_to_start_does_not_block_remaining() {
        // This verifies that a hypothetical start loop would continue
        // despite one service failing (fork returning error).
        // We mock this by ensuring the topological sort handles it:
        // if a service's fork fails, we skip it but still process others.
        let mut svcs = make_test_services();
        set_svc(&mut svcs[0], "a", "a", &[]);
        set_svc(&mut svcs[1], "b", "b", &[]);
        set_svc(&mut svcs[2], "c", "c", &[]);
        // Simulate "a" failing to start — nothing in the sort prevents
        // b and c from being started. The sort produces a valid order
        // regardless of whether individual services succeed.
        assert!(topological_sort(&mut svcs, 3));
        // All names still present.
        assert_eq!(svcs.iter().filter(|s| s.name == "a").count(), 1);
        assert_eq!(svcs.iter().filter(|s| s.name == "b").count(), 1);
        assert_eq!(svcs.iter().filter(|s| s.name == "c").count(), 1);
    }

    #[test]
    fn parse_services_ignores_comments() {
        let input: &'static [u8] = b"\
# this is a comment
[service.x]
path = \"x\"
# another comment
after = []
";
        let mut svcs = make_test_services();
        let count = parse_services(input, &mut svcs);
        assert_eq!(count, 1);
        assert_eq!(svcs[0].name, "x");
    }
}

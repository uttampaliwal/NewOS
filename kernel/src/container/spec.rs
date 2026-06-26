use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespaceType {
    Pid,
    Network,
    Mount,
    User,
    Ipc,
    Uts,
    Cgroup,
}

#[derive(Debug, Clone)]
pub struct OciLinuxNamespace {
    pub ns_type: NamespaceType,
    pub path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryLimit {
    pub limit: u64,
    pub reservation: u64,
}

#[derive(Debug, Clone)]
pub struct CpuLimit {
    pub shares: u64,
    pub quota: u64,
    pub period: u64,
}

#[derive(Debug, Clone)]
pub struct PidsLimit {
    pub limit: i64,
}

#[derive(Debug, Clone)]
pub struct OciResources {
    pub memory: Option<MemoryLimit>,
    pub cpu: Option<CpuLimit>,
    pub pids: Option<PidsLimit>,
}

#[derive(Debug, Clone)]
pub struct OciLinux {
    pub namespaces: Vec<OciLinuxNamespace>,
    pub resources: OciResources,
    pub masked_paths: Vec<String>,
    pub readonly_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OciUser {
    pub uid: u32,
    pub gid: u32,
    pub additional_gids: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct OciProcess {
    pub terminal: bool,
    pub user: OciUser,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub cwd: String,
}

#[derive(Debug, Clone)]
pub struct OciRoot {
    pub path: String,
    pub readonly: bool,
    /// Content hashes of image layers (e.g., "sha256:abc123...").
    /// Ordered from bottom (base) to top (most recent).
    pub diff_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OciSpec {
    pub oci_version: String,
    pub process: OciProcess,
    pub root: OciRoot,
    pub linux: OciLinux,
    pub hostname: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OciError {
    InvalidSpec(String),
    MissingField(String),
    Unsupported(String),
}

pub fn parse_default_spec(root_path: &str, args: &[&str]) -> OciSpec {
    let args_vec: Vec<String> = args.iter().map(|a| String::from(*a)).collect();

    OciSpec {
        oci_version: String::from("1.0.0"),
        process: OciProcess {
            terminal: false,
            user: OciUser {
                uid: 0,
                gid: 0,
                additional_gids: Vec::new(),
            },
            args: args_vec,
            env: Vec::new(),
            cwd: String::from("/"),
        },
        root: OciRoot {
            path: String::from(root_path),
            readonly: false,
            diff_ids: Vec::new(),
        },
        linux: OciLinux {
            namespaces: vec![
                OciLinuxNamespace {
                    ns_type: NamespaceType::Pid,
                    path: None,
                },
                OciLinuxNamespace {
                    ns_type: NamespaceType::Network,
                    path: None,
                },
                OciLinuxNamespace {
                    ns_type: NamespaceType::Mount,
                    path: None,
                },
                OciLinuxNamespace {
                    ns_type: NamespaceType::User,
                    path: None,
                },
            ],
            resources: OciResources {
                memory: None,
                cpu: None,
                pids: None,
            },
            masked_paths: Vec::new(),
            readonly_paths: Vec::new(),
        },
        hostname: String::from("container"),
    }
}

pub fn validate(spec: &OciSpec) -> Result<(), OciError> {
    if spec.oci_version.is_empty() {
        return Err(OciError::MissingField(
            "oci_version must not be empty".into(),
        ));
    }
    if spec.process.args.is_empty() {
        return Err(OciError::MissingField(
            "process.args must not be empty".into(),
        ));
    }
    if spec.process.cwd.is_empty() {
        return Err(OciError::MissingField(
            "process.cwd must not be empty".into(),
        ));
    }
    if spec.root.path.is_empty() {
        return Err(OciError::MissingField(
            "root.path must not be empty".into(),
        ));
    }
    if let Some(ref mem) = spec.linux.resources.memory {
        if mem.limit == 0 {
            return Err(OciError::InvalidSpec(
                "memory limit must be greater than zero".into(),
            ));
        }
    }
    if let Some(ref cpu) = spec.linux.resources.cpu {
        if cpu.period == 0 {
            return Err(OciError::InvalidSpec(
                "cpu period must be greater than zero".into(),
            ));
        }
    }
    if let Some(ref pids) = spec.linux.resources.pids {
        if pids.limit == 0 {
            return Err(OciError::InvalidSpec(
                "pids limit must be non-zero".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic_spec() -> OciSpec {
        parse_default_spec("/rootfs", &["/bin/sh"])
    }

    #[test]
    fn default_spec_oci_version() {
        let _guard = crate::test_serial::acquire();
        let spec = basic_spec();
        assert_eq!(spec.oci_version, "1.0.0");
    }

    #[test]
    fn default_spec_root_path() {
        let _guard = crate::test_serial::acquire();
        let spec = basic_spec();
        assert_eq!(spec.root.path, "/rootfs");
    }

    #[test]
    fn default_spec_process_args() {
        let _guard = crate::test_serial::acquire();
        let spec = basic_spec();
        assert_eq!(spec.process.args.len(), 1);
        assert_eq!(spec.process.args[0], "/bin/sh");
    }

    #[test]
    fn default_spec_namespaces_present() {
        let _guard = crate::test_serial::acquire();
        let spec = basic_spec();
        assert_eq!(spec.linux.namespaces.len(), 4);
    }

    #[test]
    fn validate_valid_spec() {
        let _guard = crate::test_serial::acquire();
        let spec = basic_spec();
        assert!(validate(&spec).is_ok());
    }

    #[test]
    fn validate_empty_oci_version() {
        let _guard = crate::test_serial::acquire();
        let mut spec = basic_spec();
        spec.oci_version.clear();
        assert_eq!(
            validate(&spec),
            Err(OciError::MissingField(
                "oci_version must not be empty".into()
            ))
        );
    }

    #[test]
    fn validate_empty_args() {
        let _guard = crate::test_serial::acquire();
        let mut spec = basic_spec();
        spec.process.args.clear();
        assert_eq!(
            validate(&spec),
            Err(OciError::MissingField(
                "process.args must not be empty".into()
            ))
        );
    }

    #[test]
    fn validate_empty_cwd() {
        let _guard = crate::test_serial::acquire();
        let mut spec = basic_spec();
        spec.process.cwd.clear();
        assert_eq!(
            validate(&spec),
            Err(OciError::MissingField(
                "process.cwd must not be empty".into()
            ))
        );
    }

    #[test]
    fn validate_empty_root_path() {
        let _guard = crate::test_serial::acquire();
        let mut spec = basic_spec();
        spec.root.path.clear();
        assert_eq!(
            validate(&spec),
            Err(OciError::MissingField(
                "root.path must not be empty".into()
            ))
        );
    }

    #[test]
    fn validate_zero_memory_limit() {
        let _guard = crate::test_serial::acquire();
        let mut spec = basic_spec();
        spec.linux.resources.memory = Some(MemoryLimit {
            limit: 0,
            reservation: 0,
        });
        assert_eq!(
            validate(&spec),
            Err(OciError::InvalidSpec(
                "memory limit must be greater than zero".into()
            ))
        );
    }
}

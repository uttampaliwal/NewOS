use serde::{Deserialize, Serialize};
use semver::Version;
use std::fmt;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    EmptyPackageName,
    InvalidPackageName(String),
    EmptyVersion,
    InvalidVersion(String),
    MissingField(&'static str),
    DuplicateFile(String),
    InvalidScriptName(String),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::EmptyPackageName => write!(f, "package name must not be empty"),
            ValidationError::InvalidPackageName(name) => {
                write!(f, "invalid package name: {name}")
            }
            ValidationError::EmptyVersion => write!(f, "version must not be empty"),
            ValidationError::InvalidVersion(v) => write!(f, "invalid semver version: {v}"),
            ValidationError::MissingField(field) => write!(f, "missing required field: {field}"),
            ValidationError::DuplicateFile(path) => write!(f, "duplicate file entry: {path}"),
            ValidationError::InvalidScriptName(name) => {
                write!(f, "invalid script name: {name}")
            }
        }
    }
}

pub type ValidationResult<T> = Result<T, ValidationError>;

// ---------------------------------------------------------------------------
// PackageName — validated newtype
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PackageName(String);

impl PackageName {
    pub fn new(name: &str) -> ValidationResult<Self> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ValidationError::EmptyPackageName);
        }
        if trimmed.len() > 64 {
            return Err(ValidationError::InvalidPackageName(name.to_string()));
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(ValidationError::InvalidPackageName(name.to_string()));
        }
        if trimmed.starts_with('.') || trimmed.starts_with('-') || trimmed.ends_with('.') || trimmed.ends_with('-') {
            return Err(ValidationError::InvalidPackageName(name.to_string()));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PackageName {
    type Error = ValidationError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

impl From<PackageName> for String {
    fn from(p: PackageName) -> String {
        p.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// DataFile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataFile {
    pub path: String,
    #[serde(default)]
    pub mode: Option<u32>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

// ---------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Scripts {
    #[serde(default)]
    pub pre_install: Option<String>,
    #[serde(default)]
    pub post_install: Option<String>,
    #[serde(default)]
    pub pre_remove: Option<String>,
    #[serde(default)]
    pub post_remove: Option<String>,
}

// ---------------------------------------------------------------------------
// InstallSpec
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSpec {
    #[serde(default)]
    pub files: Vec<DataFile>,
    #[serde(default)]
    pub scripts: Option<Scripts>,
}

// ---------------------------------------------------------------------------
// BuildSpec
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// TpkgManifest — the top-level manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TpkgManifest {
    pub package: PackageManifest,
    #[serde(default)]
    pub install: Option<InstallSpec>,
    #[serde(default)]
    pub build: Option<BuildSpec>,
    #[serde(default)]
    pub scripts: Option<Scripts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    pub name: PackageName,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub name: PackageName,
    pub version_req: String,
}

impl TpkgManifest {
    pub fn parse(toml_str: &str) -> ValidationResult<Self> {
        let manifest: TpkgManifest =
            toml::from_str(toml_str).map_err(|e| ValidationError::InvalidVersion(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn serialize(&self) -> ValidationResult<String> {
        toml::to_string(self).map_err(|e| ValidationError::InvalidVersion(e.to_string()))
    }

    pub fn validate(&self) -> ValidationResult<()> {
        let name = self.package.name.as_str();
        if name.is_empty() {
            return Err(ValidationError::EmptyPackageName);
        }
        // Re-validate package name
        PackageName::new(name).map_err(|_| ValidationError::InvalidPackageName(name.to_string()))?;

        let ver = self.package.version.trim();
        if ver.is_empty() {
            return Err(ValidationError::EmptyVersion);
        }
        Version::parse(ver).map_err(|_| ValidationError::InvalidVersion(ver.to_string()))?;

        // Validate dependency version requirements
        for dep in &self.package.dependencies {
            let dep_ver = dep.version_req.trim();
            if dep_ver.is_empty() {
                return Err(ValidationError::EmptyVersion);
            }
            dep_ver.parse::<semver::VersionReq>().map_err(|_| {
                ValidationError::InvalidVersion(format!("{}/{}", dep.name, dep_ver))
            })?;
        }

        // Check for duplicate file paths
        if let Some(ref install) = self.install {
            let mut seen = std::collections::BTreeSet::new();
            for file in &install.files {
                if !seen.insert(&file.path) {
                    return Err(ValidationError::DuplicateFile(file.path.clone()));
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PackageSource, ResolvedPackage, InstallPlan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageSource {
    Repository { url: String, checksum: String },
    Local { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPackage {
    pub name: PackageName,
    pub version: Version,
    pub manifest: TpkgManifest,
    pub source: PackageSource,
    pub dependencies: Vec<PackageName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub packages: Vec<ResolvedPackage>,
    pub order: Vec<PackageName>,
}

// ===========================================================================
// Proptest generators and property tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    pub fn arb_package_name() -> impl Strategy<Value = PackageName> {
        proptest::string::string_regex("[a-z][a-z0-9_-]{0,30}")
            .unwrap()
            .prop_filter("valid package name", |s| !s.starts_with('-') && !s.ends_with('-'))
            .prop_map(|s| PackageName::new(&s).unwrap())
    }

    pub fn arb_version() -> impl Strategy<Value = String> {
        (0u64..=u64::MAX, 0u64..=999u64, 0u64..=999u64)
            .prop_map(|(major, minor, patch)| format!("{major}.{minor}.{patch}"))
            .prop_filter("valid semver", |s| Version::parse(s).is_ok())
    }

    fn arb_data_file() -> impl Strategy<Value = DataFile> {
        (
            proptest::string::string_regex("(usr|etc|opt|var)/[a-z0-9/._-]{1,40}")
                .unwrap(),
            proptest::option::of(0o644u32..=0o755u32),
        )
            .prop_map(|(path, mode)| DataFile {
                path,
                mode,
                user: None,
                group: None,
            })
    }

    fn arb_scripts() -> impl Strategy<Value = Scripts> {
        (
            proptest::option::of(proptest::string::string_regex("[a-z_]{3,20}").unwrap()),
            proptest::option::of(proptest::string::string_regex("[a-z_]{3,20}").unwrap()),
        )
            .prop_map(|(pre, post)| Scripts {
                pre_install: pre,
                post_install: None,
                pre_remove: None,
                post_remove: post,
            })
    }

    fn arb_package_manifest() -> impl Strategy<Value = PackageManifest> {
        (
            arb_package_name(),
            arb_version(),
            proptest::option::of(proptest::string::string_regex(".{1,80}").unwrap()),
            proptest::option::of(proptest::string::string_regex("[A-Z][a-z]{1,20}").unwrap()),
            proptest::collection::vec(
                (arb_package_name(), arb_version()).prop_map(|(name, version_req)| Dependency {
                    name,
                    version_req,
                }),
                0..=5,
            ),
        )
            .prop_map(|(name, version, desc, license, dependencies)| PackageManifest {
                name,
                version,
                description: desc,
                license,
                authors: vec![],
                dependencies,
            })
    }

    pub fn arb_valid_manifest() -> impl Strategy<Value = TpkgManifest> {
        (
            arb_package_manifest(),
            proptest::option::of(arb_install_spec()),
            proptest::option::of(arb_build_spec()),
            proptest::option::of(arb_scripts()),
        )
            .prop_map(|(package, install, build, scripts)| TpkgManifest {
                package,
                install,
                build,
                scripts,
            })
    }

    fn arb_install_spec() -> impl Strategy<Value = InstallSpec> {
        proptest::collection::vec(arb_data_file(), 0..=8).prop_map(|files| {
            let mut seen = std::collections::HashSet::new();
            let unique: Vec<_> = files
                .into_iter()
                .filter(|f| seen.insert(f.path.clone()))
                .collect();
            InstallSpec {
                files: unique,
                scripts: None,
            }
        })
    }

    fn arb_build_spec() -> impl Strategy<Value = BuildSpec> {
        (
            proptest::string::string_regex("[a-z]{3,15}").unwrap(),
            proptest::collection::vec(proptest::string::string_regex("--[a-z-]{1,20}").unwrap(), 0..=5),
        )
            .prop_map(|(command, args)| BuildSpec {
                command,
                args,
                env: vec![],
            })
    }

    // -----------------------------------------------------------------------
    // Property 24: Manifest Parse-Serialize Round-Trip
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn prop_manifest_parse_serialize_round_trip(manifest in arb_valid_manifest()) {
            let serialized = manifest.serialize().expect("serialization should succeed");
            let parsed = TpkgManifest::parse(&serialized).expect("re-parse should succeed");
            prop_assert_eq!(&manifest.package.name, &parsed.package.name);
            prop_assert_eq!(&manifest.package.version, &parsed.package.version);
            prop_assert_eq!(&manifest.package.description, &parsed.package.description);
            prop_assert_eq!(&manifest.package.license, &parsed.package.license);
            prop_assert_eq!(
                manifest.package.dependencies.len(),
                parsed.package.dependencies.len()
            );
            for (a, b) in manifest.package.dependencies.iter().zip(&parsed.package.dependencies) {
                prop_assert_eq!(&a.name, &b.name);
                prop_assert_eq!(&a.version_req, &b.version_req);
            }
            prop_assert_eq!(manifest.install.is_some(), parsed.install.is_some());
            prop_assert_eq!(manifest.build.is_some(), parsed.build.is_some());
        }
    }

    // -----------------------------------------------------------------------
    // Property 25: Invalid Manifest Rejection
    // -----------------------------------------------------------------------

    fn arb_invalid_toml() -> impl Strategy<Value = String> {
        proptest::string::string_regex("[a-zA-Z0-9_.@#$%^&*(){}\\[\\]|<>?/~` \n\t]{1,200}")
            .unwrap()
            .prop_filter("invalid TOML or missing fields", |s| {
                // Must either be invalid TOML, or if valid TOML, not a valid manifest
                if toml::from_str::<toml::Value>(s).is_err() {
                    return true;
                }
                // Valid TOML but either missing `package` key or package table has issues
                let v: toml::Value = toml::from_str(s).unwrap();
                !v.as_table().unwrap().contains_key("package")
                    || v.as_table().unwrap().get("package").unwrap().as_table().is_none()
            })
    }

    proptest! {
        #[test]
        fn prop_invalid_manifest_rejection(invalid_toml in arb_invalid_toml()) {
            let result = TpkgManifest::parse(&invalid_toml);
            prop_assert!(result.is_err(), "invalid manifest should be rejected");
            let err = result.unwrap_err();
            let msg = format!("{}", err);
            prop_assert!(!msg.is_empty(), "error message must not be empty");
        }
    }

    // -----------------------------------------------------------------------
    // Unit tests for edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_package_name_rejected() {
        assert_eq!(
            PackageName::new(""),
            Err(ValidationError::EmptyPackageName)
        );
    }

    #[test]
    fn test_invalid_package_name_chars() {
        assert!(PackageName::new("hello world").is_err());
        assert!(PackageName::new("hello@world").is_err());
        assert!(PackageName::new("hello/world").is_err());
    }

    #[test]
    fn test_package_name_starts_or_ends_with_dot_dash() {
        assert!(PackageName::new(".hidden").is_err());
        assert!(PackageName::new("-dash").is_err());
        assert!(PackageName::new("dash-").is_err());
        assert!(PackageName::new("dot.").is_err());
    }

    #[test]
    fn test_valid_package_names() {
        assert!(PackageName::new("hello-world").is_ok());
        assert!(PackageName::new("my_package.1").is_ok());
        assert!(PackageName::new("a").is_ok());
        assert!(PackageName::new("rust-core-utils").is_ok());
    }

    #[test]
    fn test_manifest_missing_version() {
        let toml_str = r#"
[package]
name = "test-pkg"
version = ""
"#;
        let result = TpkgManifest::parse(toml_str);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ValidationError::EmptyVersion));
    }

    #[test]
    fn test_manifest_invalid_semver() {
        let toml_str = r#"
[package]
name = "test-pkg"
version = "not.a.version"
"#;
        let result = TpkgManifest::parse(toml_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::InvalidVersion(_)
        ));
    }

    #[test]
    fn test_manifest_duplicate_files() {
        let toml_str = r#"
[package]
name = "test-pkg"
version = "1.0.0"

[install]
files = [
    { path = "/usr/bin/foo" },
    { path = "/usr/bin/foo" },
]
"#;
        let result = TpkgManifest::parse(toml_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::DuplicateFile(_)
        ));
    }

    #[test]
    fn test_manifest_valid_minimal() {
        let toml_str = r#"
[package]
name = "hello-world"
version = "0.1.0"
"#;
        let manifest = TpkgManifest::parse(toml_str).unwrap();
        assert_eq!(manifest.package.name.as_str(), "hello-world");
        assert_eq!(manifest.package.version, "0.1.0");
    }

    #[test]
    fn test_manifest_with_dependencies() {
        let toml_str = r#"
[package]
name = "app"
version = "2.0.0"
dependencies = [
    { name = "libfoo", version_req = ">=1.0" },
    { name = "libbar", version_req = "^0.5" },
]
"#;
        let manifest = TpkgManifest::parse(toml_str).unwrap();
        assert_eq!(manifest.package.dependencies.len(), 2);
        assert_eq!(manifest.package.dependencies[0].name.as_str(), "libfoo");
        assert_eq!(manifest.package.dependencies[1].version_req, "^0.5");
    }

    #[test]
    fn test_serialize_round_trip() {
        let toml_str = r#"
[package]
name = "roundtrip-test"
version = "1.2.3"
description = "A test package"
"#;
        let manifest = TpkgManifest::parse(toml_str).unwrap();
        let serialized = manifest.serialize().unwrap();
        let reparsed = TpkgManifest::parse(&serialized).unwrap();
        assert_eq!(manifest.package.name, reparsed.package.name);
        assert_eq!(manifest.package.version, reparsed.package.version);
        assert_eq!(manifest.package.description, reparsed.package.description);
    }

    #[test]
    fn test_package_source_variants() {
        let repo = PackageSource::Repository {
            url: "https://example.com/pkg.tpkg".into(),
            checksum: "sha256:abc123".into(),
        };
        let local = PackageSource::Local {
            path: "/tmp/pkg.tpkg".into(),
        };
        match repo {
            PackageSource::Repository { ref url, .. } => assert!(url.contains("example.com")),
            _ => panic!("wrong variant"),
        }
        match local {
            PackageSource::Local { ref path } => assert_eq!(path, "/tmp/pkg.tpkg"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_install_plan_empty() {
        let plan = InstallPlan {
            packages: vec![],
            order: vec![],
        };
        assert!(plan.packages.is_empty());
        assert!(plan.order.is_empty());
    }

    #[test]
    fn test_resolved_package_creation() {
        let manifest = TpkgManifest {
            package: PackageManifest {
                name: PackageName::new("test-pkg").unwrap(),
                version: "1.0.0".into(),
                description: None,
                license: None,
                authors: vec![],
                dependencies: vec![],
            },
            install: None,
            build: None,
            scripts: None,
        };
        let resolved = ResolvedPackage {
            name: PackageName::new("test-pkg").unwrap(),
            version: Version::parse("1.0.0").unwrap(),
            manifest,
            source: PackageSource::Local {
                path: "/tmp/test.tpkg".into(),
            },
            dependencies: vec![],
        };
        assert_eq!(resolved.name.as_str(), "test-pkg");
        assert_eq!(resolved.version, Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn test_package_name_display() {
        let name = PackageName::new("my-pkg").unwrap();
        assert_eq!(format!("{name}"), "my-pkg");
        assert_eq!(name.as_str(), "my-pkg");
    }

    #[test]
    fn test_validation_error_display() {
        let err = ValidationError::EmptyPackageName;
        assert!(!format!("{err}").is_empty());
        let err = ValidationError::InvalidVersion("bad".into());
        assert!(!format!("{err}").is_empty());
    }

    #[test]
    fn test_manifest_name_too_long() {
        let long_name = "a".repeat(65);
        assert!(PackageName::new(&long_name).is_err());
    }

    #[test]
    fn test_manifest_name_max_length() {
        let name = "a".repeat(64);
        assert!(PackageName::new(&name).is_ok());
    }

    #[test]
    fn test_dependency_invalid_version_req() {
        let toml_str = r#"
[package]
name = "test"
version = "1.0.0"
dependencies = [
    { name = "libbad", version_req = "!!not-a-valid-req!!" },
]
"#;
        let result = TpkgManifest::parse(toml_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::InvalidVersion(_)
        ));
    }
}

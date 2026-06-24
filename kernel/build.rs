fn main() {
    if std::env::var("SOURCE_DATE_EPOCH").is_err() {
        // SAFETY: This is a build script; only the build process is affected.
        unsafe { std::env::set_var("SOURCE_DATE_EPOCH", "1735689600") };
    }

    println!("cargo:rerun-if-changed=build.rs");
}

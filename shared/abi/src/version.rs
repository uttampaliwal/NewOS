pub const PROJECT_NAME: &str = "turnix";
pub const ABI_VERSION: u32 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_is_expected() {
        assert_eq!(ABI_VERSION, 4);
    }
}

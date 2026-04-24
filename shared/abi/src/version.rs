pub const PROJECT_NAME: &str = "NewOS";
pub const ABI_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::ABI_VERSION;

    #[test]
    fn abi_version_starts_at_one() {
        assert_eq!(ABI_VERSION, 1);
    }
}

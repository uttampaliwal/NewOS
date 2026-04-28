pub const PROJECT_NAME: &str = "NewOS";
pub const ABI_VERSION: u32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_is_current() {
        assert_eq!(ABI_VERSION, 3);
    }
}


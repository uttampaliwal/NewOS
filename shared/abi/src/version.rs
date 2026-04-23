pub const PROJECT_NAME: &str = "NewOS";
pub const ABI_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syscall::Syscall;

    #[test]
    fn abi_version_starts_at_one() {
        assert_eq!(ABI_VERSION, 1);
    }

    #[test]
    fn syscall_round_trip_works() {
        assert_eq!(Syscall::from_u16(Syscall::Write as u16), Some(Syscall::Write));
    }
}


#![cfg_attr(feature = "host_bin", no_std)]

use sha2::{Digest, Sha256};

pub const PASSWD_PATH: &str = "/etc/turnix/passwd";

#[derive(Debug, Clone, PartialEq)]
pub struct PasswdEntry<'a> {
    pub username: &'a str,
    pub uid: u32,
    pub gid: u32,
    pub home: &'a str,
    pub shell: &'a str,
    pub password_hash: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    MissingField,
    InvalidUid,
    InvalidGid,
    InvalidFormat,
}

pub fn parse_passwd_entry<'a>(line: &'a str) -> Result<PasswdEntry<'a>, ParseError> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Err(ParseError::InvalidFormat);
    }
    let mut parts = ["", "", "", "", "", ""];
    for (i, p) in trimmed.splitn(6, ':').enumerate() {
        if i < 6 {
            parts[i] = p;
        }
    }
    if parts[5].is_empty() {
        return Err(ParseError::MissingField);
    }
    let uid: u32 = parts[1].parse().map_err(|_| ParseError::InvalidUid)?;
    let gid: u32 = parts[2].parse().map_err(|_| ParseError::InvalidGid)?;
    Ok(PasswdEntry {
        username: parts[0],
        uid,
        gid,
        home: parts[3],
        shell: parts[4],
        password_hash: parts[5],
    })
}

pub fn verify_password(password: &str, stored_hash_hex: &str) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    let result = hasher.finalize();
    let mut hex_buf = [0u8; 64];
    hex_encode(&result, &mut hex_buf);
    let computed = core::str::from_utf8(&hex_buf).unwrap_or("");
    computed == stored_hash_hex
}

fn hex_encode(bytes: &[u8], out: &mut [u8]) {
    for (i, b) in bytes.iter().enumerate() {
        let hi = b >> 4;
        let lo = b & 0x0f;
        out[i * 2] = hex_nibble(hi);
        out[i * 2 + 1] = hex_nibble(lo);
    }
}

fn hex_nibble(n: u8) -> u8 {
    match n {
        0..=9 => b'0' + n,
        _ => b'a' + n - 10,
    }
}

pub fn authenticate<'a>(
    username: &str,
    password: &str,
    passwd_content: &'a str,
) -> Option<PasswdEntry<'a>> {
    for line in passwd_content.lines() {
        if let Ok(entry) = parse_passwd_entry(line)
            && entry.username == username
            && verify_password(password, entry.password_hash)
        {
            return Some(entry);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_entry() {
        let line = "uttam:1000:1000:/home/uttam:/bin/sh:a665a45920422f9d417e4867efdc4fb8a04a1f3fff1fa07e998e86f7f7a27ae3";
        let entry = parse_passwd_entry(line).unwrap();
        assert_eq!(entry.username, "uttam");
        assert_eq!(entry.uid, 1000);
        assert_eq!(entry.gid, 1000);
        assert_eq!(entry.home, "/home/uttam");
        assert_eq!(entry.shell, "/bin/sh");
        assert_eq!(
            entry.password_hash,
            "a665a45920422f9d417e4867efdc4fb8a04a1f3fff1fa07e998e86f7f7a27ae3"
        );
    }

    #[test]
    fn test_parse_empty_line() {
        assert_eq!(parse_passwd_entry(""), Err(ParseError::InvalidFormat));
    }

    #[test]
    fn test_parse_comment_line() {
        assert_eq!(
            parse_passwd_entry("# this is a comment"),
            Err(ParseError::InvalidFormat)
        );
    }

    #[test]
    fn test_parse_too_few_fields() {
        assert_eq!(
            parse_passwd_entry("user:1000:1000:/home/user"),
            Err(ParseError::MissingField)
        );
    }

    #[test]
    fn test_parse_invalid_uid() {
        assert_eq!(
            parse_passwd_entry("user:abc:1000:/home/user:/bin/sh:hash"),
            Err(ParseError::InvalidUid)
        );
    }

    #[test]
    fn test_verify_correct_password() {
        // SHA-256("hello123") = 27cc6994fc1c01ce6659c6bddca9b69c4c6a9418065e612c69d110b3f7b11f8a
        let hash = "27cc6994fc1c01ce6659c6bddca9b69c4c6a9418065e612c69d110b3f7b11f8a";
        assert!(verify_password("hello123", hash));
    }

    #[test]
    fn test_verify_incorrect_password() {
        // "hello123" correct hash, but we pass "wrongpassword"
        let hash = "27cc6994fc1c01ce6659c6bddca9b69c4c6a9418065e612c69d110b3f7b11f8a";
        assert!(!verify_password("wrongpassword", hash));
    }

    #[test]
    fn test_authenticate_success() {
        let passwd = "root:0:0:/root:/bin/sh:5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8\nuttam:1000:1000:/home/uttam:/bin/sh:a665a45920422f9d417e4867efdc4fb8a04a1f3fff1fa07e998e86f7f7a27ae3\n";
        let result = authenticate("root", "password", passwd);
        assert!(result.is_some());
        let entry = result.unwrap();
        assert_eq!(entry.uid, 0);
        assert_eq!(entry.gid, 0);
    }

    #[test]
    fn test_authenticate_wrong_password() {
        let passwd = "root:0:0:/root:/bin/sh:5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8\n";
        let result = authenticate("root", "wrongpass", passwd);
        assert!(result.is_none());
    }

    #[test]
    fn test_authenticate_unknown_user() {
        let passwd = "root:0:0:/root:/bin/sh:5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8\n";
        let result = authenticate("nobody", "password", passwd);
        assert!(result.is_none());
    }

    #[test]
    fn test_hex_encode() {
        let bytes = [0xab, 0xcd, 0xef];
        let mut out = [0u8; 64];
        hex_encode(&bytes, &mut out);
        assert_eq!(core::str::from_utf8(&out[..6]).unwrap(), "abcdef");
    }
}

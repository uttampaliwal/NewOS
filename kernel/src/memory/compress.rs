//! Page compression for zswap/zram.
//!
//! Uses a simple run-length + back-reference encoding optimized for kernel
//! memory pages. The format stores a header followed by encoded payload:
//!
//! ```text
//! [4 bytes: uncompressed_len (LE)] [4 bytes: compressed_len (LE)] [payload...]
//! ```
//!
//! Payload is a sequence of tokens:
//! - `0x00 byte`: literal byte follows
//! - `0x01 len_lo len_hi offset_lo offset_hi`: back-reference
//!   (length = len + 5, offset = 1-based distance back)

use alloc::vec::Vec;

/// Maximum uncompressed block size (4 KiB page).
pub const MAX_BLOCK_SIZE: usize = 4096;

/// Header size.
pub const HEADER_SIZE: usize = 8;

/// Compress a block of data. Returns compressed bytes including the 8-byte header.
pub fn compress(input: &[u8]) -> Option<Vec<u8>> {
    if input.is_empty() || input.len() > MAX_BLOCK_SIZE {
        return None;
    }

    let mut output: Vec<u8> = Vec::with_capacity(input.len() / 2 + 64);
    output.extend_from_slice(&[0u8; HEADER_SIZE]);

    let len = input.len();
    let mut src: usize = 0;

    while src < len {
        let (match_len, match_off) = find_match(input, src);

        if match_len >= 5 {
            // Back-reference token
            output.push(0x01);
            let ml = (match_len - 5).min(65535);
            output.push((ml & 0xFF) as u8);
            output.push(((ml >> 8) & 0xFF) as u8);
            output.push((match_off & 0xFF) as u8);
            output.push(((match_off >> 8) & 0xFF) as u8);
            src += match_len;
        } else {
            // Collect a run of literal bytes
            let mut lits: Vec<u8> = Vec::new();
            loop {
                if src >= len || lits.len() >= 65535 {
                    break;
                }
                let (ml, _) = find_match(input, src);
                if ml >= 5 {
                    break;
                }
                lits.push(input[src]);
                src += 1;
            }

            // Emit in chunks of up to 255 literals per token
            for chunk in lits.chunks(255) {
                output.push(0x00);
                output.push(chunk.len() as u8);
                output.extend_from_slice(chunk);
            }
        }
    }

    let compressed_len = output.len();
    output[0..4].copy_from_slice(&(input.len() as u32).to_le_bytes());
    output[4..8].copy_from_slice(&(compressed_len as u32).to_le_bytes());

    Some(output)
}

/// Decompress data produced by `compress`.
pub fn decompress(input: &[u8]) -> Option<Vec<u8>> {
    if input.len() < HEADER_SIZE {
        return None;
    }

    let uncompressed_len = (input[0] as usize)
        | ((input[1] as usize) << 8)
        | ((input[2] as usize) << 16)
        | ((input[3] as usize) << 24);
    let compressed_len = (input[4] as usize)
        | ((input[5] as usize) << 8)
        | ((input[6] as usize) << 16)
        | ((input[7] as usize) << 24);

    if compressed_len > input.len() || uncompressed_len > MAX_BLOCK_SIZE {
        return None;
    }

    let mut output: Vec<u8> = Vec::with_capacity(uncompressed_len);
    let mut pos = HEADER_SIZE;
    let end = compressed_len;

    while pos < end && output.len() < uncompressed_len {
        let tag = input[pos];
        pos += 1;

        match tag {
            0x00 => {
                // Literal run
                if pos >= end {
                    return None;
                }
                let count = input[pos] as usize;
                pos += 1;
                if pos + count > end {
                    return None;
                }
                output.extend_from_slice(&input[pos..pos + count]);
                pos += count;
            }
            0x01 => {
                // Back-reference
                if pos + 4 > end {
                    return None;
                }
                let ml_lo = input[pos] as usize;
                let ml_hi = input[pos + 1] as usize;
                let off_lo = input[pos + 2] as usize;
                let off_hi = input[pos + 3] as usize;
                pos += 4;

                let match_len = ml_lo | (ml_hi << 8);
                let match_len = match_len + 5;
                let offset = off_lo | (off_hi << 8);

                if offset == 0 || offset > output.len() {
                    return None;
                }

                let start = output.len() - offset;
                for i in 0..match_len {
                    let idx = start + (i % offset);
                    let b = output[idx];
                    output.push(b);
                }
            }
            _ => {
                return None;
            }
        }
    }

    if output.len() != uncompressed_len {
        return None;
    }

    Some(output)
}

/// Find the longest match starting at `pos`. Returns `(length, offset)`.
/// Length >= 5 to be useful. Offset is 1-based distance back.
fn find_match(input: &[u8], pos: usize) -> (usize, usize) {
    if pos == 0 {
        return (0, 0);
    }

    let mut best_len = 0usize;
    let mut best_off = 0usize;

    let search_start = pos.saturating_sub(4096);

    for cand in search_start..pos {
        let mut ml = 0usize;
        while pos + ml < input.len()
            && ml < 65540
            && input[cand + ml] == input[pos + ml]
        {
            ml += 1;
        }
        if ml >= 5 && ml > best_len {
            best_len = ml;
            best_off = pos - cand;
        }
    }

    (best_len, best_off)
}

/// Calculate compression ratio: (compressed_payload / original) * 100.
pub fn compression_ratio(compressed: &[u8], original_size: usize) -> u32 {
    if original_size == 0 || compressed.len() < HEADER_SIZE {
        return 100;
    }
    let payload_len = (compressed[4] as u32)
        | ((compressed[5] as u32) << 8)
        | ((compressed[6] as u32) << 16)
        | ((compressed[7] as u32) << 24);
    if payload_len == 0 {
        return 0;
    }
    (payload_len * 100) / (original_size as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn round_trip_random_data() {
        let _s = crate::test_serial::acquire();
        let data: Vec<u8> = (0..4096).map(|i| (i * 7 + 13) as u8).collect();
        let c = compress(&data).expect("compress ok");
        let d = decompress(&c).expect("decompress ok");
        assert_eq!(data, d);
    }

    #[test]
    fn round_trip_all_zeros() {
        let _s = crate::test_serial::acquire();
        let data = [0u8; 4096];
        let c = compress(&data).expect("compress ok");
        let d = decompress(&c).expect("decompress ok");
        assert_eq!(&data[..], &d[..]);
    }

    #[test]
    fn round_trip_all_same_byte() {
        let _s = crate::test_serial::acquire();
        let data = [0xABu8; 4096];
        let c = compress(&data).expect("compress ok");
        let d = decompress(&c).expect("decompress ok");
        assert_eq!(&data[..], &d[..]);
    }

    #[test]
    fn round_trip_sequential() {
        let _s = crate::test_serial::acquire();
        let data: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
        let c = compress(&data).expect("compress ok");
        let d = decompress(&c).expect("decompress ok");
        assert_eq!(data, d);
    }

    #[test]
    fn round_trip_repeating_pattern() {
        let _s = crate::test_serial::acquire();
        let pat = [0xDE, 0xAD, 0xBE, 0xEF];
        let data: Vec<u8> = pat.iter().copied().cycle().take(4096).collect();
        let c = compress(&data).expect("compress ok");
        let d = decompress(&c).expect("decompress ok");
        assert_eq!(data, d);
    }

    #[test]
    fn round_trip_small() {
        let _s = crate::test_serial::acquire();
        let data = [0x42u8; 16];
        let c = compress(&data).expect("compress ok");
        let d = decompress(&c).expect("decompress ok");
        assert_eq!(&data[..], &d[..]);
    }

    #[test]
    fn round_trip_single_byte() {
        let _s = crate::test_serial::acquire();
        let data = [0xFFu8; 1];
        let c = compress(&data).expect("compress ok");
        let d = decompress(&c).expect("decompress ok");
        assert_eq!(&data[..], &d[..]);
    }

    #[test]
    fn round_trip_exact_4096() {
        let _s = crate::test_serial::acquire();
        let mut data = vec![0u8; 4096];
        for i in 0..18 {
            data[i] = (i as u8) + 1;
        }
        for i in 18..4096 {
            data[i] = data[i % 18];
        }
        let c = compress(&data).expect("compress ok");
        let d = decompress(&c).expect("decompress ok");
        assert_eq!(data, d);
    }

    #[test]
    fn empty_rejected() {
        let _s = crate::test_serial::acquire();
        assert!(compress(&[]).is_none());
    }

    #[test]
    fn oversized_rejected() {
        let _s = crate::test_serial::acquire();
        assert!(compress(&[0u8; 4097]).is_none());
    }

    #[test]
    fn decompress_truncated_header() {
        let _s = crate::test_serial::acquire();
        assert!(decompress(&[0u8; 4]).is_none());
    }

    #[test]
    fn decompress_bad_lengths() {
        let _s = crate::test_serial::acquire();
        let mut h = [0u8; 16];
        h[0..4].copy_from_slice(&100u32.to_le_bytes());
        h[4..8].copy_from_slice(&999u32.to_le_bytes());
        assert!(decompress(&h).is_none());
    }

    #[test]
    fn compression_ratio_works() {
        let _s = crate::test_serial::acquire();
        let data = [0xAAu8; 4096];
        let c = compress(&data).unwrap();
        let r = compression_ratio(&c, 4096);
        assert!(r < 50, "should compress well, got {}%", r);
    }

    #[test]
    fn find_match_finds_repetition() {
        let _s = crate::test_serial::acquire();
        let mut data = vec![0u8; 100];
        data[10..14].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        data[50..54].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let (len, off) = find_match(&data, 50);
        assert!(len >= 4);
        assert_eq!(off, 40);
    }

    #[test]
    fn find_match_no_match_at_start() {
        let _s = crate::test_serial::acquire();
        let data: Vec<u8> = (0..100).map(|i| i as u8).collect();
        let (len, off) = find_match(&data, 0);
        assert_eq!(len, 0);
        assert_eq!(off, 0);
    }
}

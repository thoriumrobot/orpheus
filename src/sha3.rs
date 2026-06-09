//! Self-contained SHA3-256 (FIPS 202). No dependencies — the hash function that
//! produces content-addresses (CIDs) for knots, modules, and network events is
//! part of the trusted base, so we keep it in-tree and verifiable.
//!
//! Keccak-f[1600], rate = 1088 bits (136 bytes), capacity 512, SHA-3 domain pad 0x06.

const ROUNDS: usize = 24;

const RC: [u64; 24] = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808a, 0x8000000080008000,
    0x000000000000808b, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008a, 0x0000000000000088, 0x0000000080008009, 0x000000008000000a,
    0x000000008000808b, 0x800000000000008b, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800a, 0x800000008000000a,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
];

const ROT: [[u32; 5]; 5] = [
    [0, 36, 3, 41, 18],
    [1, 44, 10, 45, 2],
    [62, 6, 43, 15, 61],
    [28, 55, 25, 21, 56],
    [27, 20, 39, 8, 14],
];

fn keccak_f(st: &mut [u64; 25]) {
    for round in 0..ROUNDS {
        // theta
        let mut c = [0u64; 5];
        for x in 0..5 {
            c[x] = st[x] ^ st[x + 5] ^ st[x + 10] ^ st[x + 15] ^ st[x + 20];
        }
        let mut d = [0u64; 5];
        for x in 0..5 {
            d[x] = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
        }
        for x in 0..5 {
            for y in 0..5 {
                st[x + 5 * y] ^= d[x];
            }
        }
        // rho + pi
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                let nx = y;
                let ny = (2 * x + 3 * y) % 5;
                b[nx + 5 * ny] = st[x + 5 * y].rotate_left(ROT[x][y]);
            }
        }
        // chi
        for x in 0..5 {
            for y in 0..5 {
                st[x + 5 * y] =
                    b[x + 5 * y] ^ ((!b[(x + 1) % 5 + 5 * y]) & b[(x + 2) % 5 + 5 * y]);
            }
        }
        // iota
        st[0] ^= RC[round];
    }
}

/// SHA3-256 digest of `input`.
pub fn sha3_256(input: &[u8]) -> [u8; 32] {
    const RATE: usize = 136; // bytes
    let mut st = [0u64; 25];

    // absorb
    let mut offset = 0;
    while input.len() - offset >= RATE {
        absorb_block(&mut st, &input[offset..offset + RATE]);
        keccak_f(&mut st);
        offset += RATE;
    }
    // pad: 0x06 ... 0x80 within the final RATE-sized block
    let mut block = [0u8; RATE];
    let rem = &input[offset..];
    block[..rem.len()].copy_from_slice(rem);
    block[rem.len()] ^= 0x06;
    block[RATE - 1] ^= 0x80;
    absorb_block(&mut st, &block);
    keccak_f(&mut st);

    // squeeze 32 bytes (fits in first 4 lanes)
    let mut out = [0u8; 32];
    for i in 0..4 {
        out[i * 8..i * 8 + 8].copy_from_slice(&st[i].to_le_bytes());
    }
    out
}

fn absorb_block(st: &mut [u64; 25], block: &[u8]) {
    // RATE (136) = 17 lanes of 8 bytes
    for i in 0..17 {
        let mut lane = 0u64;
        for j in 0..8 {
            lane |= (block[i * 8 + j] as u64) << (8 * j);
        }
        st[i] ^= lane;
    }
}

/// Hex string of a 32-byte digest.
pub fn hex(d: &[u8]) -> String {
    let mut s = String::with_capacity(d.len() * 2);
    for b in d {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kat_empty() {
        // Known answer: SHA3-256("") = a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a
        let d = sha3_256(b"");
        assert_eq!(
            hex(&d),
            "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
        );
    }

    #[test]
    fn kat_abc() {
        // SHA3-256("abc") = 3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532
        let d = sha3_256(b"abc");
        assert_eq!(
            hex(&d),
            "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
        );
    }
}

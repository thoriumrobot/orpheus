//! Knot: the sole datatype of Loom. A knot is either an atom or an ordered pair
//! (cell) of knots. Every value in the system — code, data, types, UI, network
//! messages — is a knot: an acyclic binary tree of natural numbers.
//!
//! Structural sharing via `Arc` (knots cross threads in the distributed runtime). Pointer identity is never observable; equality is
//! purely structural, which is what makes content-addressing sound.

use crate::atom::Atom;
use crate::sha3;
use std::sync::Arc;

pub type N = Arc<Knot>;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Knot {
    Atom(Atom),
    Cell(N, N),
}

pub fn atom(a: Atom) -> N {
    Arc::new(Knot::Atom(a))
}
pub fn num(n: u128) -> N {
    Arc::new(Knot::Atom(Atom::from_u128(n)))
}
pub fn cord(s: &str) -> N {
    Arc::new(Knot::Atom(Atom::from_cord(s)))
}
pub fn cell(h: N, t: N) -> N {
    Arc::new(Knot::Cell(h, t))
}
/// Right-nested tuple: list![a,b,c] = [a [b c]].
#[macro_export]
macro_rules! knot_tuple {
    ($a:expr) => { $a };
    ($a:expr, $($rest:expr),+) => { $crate::knot::cell($a, knot_tuple!($($rest),+)) };
}

impl Knot {
    pub fn is_atom(&self) -> bool {
        matches!(self, Knot::Atom(_))
    }
    pub fn as_atom(&self) -> Option<&Atom> {
        match self {
            Knot::Atom(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_cell(&self) -> Option<(&N, &N)> {
        match self {
            Knot::Cell(h, t) => Some((h, t)),
            _ => None,
        }
    }

    // ---- deterministic serialization (our "jam") ----------------------------
    // atom: 0x00, uleb128(len), len bytes (LE)
    // cell: 0x01, jam(head), jam(tail)
    pub fn jam(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.jam_into(&mut out);
        out
    }
    fn jam_into(&self, out: &mut Vec<u8>) {
        match self {
            Knot::Atom(a) => {
                out.push(0x00);
                let b = a.bytes_le();
                write_uleb(out, b.len() as u64);
                out.extend_from_slice(b);
            }
            Knot::Cell(h, t) => {
                out.push(0x01);
                h.jam_into(out);
                t.jam_into(out);
            }
        }
    }

    /// Deserialize ("cue"). Returns the knot and bytes consumed.
    pub fn cue(bytes: &[u8]) -> Option<(N, usize)> {
        if bytes.is_empty() {
            return None;
        }
        match bytes[0] {
            0x00 => {
                let (len, n) = read_uleb(&bytes[1..])?;
                let start = 1 + n;
                let end = start + len as usize;
                if end > bytes.len() {
                    return None;
                }
                let a = Atom::from_bytes_le(bytes[start..end].to_vec());
                Some((atom(a), end))
            }
            0x01 => {
                let (h, hn) = Knot::cue(&bytes[1..])?;
                let (t, tn) = Knot::cue(&bytes[1 + hn..])?;
                Some((cell(h, t), 1 + hn + tn))
            }
            _ => None,
        }
    }

    /// Content address: SHA3-256 of the canonical serialization.
    pub fn cid(&self) -> [u8; 32] {
        sha3::sha3_256(&self.jam())
    }
    pub fn cid_hex(&self) -> String {
        sha3::hex(&self.cid())
    }
}

fn write_uleb(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

fn read_uleb(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0;
    for (i, b) in bytes.iter().enumerate() {
        result |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some((result, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

// ---- pretty printing --------------------------------------------------------
impl std::fmt::Debug for Knot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Knot::Atom(a) => {
                if let Some(c) = a.as_cord() {
                    write!(f, "%{}", c)
                } else {
                    write!(f, "{:?}", a)
                }
            }
            Knot::Cell(h, t) => write!(f, "[{:?} {:?}]", h, t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jam_cue_roundtrip() {
        let k = cell(num(42), cell(cord("incr"), num(0)));
        let bytes = k.jam();
        let (k2, used) = Knot::cue(&bytes).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(k, k2);
    }

    #[test]
    fn cid_is_stable_and_structural() {
        let a = cell(num(1), num(2));
        let b = cell(num(1), num(2));
        assert_eq!(a.cid(), b.cid()); // structural equality => same content address
        let c = cell(num(1), num(3));
        assert_ne!(a.cid(), c.cid());
    }
}

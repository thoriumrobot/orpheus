//! Atom: an arbitrary-precision natural number, the only scalar in the Loom data model.
//!
//! Stored little-endian (least-significant byte first), always normalized so the
//! most-significant byte is nonzero. The number zero is the empty byte vector.
//! Text "cords" (UTF-8) are stored byte-for-byte; under the LE numeric reading this
//! is exactly Urbit's `@t`/`@tas` convention, so a tag like `%incr` is just the
//! number whose LE bytes spell "incr".

use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(Vec<u8>); // little-endian, normalized (no trailing zero bytes)

impl Atom {
    #[allow(dead_code)]
    pub fn zero() -> Self {
        Atom(Vec::new())
    }

    pub fn from_bytes_le(mut v: Vec<u8>) -> Self {
        while let Some(&0) = v.last() {
            v.pop();
        }
        Atom(v)
    }

    pub fn from_u128(mut n: u128) -> Self {
        let mut v = Vec::new();
        while n > 0 {
            v.push((n & 0xff) as u8);
            n >>= 8;
        }
        Atom(v)
    }

    /// A "cord": store the UTF-8 bytes directly (LE numeric reading = @tas).
    pub fn from_cord(s: &str) -> Self {
        Atom::from_bytes_le(s.as_bytes().to_vec())
    }

    #[allow(dead_code)]
    pub fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    pub fn bytes_le(&self) -> &[u8] {
        &self.0
    }

    /// Arbitrary-precision addition. Loom's core only has `inc`; this is a *host
    /// primitive* used by jets to accelerate what the language computes by looping.
    pub fn add(&self, other: &Atom) -> Atom {
        let (a, b) = (&self.0, &other.0);
        let n = a.len().max(b.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u16;
        for i in 0..n {
            let x = *a.get(i).unwrap_or(&0) as u16;
            let y = *b.get(i).unwrap_or(&0) as u16;
            let s = x + y + carry;
            out.push((s & 0xff) as u8);
            carry = s >> 8;
        }
        if carry > 0 {
            out.push(carry as u8);
        }
        Atom::from_bytes_le(out)
    }

    /// Compare magnitudes (arbitrary precision).
    pub fn cmp_big(&self, other: &Atom) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match self.0.len().cmp(&other.0.len()) {
            Ordering::Equal => {
                for i in (0..self.0.len()).rev() {
                    match self.0[i].cmp(&other.0[i]) {
                        Ordering::Equal => continue,
                        ord => return ord,
                    }
                }
                Ordering::Equal
            }
            ord => ord,
        }
    }

    /// Arbitrary-precision subtraction; None on underflow (naturals only).
    pub fn sub_big(&self, other: &Atom) -> Option<Atom> {
        if self.cmp_big(other) == std::cmp::Ordering::Less {
            return None;
        }
        let mut out = Vec::with_capacity(self.0.len());
        let mut borrow = 0i16;
        for i in 0..self.0.len() {
            let x = self.0[i] as i16;
            let y = *other.0.get(i).unwrap_or(&0) as i16;
            let mut d = x - y - borrow;
            if d < 0 {
                d += 256;
                borrow = 1;
            } else {
                borrow = 0;
            }
            out.push(d as u8);
        }
        Some(Atom::from_bytes_le(out))
    }

    /// Arbitrary-precision multiplication (schoolbook; operands are cords and
    /// counters, kilobytes at most).
    pub fn mul_big(&self, other: &Atom) -> Atom {
        if self.0.is_empty() || other.0.is_empty() {
            return Atom::from_u128(0);
        }
        let mut out = vec![0u16; self.0.len() + other.0.len()];
        for (i, &x) in self.0.iter().enumerate() {
            let mut carry = 0u32;
            for (j, &y) in other.0.iter().enumerate() {
                let t = out[i + j] as u32 + (x as u32) * (y as u32) + carry;
                out[i + j] = (t & 0xff) as u16;
                carry = t >> 8;
            }
            let mut k = i + other.0.len();
            while carry > 0 {
                let t = out[k] as u32 + carry;
                out[k] = (t & 0xff) as u16;
                carry = t >> 8;
                k += 1;
            }
        }
        Atom::from_bytes_le(out.into_iter().map(|b| b as u8).collect())
    }

    /// Arbitrary-precision division with remainder (binary long division);
    /// None when dividing by zero.
    pub fn divmod_big(&self, other: &Atom) -> Option<(Atom, Atom)> {
        if other.0.is_empty() {
            return None;
        }
        if self.cmp_big(other) == std::cmp::Ordering::Less {
            return Some((Atom::from_u128(0), self.clone()));
        }
        let bits = self.0.len() * 8;
        let mut q = vec![0u8; self.0.len()];
        let mut rem = Atom::from_u128(0);
        for k in (0..bits).rev() {
            // rem = rem*2 + bit_k(self)
            rem = rem.mul_big(&Atom::from_u128(2));
            if (self.0[k / 8] >> (k % 8)) & 1 == 1 {
                rem = rem.add(&Atom::from_u128(1));
            }
            if rem.cmp_big(other) != std::cmp::Ordering::Less {
                rem = rem.sub_big(other).unwrap();
                q[k / 8] |= 1 << (k % 8);
            }
        }
        Some((Atom::from_bytes_le(q), rem))
    }

    /// Increment by one (the only native arithmetic in Loom: rule SUCC).
    pub fn inc(&self) -> Atom {
        let mut v = self.0.clone();
        let mut carry = true;
        for b in v.iter_mut() {
            if carry {
                let (nb, c) = b.overflowing_add(1);
                *b = nb;
                carry = c;
            } else {
                break;
            }
        }
        if carry {
            v.push(1);
        }
        Atom(v) // already normalized: inc never creates a trailing zero
    }

    /// Best-effort small-integer view, for display and the network layer.
    pub fn to_u128(&self) -> Option<u128> {
        if self.0.len() > 16 {
            return None;
        }
        let mut n: u128 = 0;
        for (i, b) in self.0.iter().enumerate() {
            n |= (*b as u128) << (8 * i);
        }
        Some(n)
    }

    /// Interpret the bytes as a UTF-8 cord, if printable.
    pub fn as_cord(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        match std::str::from_utf8(&self.0) {
            Ok(s) if s.chars().all(|c| !c.is_control()) => Some(s.to_string()),
            _ => None,
        }
    }

    /// Interpret the bytes as UTF-8 TEXT: like `as_cord`, but newlines and
    /// tabs are welcome — multi-line cords carry source code, documents, and
    /// markup throughout the system.
    pub fn as_text(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        match std::str::from_utf8(&self.0) {
            Ok(s) if s.chars().all(|c| !c.is_control() || c == '\n' || c == '\t' || c == '\r') => {
                Some(s.to_string())
            }
            _ => None,
        }
    }
}

impl fmt::Debug for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(n) = self.to_u128() {
            write!(f, "{}", n)
        } else {
            write!(f, "0x")?;
            for b in self.0.iter().rev() {
                write!(f, "{:02x}", b)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inc_and_roundtrip() {
        assert_eq!(Atom::zero().inc(), Atom::from_u128(1));
        assert_eq!(Atom::from_u128(255).inc(), Atom::from_u128(256));
        assert_eq!(Atom::from_u128(1_000_000).to_u128(), Some(1_000_000));
    }

    #[test]
    fn bignum_add() {
        assert_eq!(Atom::from_u128(255).add(&Atom::from_u128(1)), Atom::from_u128(256));
        assert_eq!(Atom::from_u128(0).add(&Atom::from_u128(0)), Atom::zero());
        let big = Atom::from_u128(u128::MAX);
        // u128::MAX + 1 must carry into a new byte (exceeds 128 bits)
        let sum = big.add(&Atom::from_u128(1));
        assert!(sum.to_u128().is_none()); // no longer fits in 128 bits
        // commutativity on a few values
        for (x, y) in [(7u128, 19u128), (1000, 24), (0, 99)] {
            assert_eq!(Atom::from_u128(x).add(&Atom::from_u128(y)), Atom::from_u128(x + y));
        }
    }

    #[test]
    fn cord_roundtrip() {
        let a = Atom::from_cord("incr");
        assert_eq!(a.as_cord().as_deref(), Some("incr"));
        // Same value built two ways must be equal (content addressing relies on this).
        assert_eq!(Atom::from_cord("incr"), Atom::from_cord("incr"));
    }
}

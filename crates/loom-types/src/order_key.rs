//! Reusable ordered-membership primitive: opaque, lexicographically comparable order keys with
//! fractional insertion.
//!
//! An [`OrderKey`] is an opaque string over a fixed base-62 alphabet whose byte-lexicographic order
//! is the membership order. [`OrderKey::between`] returns a key strictly between two neighbours (or
//! before the first / after the last), so callers place items with append/first/before/after
//! semantics and never compute or see numeric ranks. Inserting between two existing keys does not
//! renumber their neighbours.
//!
//! This primitive is deliberately facet-agnostic (Lanes is one consumer, not the owner).

use crate::error::{LoomError, Result};

/// Ordered base-62 alphabet. Bytes are in ascending order, so byte-lexicographic comparison of
/// alphabet strings equals comparison by digit index.
const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: usize = 62;

/// The index of an alphabet byte, or `None` if the byte is not a valid order-key digit.
fn digit_index(byte: u8) -> Option<usize> {
    DIGITS.iter().position(|&candidate| candidate == byte)
}

/// The digit value at position `i` in `key`, treating positions beyond the end as the minimum
/// digit (`0`). `key` must contain only valid alphabet bytes.
fn digit_at(key: &[u8], i: usize) -> usize {
    match key.get(i) {
        Some(&byte) => digit_index(byte).expect("order key holds only valid digits"),
        None => 0,
    }
}

/// An opaque, lexicographically comparable order key.
///
/// Ordering derives from the byte value of the underlying string; two keys compare the same way a
/// human would read their membership order, but the string itself is not a rank and carries no
/// externally meaningful magnitude.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderKey(String);

impl OrderKey {
    /// Validate and wrap an existing order-key string.
    ///
    /// A canonical order key is non-empty, contains only base-62 alphabet bytes, and does not end
    /// in the minimum digit (`'0'`); the trailing-zero rule keeps one string per position so
    /// equality and ordering stay in lockstep.
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Err(LoomError::invalid("order key must not be empty"));
        }
        if value.bytes().any(|byte| digit_index(byte).is_none()) {
            return Err(LoomError::invalid(
                "order key must contain only base-62 alphabet characters",
            ));
        }
        if value.as_bytes().last() == Some(&DIGITS[0]) {
            return Err(LoomError::invalid(
                "order key must not end in the minimum digit",
            ));
        }
        Ok(Self(value.to_string()))
    }

    /// The underlying opaque string. Callers persist this but must not expose it as a routine API
    /// surface or interpret it as a rank.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume into the underlying string.
    pub fn into_string(self) -> String {
        self.0
    }

    /// Return a key strictly between `lo` and `hi`.
    ///
    /// `lo == None` means "before everything" (append at front / first) and `hi == None` means
    /// "after everything" (append at end). The caller must pass `lo < hi` when both are present;
    /// the placement helpers in consumers guarantee this.
    pub fn between(lo: Option<&OrderKey>, hi: Option<&OrderKey>) -> OrderKey {
        let generated = key_between(
            lo.map(OrderKey::as_str).unwrap_or(""),
            hi.map(OrderKey::as_str),
        );
        // `key_between` only emits alphabet bytes and always terminates on a non-minimum digit, so
        // the result is a valid canonical key.
        OrderKey(generated)
    }
}

/// Core midstring generation. Returns a string `k` with `lo < k < hi` under byte-lexicographic
/// order, where an empty `lo` is treated as the smallest possible value and `hi == None` as the
/// largest. Precondition: `lo < hi`.
fn key_between(lo: &str, hi: Option<&str>) -> String {
    let lo = lo.as_bytes();
    let mut hi: Option<&[u8]> = hi.map(str::as_bytes);
    let mut result: Vec<u8> = Vec::new();
    let mut i = 0usize;
    loop {
        let d_lo = digit_at(lo, i);
        let d_hi = match hi {
            Some(bytes) => digit_at(bytes, i),
            None => BASE,
        };
        if d_lo == d_hi {
            // Shared digit: commit it and descend one position.
            result.push(DIGITS[d_lo]);
            i += 1;
            continue;
        }
        // Precondition `lo < hi` with an equal prefix so far guarantees `d_lo < d_hi` here.
        let mid = (d_lo + d_hi) / 2;
        if mid > d_lo {
            result.push(DIGITS[mid]);
            return String::from_utf8(result).expect("alphabet bytes are valid UTF-8");
        }
        // `mid == d_lo` (the neighbours are adjacent digits): keep `lo`'s digit and continue
        // with an open upper bound, so the remainder only has to exceed `lo`'s suffix.
        result.push(DIGITS[d_lo]);
        hi = None;
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn between(lo: Option<&OrderKey>, hi: Option<&OrderKey>) -> OrderKey {
        let key = OrderKey::between(lo, hi);
        // Every generated key must be canonical.
        OrderKey::parse(key.as_str()).expect("generated key is canonical");
        key
    }

    #[test]
    fn between_none_none_is_valid_and_midrange() {
        let key = between(None, None);
        assert!(!key.as_str().is_empty());
    }

    #[test]
    fn first_is_before_and_append_is_after() {
        let mid = between(None, None);
        let before = between(None, Some(&mid));
        let after = between(Some(&mid), None);
        assert!(before < mid, "{before:?} < {mid:?}");
        assert!(mid < after, "{mid:?} < {after:?}");
    }

    #[test]
    fn between_two_keys_is_strictly_between() {
        let a = OrderKey::parse("F").unwrap();
        let b = OrderKey::parse("V").unwrap();
        let mid = between(Some(&a), Some(&b));
        assert!(a < mid && mid < b, "{a:?} < {mid:?} < {b:?}");
    }

    #[test]
    fn between_adjacent_digits_descends() {
        let a = OrderKey::parse("V").unwrap();
        let b = OrderKey::parse("W").unwrap();
        let mid = between(Some(&a), Some(&b));
        assert!(a < mid && mid < b, "{a:?} < {mid:?} < {b:?}");
    }

    #[test]
    fn repeated_appends_are_strictly_increasing() {
        let mut prev: Option<OrderKey> = None;
        let mut keys = Vec::new();
        for _ in 0..200 {
            let next = between(prev.as_ref(), None);
            if let Some(prev) = &prev {
                assert!(prev < &next, "append must increase: {prev:?} !< {next:?}");
            }
            prev = Some(next.clone());
            keys.push(next);
        }
        // Global order is preserved.
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn repeated_prepends_are_strictly_decreasing() {
        let mut prev: Option<OrderKey> = None;
        for _ in 0..200 {
            let next = between(None, prev.as_ref());
            if let Some(prev) = &prev {
                assert!(&next < prev, "prepend must decrease: {next:?} !< {prev:?}");
            }
            prev = Some(next);
        }
    }

    #[test]
    fn repeated_inserts_between_same_neighbours_stay_ordered() {
        let mut lo = OrderKey::parse("F").unwrap();
        let hi = OrderKey::parse("V").unwrap();
        // Always insert just after `lo` (before `hi`); the sequence must stay within (lo0, hi).
        for _ in 0..200 {
            let mid = between(Some(&lo), Some(&hi));
            assert!(lo < mid && mid < hi, "{lo:?} < {mid:?} < {hi:?}");
            lo = mid;
        }
    }

    #[test]
    fn interleaved_placements_keep_a_total_order() {
        // Build a small list and repeatedly insert in the middle; verify sortedness each step.
        let mut order: Vec<OrderKey> = Vec::new();
        order.push(between(None, None));
        order.insert(0, between(None, Some(&order[0])));
        let last = order.last().unwrap().clone();
        order.push(between(Some(&last), None));
        for _ in 0..100 {
            let idx = order.len() / 2;
            let lo = order.get(idx - 1).cloned();
            let hi = order.get(idx).cloned();
            let mid = between(lo.as_ref(), hi.as_ref());
            order.insert(idx, mid);
            let mut sorted = order.clone();
            sorted.sort();
            assert_eq!(order, sorted, "list must remain in key order");
        }
    }

    #[test]
    fn parse_rejects_invalid_keys() {
        assert!(OrderKey::parse("").is_err(), "empty");
        assert!(OrderKey::parse("A!").is_err(), "non-alphabet byte");
        assert!(OrderKey::parse("A0").is_err(), "trailing minimum digit");
        assert!(OrderKey::parse("V").is_ok());
    }
}

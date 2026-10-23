//! The two orders search and the object tree use.

use std::cmp::Ordering;

/// Orders two names with digit runs compared by value: `Skin2` before `Skin10`.
///
/// Each name splits into runs of ASCII digits and runs of anything else. A digit run compares
/// by its value, with equal values ordered by length. A run of more than nineteen digits
/// compares as `u64::MAX`. Any other run compares by ASCII-lowercased bytes. A digit run sorts
/// before any other run at the same position, and a name that runs out first sorts first.
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        let (run_a, rest_a) = split_run(a);
        let (run_b, rest_b) = split_run(b);
        let order = match (run_a.first(), run_b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match (x.is_ascii_digit(), y.is_ascii_digit()) {
                (true, true) => digit_value(run_a)
                    .cmp(&digit_value(run_b))
                    .then(run_a.len().cmp(&run_b.len())),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => run_a
                    .iter()
                    .map(u8::to_ascii_lowercase)
                    .cmp(run_b.iter().map(u8::to_ascii_lowercase)),
            },
        };
        if order.is_ne() {
            return order;
        }
        (a, b) = (rest_a, rest_b);
    }
}

/// Orders two paths by bytes with `/` below every other byte.
///
/// Under this order a path sorts before every path beneath it, and every path beneath it sorts
/// before any sibling: `a`, `a/b`, `a-b`.
#[must_use]
pub fn path_cmp(a: &str, b: &str) -> Ordering {
    let key = |byte: u8| match byte {
        b'/' => 0,
        byte => u16::from(byte) + 1,
    };
    a.bytes().map(key).cmp(b.bytes().map(key))
}

/// The leading run of `bytes`, all digits or all non-digits, and the rest.
fn split_run(bytes: &[u8]) -> (&[u8], &[u8]) {
    let Some(first) = bytes.first() else {
        return (bytes, bytes);
    };
    let digits = first.is_ascii_digit();
    let len = bytes
        .iter()
        .position(|byte| byte.is_ascii_digit() != digits)
        .unwrap_or(bytes.len());
    bytes.split_at(len)
}

/// The value of a run of ASCII digits, `u64::MAX` past its range.
fn digit_value(digits: &[u8]) -> u64 {
    digits
        .iter()
        .try_fold(0u64, |value, digit| {
            value.checked_mul(10)?.checked_add(u64::from(digit - b'0'))
        })
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_digit_runs_by_value() {
        let mut names = ["skin10", "skin2", "Skin1", "skin100", "root"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["root", "Skin1", "skin2", "skin10", "skin100"]);
    }

    #[test]
    fn natural_order_breaks_equal_values_by_length_and_caps_long_runs() {
        assert_eq!(natural_cmp("skin1", "skin01"), Ordering::Less);
        assert_eq!(natural_cmp("a", "A"), Ordering::Equal);
        assert_eq!(natural_cmp("ab", "abc"), Ordering::Less);
        assert_eq!(natural_cmp("a1", "ab"), Ordering::Less);
        assert_eq!(
            natural_cmp("x99999999999999999999", "x99999999999999999999999"),
            Ordering::Less
        );
        assert_eq!(
            natural_cmp("x18446744073709551615", "x99999999999999999999"),
            Ordering::Equal
        );
    }

    #[test]
    fn path_order_puts_slash_below_every_byte() {
        let mut paths = ["a-b", "a/b", "a", "a/b/c", "a/c", "a.b"];
        paths.sort_by(|a, b| path_cmp(a, b));
        assert_eq!(paths, ["a", "a/b", "a/b/c", "a/c", "a-b", "a.b"]);
    }
}

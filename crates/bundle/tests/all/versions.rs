//! The order of versions, newest last.
use routeh_plugin_bundle::feed::newer;
use std::cmp::Ordering;

#[test]
fn numbers_are_compared_as_numbers() {
    assert_eq!(newer("1.10.0", "1.9.3"), Ordering::Greater);
    assert_eq!(newer("1.0", "1.0"), Ordering::Equal);
    assert_eq!(newer("1.0", "1.0.1"), Ordering::Less);
    assert_eq!(newer("0.3.1-r2", "0.3.1-r10"), Ordering::Less);
    assert_eq!(newer("2.0a", "2.0b"), Ordering::Less);
    assert_eq!(newer("2.0", "2.0a"), Ordering::Less);
    assert_eq!(newer("010", "9"), Ordering::Greater);
}

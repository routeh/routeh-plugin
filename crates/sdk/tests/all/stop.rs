//! The flag that ends a service.
use routeh_plugin_sdk::Stop;

#[test]
fn a_stop_is_shared_by_its_clones() {
    let stop = Stop::new();
    assert!(!stop.is_stopped());
    let other = stop.clone();
    other.stop();
    assert!(stop.is_stopped());
    assert!(!Stop::default().is_stopped());
}

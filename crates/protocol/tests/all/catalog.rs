//! The installed manifests by id.
use routeh_plugin_protocol::{Catalog, Manifest};

#[test]
fn finds_a_plugin_and_its_tunnel_kind() {
    let awg = Manifest::parse(crate::AWG).unwrap();
    let demo = Manifest::parse(crate::MINIMAL).unwrap();
    let catalog = Catalog::new([awg, demo]).unwrap();
    assert_eq!(
        catalog.ids().map(|id| id.as_str()).collect::<Vec<_>>(),
        ["awg", "demo"]
    );
    assert_eq!(catalog.get("awg").unwrap().name.as_str(), "AmneziaWG");
    assert_eq!(catalog.tunnel_kind("awg").unwrap().link_kind, "amneziawg");
    assert!(
        catalog.tunnel_kind("demo").is_none(),
        "a plugin without [tunnel] is no kind"
    );
    assert!(catalog.get("wg").is_none());
    assert!(Catalog::default().get("awg").is_none());
}

#[test]
fn has_one_manifest_of_an_id() {
    let one = Manifest::parse(crate::MINIMAL).unwrap();
    let two = Manifest::parse(crate::MINIMAL).unwrap();
    assert_eq!(
        Catalog::new([one, two]).unwrap_err().to_string(),
        "demo: two manifests have this id"
    );
}

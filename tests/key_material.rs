use std::fs;

use hap_resigner::materials::load_signing_identity;

#[test]
fn selects_ca_leaf_by_public_key_instead_of_p12_placeholder() {
    let p12 = fs::read("tests/fixtures/placeholder.p12").expect("P12 fixture");
    let chain = fs::read("tests/fixtures/formal-chain.pem").expect("certificate chain fixture");
    let expected_leaf = fs::read("tests/fixtures/formal-leaf.der").expect("formal leaf fixture");

    let identity = load_signing_identity(&p12, "123456", &chain)
        .expect("matching private key and formal certificate");

    assert_eq!(identity.alias, "debugKey");
    assert_eq!(identity.certificates.len(), 2);
    assert_eq!(identity.certificates[0], expected_leaf);
}

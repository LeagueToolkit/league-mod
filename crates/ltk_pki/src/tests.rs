use crate::io::modsig::{ModSig, ModSigEntry, ModSigEntryList};
use crate::pki::sign::SignerKeys;
use crate::pki::verify::VerifyContextBuilder;

mod data;

#[test]
fn test_sign_and_verify() {
    let mut alist = ModSigEntryList::default();
    alist.entries.push(ModSigEntry {
        name: 0x1122,
        checksum_compressed: 0x3344,
        checksum_uncompressed: 0x5566,
    });
    let alist_data = alist.dump().expect("Failed to encode modsig entry list!");

    let mut keys = match SignerKeys::from_pem_str(data::TEST_ROOT_KEY) {
        Err(err) => panic!("parse keys: {err:?}"),
        Ok(s) => s,
    };

    // our key is first!
    assert!(!keys.keys.is_empty());
    while keys.keys.len() > 1 {
        keys.keys.pop();
    }

    let content_info = match keys.sign(&alist_data) {
        Err(err) => panic!("generate signature: {err:?}"),
        Ok(s) => s,
    };

    let sig = match ModSig::from_content_info(&content_info) {
        Err(err) => panic!("parse sig: {err:?}"),
        Ok(s) => s,
    };

    let mut verifier = VerifyContextBuilder::default();

    verifier.with_unix_time_sec(1752525266);

    match verifier.add_anchor_der(data::TEST_ROOT_CERT) {
        Err(err) => panic!("add root: {err:?}"),
        Ok(_s) => {}
    };

    match verifier.add_crl_der(data::TEST_ROOT_CRL) {
        Err(err) => panic!("add root crl: {err:?}"),
        Ok(_s) => {}
    };

    match verifier.add_certs_from_signed_data(sig.signed_data()) {
        Err(err) => panic!("add certs: {err:?}"),
        Ok(_s) => {}
    };

    match verifier.add_crls_from_signed_data(sig.signed_data()) {
        Err(err) => panic!("add crls: {err:?}"),
        Ok(_s) => {}
    };

    let verifier = verifier.finalize();
    match verifier.verify_end_cert(0) {
        Err(err) => panic!("verify end cert: {err:?}"),
        Ok(_cert) => {}
    }

    match verifier.verify_signed_data(sig.signed_data(), None) {
        Err(err) => panic!("verify signed: {err:?}"),
        Ok(_cert) => {}
    }
}

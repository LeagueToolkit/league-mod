use cms::signed_data::SignerInfo;
use der::{
    Encode,
    oid::{
        ObjectIdentifier,
        db::{
            rfc5911::ID_MESSAGE_DIGEST,
            rfc5912::{
                ID_SHA_256, ID_SHA_384, ID_SHA_512, RSA_ENCRYPTION, SHA_256_WITH_RSA_ENCRYPTION,
                SHA_384_WITH_RSA_ENCRYPTION, SHA_512_WITH_RSA_ENCRYPTION,
            },
        },
    },
};
use rustls_pki_types::{Der, SignatureVerificationAlgorithm};
use sha2::Digest;
use thiserror::Error;
use webpki::ring;

// Standard OIDs for PKCS#1 RSA Encryption signature algorithms

pub const OID_CMS_MESSAGE_DIGEST: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");

#[derive(Error, Debug, Clone)]
pub enum SignatureError {
    #[error("Dumping attributes der: {err}")]
    DumpDERAttributes { err: der::Error },

    #[error("Signed attributes digest missing")]
    SignedAttributesDigestMissing,

    #[error("Signed attributes digest duplicated")]
    SignedAttributesDigestDuplicated,

    #[error("Signed attributes digest has no value")]
    SignedAttributesDigestValueNone,

    #[error("Signed attributes digest has more than one value")]
    SignedAttributesDigestValueMulti,

    #[error("Signed attributes digest mismatch")]
    SignedAttributesDigestMismatch,

    #[error("Verify data failed unsupported digest algorithm: {digest_alg}")]
    UnsupportedDigestAlgorithm { digest_alg: ObjectIdentifier },

    #[error("Signed attribute content-type missing")]
    SignedAttributesContentTypeMissing,

    #[error("Signed attribute content-type duplicated")]
    SignedAttributesContentTypeDuplicated,

    #[error("Signed attribute content-type decode: {err}")]
    SignedAttributesContentTypeDecode { err: der::Error },

    #[error("Signed attribute content-type missing wanted({wanted}) in_attr({in_attr})")]
    SignedAttributesContentTypeMismatch {
        wanted: ObjectIdentifier,
        in_attr: ObjectIdentifier,
    },

    #[error(
        "Verify data failed unsupported algorithm digest {digest_alg} signature: {signature_algorithm}"
    )]
    UnsupportedSignatureAlgorithm {
        digest_alg: ObjectIdentifier,
        signature_algorithm: ObjectIdentifier,
    },

    #[error("Verify data failed because: {err}")]
    Failed { err: webpki::Error },
}

fn find_signer_algo(
    signer_info: &SignerInfo,
) -> Result<&dyn SignatureVerificationAlgorithm, SignatureError> {
    match (
        signer_info.digest_alg.oid,
        signer_info.signature_algorithm.oid,
    ) {
        (ID_SHA_256, RSA_ENCRYPTION) => Ok(ring::RSA_PKCS1_2048_8192_SHA256),
        (ID_SHA_384, RSA_ENCRYPTION) => Ok(ring::RSA_PKCS1_2048_8192_SHA384),
        (ID_SHA_512, RSA_ENCRYPTION) => Ok(ring::RSA_PKCS1_2048_8192_SHA512),
        (ID_SHA_256, SHA_256_WITH_RSA_ENCRYPTION) => Ok(ring::RSA_PKCS1_2048_8192_SHA256),
        (ID_SHA_384, SHA_384_WITH_RSA_ENCRYPTION) => Ok(ring::RSA_PKCS1_2048_8192_SHA384),
        (ID_SHA_512, SHA_512_WITH_RSA_ENCRYPTION) => Ok(ring::RSA_PKCS1_2048_8192_SHA512),
        (digest_alg, _) if ![ID_SHA_256, ID_SHA_384, ID_SHA_512].contains(&digest_alg) => {
            Err(SignatureError::UnsupportedDigestAlgorithm { digest_alg })
        }
        (digest_alg, signature_algorithm) => Err(SignatureError::UnsupportedSignatureAlgorithm {
            digest_alg,
            signature_algorithm,
        }),
    }
}

fn compare_message_digest(
    econtent_oid: &der::oid::ObjectIdentifier,
    data: &[u8],
    signer_info: &cms::signed_data::SignerInfo,
) -> Result<(), SignatureError> {
    let Some(attributes) = &signer_info.signed_attrs else {
        return Ok(());
    };
    let mut attrs = attributes
        .iter()
        .filter(|attr| attr.oid == ID_MESSAGE_DIGEST);
    let Some(digest) = attrs.next() else {
        return Err(SignatureError::SignedAttributesDigestMissing);
    };
    if attrs.next().is_some() {
        return Err(SignatureError::SignedAttributesDigestDuplicated);
    }
    let Some(digest_value) = digest.values.get(0) else {
        return Err(SignatureError::SignedAttributesDigestValueNone);
    };
    if digest.values.len() > 1 {
        return Err(SignatureError::SignedAttributesDigestValueMulti);
    }
    let matches = match signer_info.digest_alg.oid {
        ID_SHA_256 => (sha2::Sha256::digest(data)[..]).eq(digest_value.value()),
        ID_SHA_384 => (sha2::Sha384::digest(data)[..]).eq(digest_value.value()),
        ID_SHA_512 => (sha2::Sha512::digest(data)[..]).eq(digest_value.value()),
        digest_alg => return Err(SignatureError::UnsupportedDigestAlgorithm { digest_alg }),
    };
    if !matches {
        return Err(SignatureError::SignedAttributesDigestMismatch);
    }
    let mut content_type = signer_info
        .signed_attrs
        .iter()
        .flat_map(|x| x.as_ref())
        .filter(|attr| {
            attr.oid.cmp(&der::oid::db::rfc5911::ID_CONTENT_TYPE) == std::cmp::Ordering::Equal
        });
    if let Some(content_type) = content_type.next() {
        if content_type.values.len() > 1 {
            return Err(SignatureError::SignedAttributesContentTypeDuplicated);
        }
        if let Some(content_type_value) = content_type.values.get(0) {
            let content_type_value = content_type_value
                .decode_as::<ObjectIdentifier>()
                .map_err(|err| SignatureError::SignedAttributesContentTypeDecode { err })?;
            if econtent_oid.cmp(&content_type_value) != std::cmp::Ordering::Equal {
                return Err(SignatureError::SignedAttributesContentTypeMismatch {
                    wanted: *econtent_oid,
                    in_attr: content_type_value,
                });
            }
        } else {
            return Err(SignatureError::SignedAttributesContentTypeMissing);
        }
    } else {
        return Err(SignatureError::SignedAttributesContentTypeMissing);
    }
    if content_type.next().is_some() {
        return Err(SignatureError::SignedAttributesContentTypeDuplicated);
    }
    Ok(())
}

pub fn verify_data_signature(
    cert: &webpki::EndEntityCert<'_>,
    econtent_oid: &der::oid::ObjectIdentifier,
    data: &[u8],
    signer_info: &cms::signed_data::SignerInfo,
) -> Result<(), SignatureError> {
    let signature_alg = find_signer_algo(signer_info)?;
    let msg = match &signer_info.signed_attrs {
        None => Der::from_slice(data),
        Some(attributes) => {
            compare_message_digest(econtent_oid, data, signer_info)?;
            match attributes.to_der() {
                Err(err) => return Err(SignatureError::DumpDERAttributes { err }),
                Ok(bytes) => Der::from(bytes),
            }
        }
    };
    let signature = signer_info.signature.as_bytes();
    match cert.verify_signature(signature_alg, &msg[..], signature) {
        Err(err) => Err(SignatureError::Failed { err }),
        Ok(_) => Ok(()),
    }
}

pub fn cert_skid(
    cert: &cms::cert::x509::Certificate,
) -> Option<cms::cert::x509::ext::pkix::SubjectKeyIdentifier> {
    cert
            .tbs_certificate
            .extensions
            .iter()
            .flatten()
            .find(|x| x.extn_id == <cms::cert::x509::ext::pkix::SubjectKeyIdentifier as der::oid::AssociatedOid>::OID)
            .map(|x| cms::cert::x509::ext::pkix::SubjectKeyIdentifier(x.extn_value.clone()))
}

pub fn cert_issuer_and_serial(
    cert: &cms::cert::x509::Certificate,
) -> cms::cert::IssuerAndSerialNumber {
    cms::cert::IssuerAndSerialNumber {
        issuer: cert.tbs_certificate.issuer.clone(),
        serial_number: cert.tbs_certificate.serial_number.clone(),
    }
}

pub fn cert_skid_or_sid(cert: &cms::cert::x509::Certificate) -> cms::signed_data::SignerIdentifier {
    cert_skid(cert)
        .map(cms::signed_data::SignerIdentifier::SubjectKeyIdentifier)
        .unwrap_or_else(|| {
            cms::signed_data::SignerIdentifier::IssuerAndSerialNumber(cert_issuer_and_serial(cert))
        })
}

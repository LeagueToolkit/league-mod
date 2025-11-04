use cms::builder::SignerInfoBuilder;
use cms::cert::CertificateChoices;
use cms::cert::x509::crl::CertificateList;
use cms::content_info::ContentInfo;
use cms::revocation::RevocationInfoChoice;
use cms::signed_data::EncapsulatedContentInfo;
use cms::{builder::SignedDataBuilder, cert::x509::Certificate};
use der::Decode;
use p12::PFX;
use rsa::RsaPrivateKey;
use rsa::pkcs8::spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use thiserror::Error;

use crate::pki::util::cert_skid_or_sid;

#[derive(Error, Debug, Clone)]
pub enum SignerError {
    #[error("Failed to extract certificates: {err}")]
    ExtractBags { err: String },

    #[error("Failed to decode pem: {err}")]
    PemDecodeFailed { err: String },

    #[error("Failed to parse key: {err}")]
    ParseDERKey { err: der::Error },

    #[error("Failed to parse X509 Certificate: {err}")]
    ParseDERCert { err: der::Error },

    #[error("Failed to parse X509 CRL: {err}")]
    ParseDERCRL { err: der::Error },

    #[error("Failed to parse PKCS8 private key: {err}")]
    ParsePKCS8Key { err: rsa::pkcs8::Error },

    #[error("Failed to parse SPKI public key: {err}")]
    ParseSPKIKey { err: rsa::pkcs8::spki::Error },

    #[error("Failed to parse SignedData: {err}")]
    ParseSignedData { err: der::Error },

    #[error("Failed to generate signature: {err}")]
    SignatureGenerate { err: String },

    #[error("No private key found")]
    NoPrivateKey,

    #[error("More than one private key found")]
    TooManyPrivateKeys,

    #[error("No signing certificate found")]
    NoSigningCert,

    #[error("Failed to validate sign data econtent as der: {err}")]
    SignDataContentDER { err: der::Error },

    #[error("Failed to add certificate to signed data: {err}")]
    SignDataCertFailed { err: String },

    #[error("Failed to add certificate to signed data: {err}")]
    SignDataCrlFailed { err: String },
}

fn is_crl_oid(oid: &[u64]) -> bool {
    const CRL_OIDS: &[&[u64]] = &[
        &[1, 2, 840, 113549, 1, 9, 23],        // PKCS#9 CRL
        &[2, 5, 4, 39],                        // X.509 CRL attribute
        &[1, 2, 840, 113549, 1, 12, 10, 1, 4], // PKCS#12 CRL bag
    ];

    CRL_OIDS.contains(&oid)
}

#[derive(Debug, Default)]
pub struct SignerKeys {
    pub keys: Vec<(SubjectPublicKeyInfoOwned, RsaPrivateKey)>,
    pub certs: Vec<Certificate>,
    pub crls: Vec<CertificateList>,
}

impl SignerKeys {
    pub fn from_keystore(pfx: &PFX, password: &str) -> Result<Self, SignerError> {
        let mut result = Self::default();

        // Convert key once to utf16-be zero terminated.
        let bmp_password = password
            .encode_utf16()
            .flat_map(|u| [(u >> 8) as u8, (u & 0xFF) as u8])
            .chain([0x00, 0x00]) // null terminator
            .collect::<Box<_>>();

        // Extract
        let bags = pfx.bags(password).map_err(|err| SignerError::ExtractBags {
            err: format!("{err}"),
        })?;
        for bag in bags {
            match bag.bag {
                // Private keys
                p12::SafeBagKind::Pkcs8ShroudedKeyBag(key_bag) => {
                    if let Some(key_data) = key_bag.decrypt(&bmp_password) {
                        result.add_key_pkcs8_der(&key_data)?
                    };
                }
                // Certs
                p12::SafeBagKind::CertBag(p12::CertBag::X509(cert_x509)) => {
                    result.add_cert_der(&cert_x509)?
                }
                // CRLs
                p12::SafeBagKind::OtherBagKind(other_bag) => {
                    if is_crl_oid(other_bag.bag_id.as_ref()) {
                        result.add_crl_der(&other_bag.bag_value)?;
                    }
                }
                _ => {}
            }
        }

        Ok(result)
    }

    pub fn from_pem_str(s: &str) -> Result<Self, SignerError> {
        let mut result = Self::default();
        let pem = pem::parse_many(s).map_err(|err| SignerError::PemDecodeFailed {
            err: format!("{err}"),
        })?;
        for pem_block in pem.into_iter() {
            match pem_block.tag() {
                "PRIVATE KEY" => result.add_key_pkcs8_der(pem_block.contents())?,
                "CRL" | "X509 CRL" => result.add_crl_der(pem_block.contents())?,
                "CERTIFICATE" | "X509 CERTIFICATE" => {
                    result.add_cert_der(pem_block.contents())?;
                }
                _ => {}
            }
        }
        Ok(result)
    }

    pub fn add_key_pkcs8_der(&mut self, key_data: &[u8]) -> Result<(), SignerError> {
        // FIXME: handle pkcs1 as well ?
        let key = <RsaPrivateKey as rsa::pkcs8::DecodePrivateKey>::from_pkcs8_der(key_data)
            .map_err(|err| SignerError::ParsePKCS8Key { err })?;

        let spki = SubjectPublicKeyInfoOwned::from_key(key.to_public_key())
            .map_err(|err| SignerError::ParseSPKIKey { err })?;

        self.keys.push((spki, key));
        Ok(())
    }

    pub fn add_cert_der(&mut self, cert_x509: &[u8]) -> Result<(), SignerError> {
        let cert =
            Certificate::from_der(cert_x509).map_err(|err| SignerError::ParseDERCert { err })?;
        self.certs.push(cert);
        Ok(())
    }

    pub fn add_crl_der(&mut self, crl_x509: &[u8]) -> Result<(), SignerError> {
        let crl =
            CertificateList::from_der(crl_x509).map_err(|err| SignerError::ParseDERCRL { err })?;
        self.crls.push(crl);
        Ok(())
    }

    pub fn sign(&self, data: &[u8]) -> Result<ContentInfo, SignerError> {
        if self.keys.is_empty() {
            return Err(SignerError::NoPrivateKey);
        }

        if self.keys.len() > 1 {
            return Err(SignerError::TooManyPrivateKeys);
        }

        if self.certs.is_empty() {
            return Err(SignerError::NoSigningCert);
        }

        let (cert, private_key) = self
            .keys
            .iter()
            .filter_map(|(spki, private_key)| {
                self.certs
                    .iter()
                    .filter_map(|cert| {
                        let cert_spki = &cert.tbs_certificate.subject_public_key_info;
                        if cert_spki.algorithm == spki.algorithm
                            && cert_spki.subject_public_key.as_bytes()
                                == spki.subject_public_key.as_bytes()
                        {
                            Some((cert, private_key))
                        } else {
                            None
                        }
                    })
                    .find(|_| true)
            })
            .find(|_| true)
            .ok_or(SignerError::NoSigningCert)?;

        let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha512>::new(private_key.clone());
        let sid = cert_skid_or_sid(cert);

        let econtent = der::Any::new(der::Tag::BitString, data)
            .map_err(|err| SignerError::SignDataContentDER { err })?;
        let encapsulated_info = EncapsulatedContentInfo {
            econtent_type: der::oid::db::rfc5911::ID_DATA,
            econtent: Some(econtent),
        };

        let signer_info_builder = SignerInfoBuilder::new(
            &signing_key,
            sid,
            AlgorithmIdentifierOwned {
                oid: der::oid::db::rfc5912::ID_SHA_512,
                parameters: None,
            },
            &encapsulated_info,
            None,
        )
        .map_err(|err| SignerError::SignatureGenerate {
            err: format!("build start: {err}"),
        })?;

        let mut builder = SignedDataBuilder::new(&encapsulated_info);
        let mut builder = &mut builder;

        for cert in &self.certs {
            builder = builder
                .add_certificate(CertificateChoices::Certificate(cert.clone()))
                .map_err(|err| SignerError::SignDataCertFailed {
                    err: format!("{err}"),
                })?;
        }

        for crl in &self.crls {
            builder = builder
                .add_crl(RevocationInfoChoice::Crl(crl.clone()))
                .map_err(|err| SignerError::SignDataCrlFailed {
                    err: format!("{err}"),
                })?;
        }

        builder
            .add_signer_info(signer_info_builder)
            .map_err(|err| SignerError::SignatureGenerate {
                err: format!("build info: {err}"),
            })?;

        builder
            .build()
            .map_err(|err| SignerError::SignatureGenerate {
                err: format!("build finalize: {err}"),
            })
    }
}

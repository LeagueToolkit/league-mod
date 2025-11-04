use cms::{
    cert::{
        CertificateChoices, IssuerAndSerialNumber,
        x509::{self, ext::pkix::SubjectKeyIdentifier},
    },
    revocation::RevocationInfoChoice,
};
use der::{Decode, Encode, oid::AssociatedOid};
use rustls_pki_types::{CertificateDer, TrustAnchor, UnixTime};
use sha2::Digest;
use std::{
    borrow::Cow,
    cell::OnceCell,
    collections::{HashMap, HashSet},
    hash::Hasher,
    time::Duration,
};
use thiserror::Error;
use webpki::VerifiedPath;

use crate::pki::util::{cert_issuer_and_serial, cert_skid};

// https://oidref.com/1.3.6.1.5.5.7.3.3 looks like best fit ? maybe 1.3.6.1.5.5.7.3.4 would work as well
const EKU_CODE_SIGNING: &[u8] = &[40 + 3, 6, 1, 5, 5, 7, 3, 3];

#[derive(Error, Debug, Clone)]
pub enum VerifyError {
    #[error("Failed to parse webpki cert because: {err}")]
    ParseWebPKICert { err: webpki::Error },

    #[error("Failed to parse cms cert because: {err}")]
    ParseCMSCert { err: der::Error },

    #[error("Failed to parse webpki crl because: {err}")]
    ParseWebPKICrl { err: webpki::Error },

    #[error("Failed to parse cms crl because: {err}")]
    ParseCMSCrl { err: der::Error },

    #[error("Failed to dump der cert because: {err}")]
    DumpDERCert { err: der::Error },

    #[error("Failed to dump der crl because: {err}")]
    DumpDERCrl { err: der::Error },

    #[error("Cert usage must be either data({is_end}) sign or ca/crl/key({is_ca}) sign")]
    BadUsage { is_end: bool, is_ca: bool },

    #[error("Cert index out of bounds {idx}/{bound}")]
    CertIndexOutOfBounds { idx: usize, bound: usize },

    #[error("No CRLS")]
    CrlsEmpty,

    #[error("Verify cert failed because: {err}")]
    VerifyCertFailed { err: webpki::Error },

    #[error("Verify data no signers")]
    VerifyDataNoSigners,

    #[error("Verify data no content")]
    VerifyDataNoContent,

    #[error("Verify data failed to encode content der")]
    VerifyDataDEREncode { err: der::Error },

    #[error("Verify data failed because: {err}")]
    VerifyDataFailed { err: super::util::SignatureError },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IssuerAndSerialNumberWithHash(pub IssuerAndSerialNumber);

impl std::hash::Hash for IssuerAndSerialNumberWithHash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.0.to_der() {
            Ok(der_bytes) => der_bytes.hash(state),
            Err(_) => format!("{:?}", self.0).hash(state),
        }
    }
}

impl From<IssuerAndSerialNumber> for IssuerAndSerialNumberWithHash {
    fn from(inner: IssuerAndSerialNumber) -> Self {
        Self(inner)
    }
}

impl AsRef<IssuerAndSerialNumber> for IssuerAndSerialNumberWithHash {
    fn as_ref(&self) -> &IssuerAndSerialNumber {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SubjectKeyIdentifierWithHash(pub SubjectKeyIdentifier);

impl std::hash::Hash for SubjectKeyIdentifierWithHash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.0.to_der() {
            Ok(der_bytes) => der_bytes.hash(state),
            Err(_) => format!("{:?}", self.0).hash(state),
        }
    }
}

impl From<SubjectKeyIdentifier> for SubjectKeyIdentifierWithHash {
    fn from(inner: SubjectKeyIdentifier) -> Self {
        Self(inner)
    }
}

impl AsRef<SubjectKeyIdentifier> for SubjectKeyIdentifierWithHash {
    fn as_ref(&self) -> &SubjectKeyIdentifier {
        &self.0
    }
}

#[derive(Debug, Default)]
pub struct VerifyContextBuilder {
    anchors: Vec<TrustAnchor<'static>>,
    ca_certs: Vec<CertificateDer<'static>>,
    end_certs: Vec<CertificateDer<'static>>,
    seen_certs: HashSet<[u8; 32]>,
    lookup_cert_by_issuer_and_serial_number: HashMap<IssuerAndSerialNumberWithHash, Vec<usize>>,
    lookup_cert_by_subject_key_identifier: HashMap<SubjectKeyIdentifierWithHash, Vec<usize>>,
    crls: Vec<(std::time::Duration, webpki::CertRevocationList<'static>)>,
    seen_crls: HashSet<[u8; 32]>,
    time: Option<UnixTime>,
}

pub struct VerifyContext<'a> {
    anchors: &'a [TrustAnchor<'static>],
    ca_certs: &'a [CertificateDer<'static>],
    end_certs: Vec<(webpki::EndEntityCert<'a>, OnceCell<Result<(), VerifyError>>)>,
    lookup_cert_by_issuer_and_serial_number: &'a HashMap<IssuerAndSerialNumberWithHash, Vec<usize>>,
    lookup_cert_by_subject_key_identifier: &'a HashMap<SubjectKeyIdentifierWithHash, Vec<usize>>,
    crls: Vec<&'a webpki::CertRevocationList<'static>>,
    time: UnixTime,
}

#[derive(Debug, Clone, Eq, PartialEq)]
/// Summoner structure used for binding certificates to a specific summoner identity.
pub struct Summoner<'a, 'b> {
    pub marker: Cow<'a, [u8]>,
    pub value: Cow<'b, [u8]>,
}

impl<'a, 'b> Summoner<'a, 'b> {
    /// Creates a new Summoner instance from the given summoner ID.
    pub fn from_summoner(summoner_id: &str) -> Self {
        let prefix = "summoner-";
        let hash = sha2::Sha256::digest(summoner_id.as_bytes());
        let marker = Cow::Owned(prefix.as_bytes().to_vec());
        let value = Cow::Owned(format!("summoner-{}", hex::encode(hash)).into_bytes());
        Self { marker, value }
    }

    /// Leaks the Summoner instance to have a 'static lifetime.
    pub fn leak(&self) -> &'static Summoner<'static, 'static> {
        let pinned = Summoner {
            marker: Cow::Borrowed(self.marker.to_vec().leak()),
            value: Cow::Borrowed(self.value.to_vec().leak()),
        };
        Box::leak(Box::new(pinned))
    }
}

unsafe impl Send for Summoner<'static, 'static>
where
    Cow<'static, [u8]>: Send,
    Cow<'static, [u8]>: Send,
{
}
unsafe impl Sync for Summoner<'static, 'static>
where
    Cow<'static, [u8]>: Sync,
    Cow<'static, [u8]>: Sync,
{
}

impl<'a> VerifyContext<'a> {
    /// Returns the number of end entity certificates available in the context.
    pub fn count_end_certs(&self) -> usize {
        self.end_certs.len()
    }

    /// Verifies and caches the given end entity certificate at the specified index.
    pub fn verify_end_cert(&self, index: usize) -> Result<&webpki::EndEntityCert<'a>, VerifyError> {
        let Some((cert, cell)) = self.end_certs.get(index) else {
            return Err(VerifyError::CertIndexOutOfBounds {
                idx: index,
                bound: self.end_certs.len(),
            });
        };
        cell.get_or_init(|| {
            self.verify_end_cert_with(
                cert,
                self.time,
                webpki::RevocationCheckDepth::Chain,
                webpki::UnknownStatusPolicy::Deny,
                webpki::ExpirationPolicy::Enforce,
            )
            .map(|_| ())
        })
        .clone()
        .map(|_| cert)
    }

    /// Verifies the given end entity certificate with the provided parameters.
    pub fn verify_end_cert_with<'b>(
        &self,
        cert: &'b webpki::EndEntityCert,
        time: UnixTime,
        depth: webpki::RevocationCheckDepth,
        status_policy: webpki::UnknownStatusPolicy,
        expiration_policy: webpki::ExpirationPolicy,
    ) -> Result<VerifiedPath<'b>, VerifyError>
    where
        'a: 'b,
    {
        let revocation = if !self.crls.is_empty() {
            // we check for empty condition before so this can never fail
            let revocation = webpki::RevocationOptionsBuilder::new(&self.crls)
                .expect("Should never happen!")
                .with_depth(depth)
                .with_status_policy(status_policy)
                .with_expiration_policy(expiration_policy)
                .build();
            Some(revocation)
        } else {
            None
        };
        cert.verify_for_usage(
            crate::consts::SUPPORTED_SIG_ALGS,
            self.anchors,
            self.ca_certs,
            time,
            webpki::KeyUsage::required_if_present(EKU_CODE_SIGNING),
            revocation,
            None,
        )
        .map_err(|err| VerifyError::VerifyCertFailed { err })
    }

    /// Finds all candidate end entity certificates that could have signed data for the given signer identifier.
    pub fn find_signer_candidates(&self, id: &cms::signed_data::SignerIdentifier) -> &[usize] {
        match id {
            cms::signed_data::SignerIdentifier::IssuerAndSerialNumber(issuer_and_serial_number) => {
                self.lookup_cert_by_issuer_and_serial_number
                    .get(&IssuerAndSerialNumberWithHash(
                        issuer_and_serial_number.clone(),
                    ))
            }
            cms::signed_data::SignerIdentifier::SubjectKeyIdentifier(subject_key_identifier) => {
                self.lookup_cert_by_subject_key_identifier
                    .get(&SubjectKeyIdentifierWithHash(
                        subject_key_identifier.clone(),
                    ))
            }
        }
        .map(|x| &x[..])
        .unwrap_or(&[])
    }

    /// Checks the signature of the signed data against all available end entity certificates.
    /// Optionally allows summoner bound certificates if they match provided summoner.
    /// If no summoner is provided, then all certificates are allowed.
    /// Certificates that are not bound to the summoner are always allowed.
    pub fn verify_signed_data(
        &self,
        signed_data: &cms::signed_data::SignedData,
        summoner: Option<Summoner>,
    ) -> Result<(), VerifyError> {
        let mut first_error = None;
        let Some(content) = &signed_data.encap_content_info.econtent else {
            return Err(VerifyError::VerifyDataNoContent);
        };
        let encontent_oid = signed_data.encap_content_info.econtent_type;
        let msg = content.value();
        for signer in signed_data.signer_infos.0.iter() {
            for candidate in self.find_signer_candidates(&signer.sid) {
                match self.verify_end_cert(*candidate) {
                    Ok(cert) => {
                        // Do we have summoner to check against ?
                        if let Some(summoner) = summoner.as_ref() {
                            let subject = cert.subject();
                            // Check if this certificate is bound to a summoner.
                            if !subject
                                .windows(summoner.marker.len())
                                .any(|window| window == summoner.marker.as_ref())
                            {
                                // Not a match, continue to next candidate.
                                if !subject
                                    .windows(summoner.value.len())
                                    .any(|window| window == summoner.value.as_ref())
                                {
                                    continue;
                                }
                            }
                        }
                        match super::util::verify_data_signature(cert, &encontent_oid, msg, signer)
                        {
                            Ok(_) => return Ok(()),
                            Err(err) => {
                                if first_error.is_none() {
                                    first_error = Some(VerifyError::VerifyDataFailed { err });
                                }
                            }
                        }
                    }
                    Err(err) => {
                        if first_error.is_none() {
                            first_error = Some(err);
                        }
                    }
                }
            }
        }
        Err(first_error.unwrap_or(VerifyError::VerifyDataNoSigners))
    }
}

impl VerifyContextBuilder {
    /// Sets the current Unix time for the verification context.
    pub fn with_unix_time(&mut self, time: UnixTime) -> &mut Self {
        self.time = Some(time);
        self
    }

    /// Sets the current Unix time in seconds for the verification context.
    pub fn with_unix_time_sec(&mut self, secs: u64) -> &mut Self {
        self.time = Some(UnixTime::since_unix_epoch(Duration::from_secs(secs)));
        self
    }

    /// Adds a trust anchor from the given DER-encoded certificate data.
    pub fn add_anchor_der(&mut self, data: &[u8]) -> Result<&mut Self, VerifyError> {
        let der = CertificateDer::from_slice(data);
        let anchor = webpki::anchor_from_trusted_cert(&der)
            .map_err(|err| VerifyError::ParseWebPKICert { err })?;
        let hash: [u8; 32] = sha2::Sha256::digest(der.as_ref()).into();
        if !self.seen_certs.insert(hash) {
            return Ok(self);
        }
        self.anchors.push(anchor.to_owned());
        Ok(self)
    }

    /// Adds a certificate from the given DER-encoded certificate data.
    pub fn add_cert_der(&mut self, data: &[u8]) -> Result<&mut Self, VerifyError> {
        // parses as webpki certificate ?
        let der = CertificateDer::from(data.to_vec());
        let _ = webpki::EndEntityCert::try_from(&der)
            .map_err(|err| VerifyError::ParseWebPKICert { err })?;

        // parses as cms/rustcrypto certificate ?
        let cert = cms::cert::x509::Certificate::from_der(der.as_ref())
            .map_err(|err| VerifyError::ParseCMSCert { err })?;

        // extract key usage
        let mut is_ca = false;
        let mut is_end = false;
        for ext in cert.tbs_certificate.extensions.iter().flatten() {
            if ext.extn_id == x509::ext::pkix::BasicConstraints::OID {
                let bc = x509::ext::pkix::BasicConstraints::from_der(ext.extn_value.as_bytes())
                    .map_err(|err| VerifyError::ParseCMSCert { err })?;
                if bc.ca {
                    is_ca = true;
                }
            }
            if ext.extn_id == x509::ext::pkix::KeyUsage::OID {
                let ku = x509::ext::pkix::KeyUsage::from_der(ext.extn_value.as_bytes())
                    .map_err(|err| VerifyError::ParseCMSCert { err })?;
                if ku.digital_signature() {
                    is_end = true;
                }
                if ku.crl_sign() || ku.key_cert_sign() {
                    is_ca = true;
                }
            }
        }

        // certificate must be either ca or end cert
        if !is_ca && !is_end {
            return Err(VerifyError::BadUsage { is_end, is_ca });
        }

        // save some space by not duplicating same entries
        let hash: [u8; 32] = sha2::Sha256::digest(der.as_ref()).into();
        if !self.seen_certs.insert(hash) {
            return Ok(self);
        }

        // no need to lookup ca certs
        if is_ca {
            self.ca_certs.push(der);
            return Ok(self);
        }

        // insert and fetch index because rust and references unusable
        let index = self.end_certs.len();
        self.end_certs.push(der);

        // we will use this too lookup who signed data
        self.lookup_cert_by_issuer_and_serial_number
            .entry(cert_issuer_and_serial(&cert).into())
            .or_default()
            .push(index);

        // alternatively we could also be using this for lookup who signed the data
        if let Some(ext) = cert_skid(&cert) {
            self.lookup_cert_by_subject_key_identifier
                .entry(ext.into())
                .or_default()
                .push(index);
        }

        Ok(self)
    }

    /// Adds a certificate from the given x509 certificate.
    pub fn add_cert_x509(&mut self, cert: &x509::Certificate) -> Result<&mut Self, VerifyError> {
        let der = cert
            .to_der()
            .map_err(|err| VerifyError::DumpDERCert { err })?;
        self.add_cert_der(&der)
    }

    /// Adds certificates from the given signed data defined in [RFC 5652 Section 5.1].
    pub fn add_certs_from_signed_data(
        &mut self,
        signed_data: &cms::signed_data::SignedData,
    ) -> Result<&mut Self, VerifyError> {
        if let Some(certs) = &signed_data.certificates {
            for cert in certs.0.iter() {
                if let CertificateChoices::Certificate(cert_inner) = cert {
                    self.add_cert_x509(cert_inner)?;
                }
            }
        }
        Ok(self)
    }

    /// Adds a CRL from the given DER-encoded CRL data.
    pub fn add_crl_der(&mut self, data: &[u8]) -> Result<&mut Self, VerifyError> {
        let crl = webpki::OwnedCertRevocationList::from_der(data)
            .map_err(|err| VerifyError::ParseWebPKICrl { err })?;

        let time = cms::cert::x509::crl::CertificateList::from_der(data)
            .map_err(|err| VerifyError::ParseCMSCrl { err })?
            .tbs_cert_list
            .this_update
            .to_unix_duration();

        let hash: [u8; 32] = sha2::Sha256::digest(data).into();
        if !self.seen_crls.insert(hash) {
            return Ok(self);
        }

        self.crls.push((time, crl.into()));

        Ok(self)
    }

    /// Adds a CRL from the given x509 CRL.
    pub fn add_crl_x509(
        &mut self,
        crl: &x509::crl::CertificateList,
    ) -> Result<&mut Self, VerifyError> {
        let der = crl
            .to_der()
            .map_err(|err| VerifyError::DumpDERCrl { err })?;
        self.add_crl_der(&der)
    }

    /// Adds CRLs from the given signed data defined in [RFC 5652 Section 5.1].
    pub fn add_crls_from_signed_data(
        &mut self,
        signed_data: &cms::signed_data::SignedData,
    ) -> Result<&mut Self, VerifyError> {
        if let Some(crls) = &signed_data.crls {
            for crl in crls.0.iter() {
                if let RevocationInfoChoice::Crl(crl_inner) = crl {
                    self.add_crl_x509(crl_inner)?;
                }
            }
        }
        Ok(self)
    }

    /// Finalizes the builder and constructs the verification context.
    pub fn finalize(&self) -> VerifyContext<'_> {
        // webpki picks first matching crl only :( so we have to sort by update time (descending)
        let mut crls = self.crls.iter().collect::<Vec<_>>();
        crls.sort_by_key(|x| x.0);
        crls.reverse();

        VerifyContext {
            anchors: &self.anchors,
            ca_certs: &self.ca_certs,
            end_certs: self
                .end_certs
                .iter()
                .map(|der| {
                    (
                        // this is already parsed once we can't fail anymore
                        webpki::EndEntityCert::try_from(der).expect("Should not happen!"),
                        Default::default(),
                    )
                })
                .collect(),
            lookup_cert_by_issuer_and_serial_number: &self.lookup_cert_by_issuer_and_serial_number,
            lookup_cert_by_subject_key_identifier: &self.lookup_cert_by_subject_key_identifier,
            crls: crls.iter().map(|(_, crl)| crl).collect(),
            time: self.time.unwrap_or(UnixTime::now()),
        }
    }
}

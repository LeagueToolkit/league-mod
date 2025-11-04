use binrw::{BinRead, BinWrite, binrw};
use cms::{content_info::ContentInfo, signed_data::SignedData};
use der::{Decode, oid::ObjectIdentifier};
use std::{io::Cursor, path::Path, rc::Rc};
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum ModSigError {
    #[error("Failed to read file {err}")]
    ReadFileFailed { err: Rc<std::io::Error> },

    #[error("Failed to decode pem: {err}")]
    PemDecodeFailed { err: Rc<pem::PemError> },

    #[error("Failed to process pem tag: {tag}")]
    PemTagUnsupported { tag: String },

    #[error("Failed to extract mod sig entries got bad content type: {oid}")]
    DerModSigBadContentType { oid: ObjectIdentifier },

    #[error("Failed to extract mod sig entries got empty content")]
    DerModSigBadContentEmpty,

    #[error("Failed to decode signed data")]
    ContentInfoFailed { err: der::Error },

    #[error("Failed to decode signed data")]
    SignedDataFailed { err: der::Error },

    #[error("Failed to deserialize mod sig")]
    ModSigDecode { err: String },

    #[error("Failed to serialize mod sig")]
    ModSigEncode { err: String },
}

#[repr(C)]
#[binrw]
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ModSigEntry {
    pub name: u64,
    pub checksum_compressed: u64,
    pub checksum_uncompressed: u64,
}

#[repr(C)]
#[binrw]
#[brw(little, magic = 0x6769736c6f6c7363u64)]
#[derive(Default, Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ModSigEntryList {
    #[br(assert(version == 0, "Version must be 0"))]
    version: u32,

    #[bw(try_calc(u32::try_from(entries.len())))]
    entry_count: u32,

    #[br(count = entry_count)]
    #[brw(assert(entries.windows(2).all(|w| w[0].name < w[1].name)))]
    pub entries: Vec<ModSigEntry>,
}

impl ModSigEntryList {
    pub fn from_signed_data(signed_data: &SignedData) -> Result<Self, ModSigError> {
        let Some(content) = &signed_data.encap_content_info.econtent else {
            return Err(ModSigError::DerModSigBadContentEmpty);
        };
        Self::load(content.value())
    }

    pub fn is_sorted(&self) -> bool {
        self.entries.windows(2).all(|w| w[0].name < w[1].name)
    }

    pub fn dump(&self) -> Result<Vec<u8>, ModSigError> {
        let mut buffer = Cursor::new(Vec::new());
        self.write(&mut buffer)
            .map_err(|err| ModSigError::ModSigEncode {
                err: format!("{err}"),
            })?;
        Ok(buffer.into_inner())
    }

    pub fn load(data: &[u8]) -> Result<Self, ModSigError> {
        let mut cursor = Cursor::new(data);
        Self::read(&mut cursor).map_err(|err| ModSigError::ModSigDecode {
            err: format!("{err}"),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ModSig(SignedData, ModSigEntryList);

impl ModSig {
    pub fn signed_data(&self) -> &SignedData {
        &self.0
    }

    pub fn list(&self) -> &ModSigEntryList {
        &self.1
    }

    pub fn from_signed_data(signed_data: SignedData) -> Result<Self, ModSigError> {
        let entries = ModSigEntryList::from_signed_data(&signed_data)?;
        Ok(Self(signed_data, entries))
    }

    pub fn from_content_info(content_info: &ContentInfo) -> Result<Self, ModSigError> {
        let signed_data = content_info
            .content
            .decode_as::<SignedData>()
            .map_err(|err| ModSigError::SignedDataFailed { err })?;
        Self::from_signed_data(signed_data)
    }

    pub fn from_der(der: &[u8]) -> Result<Self, ModSigError> {
        let content_info =
            ContentInfo::from_der(der).map_err(|err| ModSigError::ContentInfoFailed { err })?;
        Self::from_content_info(&content_info)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ModSigBundle {
    pub sigs: Vec<ModSig>,
    pub crls: Vec<Box<[u8]>>,
    pub certs: Vec<Box<[u8]>>,
}

impl ModSigBundle {
    pub fn from_pem_str(s: &str) -> Result<Self, ModSigError> {
        let mut sigs = Vec::new();
        let mut crls = Vec::new();
        let mut certs = Vec::new();
        let pem =
            pem::parse_many(s).map_err(|err| ModSigError::PemDecodeFailed { err: Rc::new(err) })?;
        for pem_block in pem.into_iter() {
            match pem_block.tag() {
                "PRIVATE KEY" => {}
                "CMS" | "PKCS7" => sigs.push(ModSig::from_der(pem_block.contents())?),
                "X509 CRL" => crls.push(pem_block.contents().to_vec().into()),
                "CERTIFICATE" => certs.push(pem_block.contents().to_vec().into()),
                tag => {
                    return Err(ModSigError::PemTagUnsupported {
                        tag: tag.to_owned(),
                    });
                }
            }
        }
        Ok(Self { sigs, crls, certs })
    }

    pub fn from_pem_file_path<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Self, ModSigError> {
        let pem_data = std::fs::read_to_string(path)
            .map_err(|err| ModSigError::ReadFileFailed { err: Rc::new(err) })?;
        Self::from_pem_str(&pem_data)
    }
}

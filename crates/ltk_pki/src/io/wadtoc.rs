use std::path::Path;

use binrw::{BinRead, BinWrite, binrw};
use rsa::signature::hazmat::PrehashVerifier;
use sha2::{Digest, digest::Update};

use crate::consts;

static mut RITO_WAD_KEY: std::sync::atomic::AtomicPtr<
    rsa::pkcs8::spki::Result<rsa::pkcs1v15::VerifyingKey<sha2::Sha256>>,
> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

fn get_rito_wad_key() -> &'static rsa::pkcs8::spki::Result<rsa::pkcs1v15::VerifyingKey<sha2::Sha256>>
{
    #[allow(static_mut_refs)]
    // SAFETY: This function uses atomic operations to ensure that the key is initialized only once.
    // It is imperative that we do not use any locks, sycalls or TLS here, for that reason we use atomic operations.
    unsafe {
        // Fast path: if already initialized, return it.
        let current = RITO_WAD_KEY.load(std::sync::atomic::Ordering::Acquire);
        if !current.is_null() {
            return &*current;
        }

        let key = Box::new(
            <rsa::pkcs1v15::VerifyingKey<sha2::Sha256> as rsa::pkcs8::DecodePublicKey>::from_public_key_der(
                consts::RITO_PKEY,
            ),
        );
        let key_ptr = Box::into_raw(key);

        loop {
            match RITO_WAD_KEY.compare_exchange(
                std::ptr::null_mut(),
                key_ptr,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            ) {
                Ok(_) => {
                    return &*key_ptr;
                }
                Err(existing_ptr) => {
                    if existing_ptr.is_null() {
                        // Spurious failure; try again.
                        continue;
                    }
                    // Another thread set the key first; free our allocation and use theirs.
                    let _ = Box::from_raw(key_ptr);
                    return &*existing_ptr;
                }
            }
        }
    }
}

#[binrw]
#[brw(little)]
#[repr(C)]
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct WadTocEntry {
    pub name: u64,
    pub unused1: u64,
    pub unused2: u64,
    pub checksum: u64,
}

#[binrw]
#[brw(little, magic = 0x4035752u32)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct WadToc {
    pub signature: [u8; 256],

    pub checksum: [u8; 8],

    #[bw(try_calc(u32::try_from(entries.len())))]
    pub entry_count: u32,

    #[br(count = entry_count)]
    #[brw(assert(entries.windows(2).all(|w| w[0].name < w[1].name)))]
    pub entries: Vec<WadTocEntry>,
}

impl Default for WadToc {
    fn default() -> Self {
        Self {
            signature: [0u8; 256],
            checksum: Default::default(),
            entries: Default::default(),
        }
    }
}

impl WadToc {
    pub fn from_file_path<P: AsRef<Path> + ?Sized>(path: &P) -> binrw::BinResult<Self> {
        let file = std::fs::File::open(path)?;
        let mut data = binrw::io::BufReader::new(file);
        Self::read(&mut data)
    }

    pub fn checksum_sha256(&self) -> [u8; 32] {
        let mut hasher = sha2::Sha256::default();
        for entry in &self.entries {
            let mut buffer = [0u8; std::mem::size_of::<WadTocEntry>()];
            let mut cur = std::io::Cursor::new(&mut buffer[..]);
            let _ = entry.write_le(&mut cur);
            Update::update(&mut hasher, &buffer[..]);
        }
        hasher.finalize().into()
    }

    pub fn verify_rsa_pkcs1(&self) -> Result<[u8; 32], String> {
        let key = match get_rito_wad_key() {
            Ok(key) => key,
            Err(err) => return Err(format!("{}", err)),
        };
        let checksum = self.checksum_sha256();
        let signature = rsa::pkcs1v15::Signature::try_from(&self.signature[..])
            .map_err(|err| format!("{err}"))?;
        key.verify_prehash(&checksum, &signature)
            .map_err(|err| format!("{err}"))?;
        Ok(checksum)
    }

    pub fn is_sorted(&self) -> bool {
        self.entries.windows(2).all(|w| w[0].name < w[1].name)
    }
}

impl WadTocEntry {
    pub fn matches_wad(&self, other: &WadTocEntry) -> bool {
        self.name == other.name && self.checksum == other.checksum
    }
}

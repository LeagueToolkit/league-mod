use binrw::NullString;
use xxhash_rust::{xxh3, xxh64};

pub(crate) fn optional_string_len<'a>(string: impl Into<Option<&'a String>>) -> usize {
    string.into().as_ref().map(|n| n.len()).unwrap_or_default()
}

pub(crate) fn optional_string_write(s: &Option<String>) -> Option<Vec<u8>> {
    s.as_ref().map(|s| s.as_bytes().to_vec())
}
pub(crate) fn optional_string_read(
    s: Vec<u8>,
) -> Result<Option<String>, std::string::FromUtf8Error> {
    String::from_utf8(s).map(Some)
}

pub(crate) fn nullstr_read(s: NullString) -> Result<String, std::string::FromUtf8Error> {
    String::from_utf8(s.into())
}

pub(crate) fn nullstr_write<'a>(s: impl Into<&'a String>) -> NullString {
    NullString::from(s.into().as_str())
}

pub fn is_hex_chunk_name(file_name: &str) -> bool {
    file_name.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn sanitize_chunk_name(file_name: &str) -> &str {
    if let Some(stripped) = file_name.strip_prefix("0x") {
        return stripped;
    }

    file_name
}

/// Hash a layer name using xxhash3.
pub fn hash_layer_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}

/// Hash a chunk name using xxhash64.
pub fn hash_chunk_name(name: &str) -> u64 {
    xxh64::xxh64(name.to_lowercase().as_bytes(), 0)
}

/// Hash a wad name using xxhash3.
pub fn hash_wad_name(name: &str) -> u64 {
    xxh3::xxh3_64(name.to_lowercase().as_bytes())
}

#[cfg(test)]
pub mod test {
    use binrw::{meta, BinWrite};
    use std::io::Cursor;

    pub fn written_size<I>(item: &I, expected_size: usize)
    where
        for<'a> I: BinWrite<Args<'a> = ()> + meta::WriteEndian,
    {
        let mut buf = Cursor::new(Vec::with_capacity(expected_size + 64));
        item.write(&mut buf).unwrap();

        assert_eq!(
            expected_size,
            buf.into_inner().len(),
            "comparing reported size with real size"
        );
    }
}

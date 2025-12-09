use std::ffi::CStr;

pub fn cstr_to_str(cstr: *const u8) -> Option<String> {
    if cstr.is_null() {
        return None;
    }
    unsafe {
        Some(
            CStr::from_ptr(cstr as *const i8)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// Encode a Rust string to a C string (UTF-16)
pub fn str_to_cstr_utf16(s: &str) -> Box<[u16]> {
    s.encode_utf16().chain([0u16]).collect()
}
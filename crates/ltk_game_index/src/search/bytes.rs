//! Arrays of fixed-width integers as one byte string of little-endian elements.
//!
//! A field serialized through this module is one MessagePack `bin` value. A load of it is one
//! copy into the target vector. A format without byte strings reads a sequence of bytes.

use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserializer, Serializer};

use crate::DeclarationId;

/// A fixed-width element of a byte-string array.
pub(crate) trait LeBytes: Copy {
    /// Bytes per element.
    const SIZE: usize;
    /// Appends the element's little-endian bytes.
    fn put(self, out: &mut Vec<u8>);
    /// Every element of `bytes`, whose length is a multiple of `SIZE`.
    fn take_all(bytes: Vec<u8>) -> Vec<Self>;
}

macro_rules! le_bytes {
    ($($ty:ty),*) => {$(
        impl LeBytes for $ty {
            const SIZE: usize = size_of::<$ty>();
            fn put(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn take_all(bytes: Vec<u8>) -> Vec<Self> {
                let (elements, _) = bytes.as_chunks::<{ size_of::<$ty>() }>();
                elements.iter().map(|element| <$ty>::from_le_bytes(*element)).collect()
            }
        }
    )*};
}

le_bytes!(u32, u64);

impl LeBytes for u8 {
    const SIZE: usize = 1;
    fn put(self, out: &mut Vec<u8>) {
        out.push(self);
    }
    fn take_all(bytes: Vec<u8>) -> Vec<Self> {
        bytes
    }
}

impl LeBytes for DeclarationId {
    const SIZE: usize = 4;
    fn put(self, out: &mut Vec<u8>) {
        u32::from(self).put(out);
    }
    fn take_all(bytes: Vec<u8>) -> Vec<Self> {
        u32::take_all(bytes)
            .into_iter()
            .map(DeclarationId::new)
            .collect()
    }
}

/// Writes `values` as one byte string.
pub(crate) fn serialize<T: LeBytes, S: Serializer>(
    values: &[T],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut out = Vec::with_capacity(values.len() * T::SIZE);
    for &value in values {
        value.put(&mut out);
    }
    serializer.serialize_bytes(&out)
}

/// Reads a byte string written by [`serialize`].
pub(crate) fn deserialize<'de, T: LeBytes, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    deserializer.deserialize_byte_buf(ArrayVisitor(PhantomData))
}

struct ArrayVisitor<T>(PhantomData<T>);

impl<T: LeBytes> ArrayVisitor<T> {
    fn decode<E: de::Error>(bytes: Vec<u8>) -> Result<Vec<T>, E> {
        if !bytes.len().is_multiple_of(T::SIZE) {
            return Err(E::invalid_length(
                bytes.len(),
                &"a multiple of the element width",
            ));
        }
        Ok(T::take_all(bytes))
    }
}

impl<'de, T: LeBytes> Visitor<'de> for ArrayVisitor<T> {
    type Value = Vec<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a byte string of {}-byte little-endian elements",
            T::SIZE
        )
    }

    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
        Self::decode(bytes.to_vec())
    }

    fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<Self::Value, E> {
        Self::decode(bytes)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(byte) = seq.next_element::<u8>()? {
            bytes.push(byte);
        }
        Self::decode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Arrays {
        #[serde(with = "super")]
        small: Vec<u8>,
        #[serde(with = "super")]
        ids: Vec<u32>,
        #[serde(with = "super")]
        wide: Vec<u64>,
    }

    #[test]
    fn arrays_round_trip_as_byte_strings() {
        let arrays = Arrays {
            small: vec![1, 2, 255],
            ids: vec![0, 7, u32::MAX],
            wide: vec![u64::MAX, 0x0102_0304_0506_0708],
        };
        let bytes = rmp_serde::to_vec(&arrays).unwrap();
        // `bin8` marker for the 12 bytes of `ids`, then the little-endian elements.
        let ids = [0xc4, 12, 0, 0, 0, 0, 7, 0, 0, 0, 0xff, 0xff, 0xff, 0xff];
        assert!(bytes.windows(ids.len()).any(|window| window == ids));
        assert_eq!(rmp_serde::from_slice::<Arrays>(&bytes).unwrap(), arrays);
    }

    #[test]
    fn a_ragged_byte_string_does_not_load() {
        #[derive(Serialize)]
        struct Ragged {
            #[serde(with = "serde_bytes_shim")]
            small: Vec<u8>,
            #[serde(with = "serde_bytes_shim")]
            ids: Vec<u8>,
            #[serde(with = "serde_bytes_shim")]
            wide: Vec<u8>,
        }
        mod serde_bytes_shim {
            pub fn serialize<S: serde::Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(v)
            }
        }
        let bytes = rmp_serde::to_vec(&Ragged {
            small: vec![1],
            ids: vec![1, 2, 3],
            wide: vec![],
        })
        .unwrap();
        assert!(rmp_serde::from_slice::<Arrays>(&bytes).is_err());
    }
}

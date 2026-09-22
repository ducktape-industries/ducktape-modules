//! How a typed key becomes bytes: integers big-endian and fixed width (scan
//! order is numeric order), bytes and strings length-prefixed, tuples
//! concatenated (a prefix scan on the leading elements works), fixed arrays
//! as they are.

pub trait KeyCodec: Sized {
    fn encode_key(&self, out: &mut Vec<u8>);
    /// Reads one key from the front of `bytes`, leaving the rest.
    fn decode_key(bytes: &mut &[u8]) -> Option<Self>;

    fn key_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_key(&mut out);
        out
    }
}

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    let (head, rest) = bytes.split_at_checked(len)?;
    *bytes = rest;
    Some(head)
}

macro_rules! integers {
    ($($t:ty),*) => {$(
        impl KeyCodec for $t {
            fn encode_key(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_be_bytes());
            }
            fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
                take(bytes, size_of::<$t>()).map(|b| <$t>::from_be_bytes(b.try_into().unwrap()))
            }
        }
    )*};
}
integers!(u8, u16, u32, u64, u128);

fn put_len(out: &mut Vec<u8>, len: usize) {
    out.extend_from_slice(&(len as u32).to_le_bytes());
}

fn get_len(bytes: &mut &[u8]) -> Option<usize> {
    u32::from_le_bytes(take(bytes, 4)?.try_into().ok()?)
        .try_into()
        .ok()
}

impl KeyCodec for Vec<u8> {
    fn encode_key(&self, out: &mut Vec<u8>) {
        put_len(out, self.len());
        out.extend_from_slice(self);
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        let len = get_len(bytes)?;
        take(bytes, len).map(<[u8]>::to_vec)
    }
}

impl KeyCodec for String {
    fn encode_key(&self, out: &mut Vec<u8>) {
        put_len(out, self.len());
        out.extend_from_slice(self.as_bytes());
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        let len = get_len(bytes)?;
        String::from_utf8(take(bytes, len)?.to_vec()).ok()
    }
}

impl<const N: usize> KeyCodec for [u8; N] {
    fn encode_key(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        take(bytes, N)?.try_into().ok()
    }
}

impl KeyCodec for () {
    fn encode_key(&self, _out: &mut Vec<u8>) {}
    fn decode_key(_bytes: &mut &[u8]) -> Option<Self> {
        Some(())
    }
}

macro_rules! tuples {
    ($(($($n:tt $t:ident),+))*) => {$(
        impl<$($t: KeyCodec),+> KeyCodec for ($($t,)+) {
            fn encode_key(&self, out: &mut Vec<u8>) {
                $(self.$n.encode_key(out);)+
            }
            fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
                Some(($($t::decode_key(bytes)?,)+))
            }
        }
    )*};
}
tuples!((0 A) (0 A, 1 B) (0 A, 1 B, 2 C) (0 A, 1 B, 2 C, 3 D));

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<K: KeyCodec + PartialEq + std::fmt::Debug>(key: K) -> Vec<u8> {
        let bytes = key.key_bytes();
        let mut rest = bytes.as_slice();
        assert_eq!(K::decode_key(&mut rest), Some(key));
        assert!(rest.is_empty());
        bytes
    }

    #[test]
    fn integers_sort_numerically_and_tuples_scan_by_their_head() {
        assert!(round_trip(2u64) < round_trip(10u64));
        let head = 7u64.key_bytes();
        assert!(round_trip((7u64, "b".to_string())).starts_with(&head));
        assert!(round_trip((7u64, vec![1u8, 2], [9u8; 3])).starts_with(&head));
        assert!(round_trip((1u8, 2u16, 3u32, "s".to_string())).len() == 1 + 2 + 4 + 4 + 1);
        assert_eq!(u64::decode_key(&mut &[1u8, 2][..]), None);
        assert_eq!(String::decode_key(&mut &[9u8, 0, 0, 0, b'a'][..]), None);
    }
}

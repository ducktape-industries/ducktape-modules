// pkt-line framing: encode data and special packets, and a Reader that tracks bytes consumed.

use crate::error::{Error, Result};

pub const MAX_PKT: usize = 65520;
pub const MAX_DATA: usize = MAX_PKT - 4;

const HEX: &[u8; 16] = b"0123456789abcdef";

pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 4);
    push(&mut out, data);
    out
}

pub fn push(out: &mut Vec<u8>, data: &[u8]) {
    let len = data.len() + 4;
    out.push(HEX[(len >> 12) & 0xf]);
    out.push(HEX[(len >> 8) & 0xf]);
    out.push(HEX[(len >> 4) & 0xf]);
    out.push(HEX[len & 0xf]);
    out.extend_from_slice(data);
}

pub fn push_line(out: &mut Vec<u8>, text: &[u8]) {
    let mut line = Vec::with_capacity(text.len() + 1);
    line.extend_from_slice(text);
    line.push(b'\n');
    push(out, &line);
}

pub fn flush() -> &'static [u8] {
    b"0000"
}

pub fn delim() -> &'static [u8] {
    b"0001"
}

pub fn response_end() -> &'static [u8] {
    b"0002"
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pkt<'a> {
    Data(&'a [u8]),
    Flush,
    Delim,
    ResponseEnd,
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, pos: 0 }
    }

    pub fn consumed(&self) -> usize {
        self.pos
    }

    pub fn rest(&self) -> &'a [u8] {
        &self.bytes[self.pos..]
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn read_pkt(&mut self) -> Result<Pkt<'a>> {
        let header_available = self.pos + 4 <= self.bytes.len();
        if !header_available {
            return Err(Error::Truncated);
        }
        let len = parse_length(&self.bytes[self.pos..self.pos + 4])?;
        match len {
            0 => {
                self.pos += 4;
                Ok(Pkt::Flush)
            }
            1 => {
                self.pos += 4;
                Ok(Pkt::Delim)
            }
            2 => {
                self.pos += 4;
                Ok(Pkt::ResponseEnd)
            }
            3 => Err(Error::BadPktLine),
            _ => {
                let too_long = len > MAX_PKT;
                if too_long {
                    return Err(Error::BadPktLine);
                }
                let end = self.pos + len;
                let available = end <= self.bytes.len();
                if !available {
                    return Err(Error::Truncated);
                }
                let data = &self.bytes[self.pos + 4..end];
                self.pos = end;
                Ok(Pkt::Data(data))
            }
        }
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = Result<Pkt<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.at_end() {
            return None;
        }
        Some(self.read_pkt())
    }
}

fn parse_length(hex: &[u8]) -> Result<usize> {
    let mut len = 0;
    for byte in hex {
        let nibble = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return Err(Error::BadPktLine),
        };
        len = (len << 4) | usize::from(nibble);
    }
    Ok(len)
}

pub fn strip_newline(data: &[u8]) -> &[u8] {
    match data.split_last() {
        Some((b'\n', rest)) => rest,
        _ => data,
    }
}

pub(crate) fn read_data<'a>(reader: &mut Reader<'a>) -> Result<Option<&'a [u8]>> {
    match reader.next() {
        None => Ok(None),
        Some(Ok(Pkt::Data(data))) => Ok(Some(strip_newline(data))),
        Some(Ok(_)) => Ok(None),
        Some(Err(error)) => Err(error),
    }
}

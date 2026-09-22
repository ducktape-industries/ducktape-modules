// Sideband-64k multiplexing: band 1 carries pack data, 2 progress, 3 errors.

use super::pktline;

pub const MAX_CHUNK: usize = pktline::MAX_DATA - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Pack = 1,
    Progress = 2,
    Error = 3,
}

pub fn encode(band: Band, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 5);
    push(&mut out, band, data);
    out
}

pub fn push(out: &mut Vec<u8>, band: Band, data: &[u8]) {
    let mut chunks = data.chunks(MAX_CHUNK).peekable();
    let empty = chunks.peek().is_none();
    if empty {
        return;
    }
    for chunk in chunks {
        let mut packet = Vec::with_capacity(chunk.len() + 1);
        packet.push(band as u8);
        packet.extend_from_slice(chunk);
        pktline::push(out, &packet);
    }
}

pub struct Writer<'a> {
    band: Band,
    buffer: Vec<u8>,
    sink: &'a mut dyn FnMut(&[u8]),
}

impl<'a> Writer<'a> {
    pub fn new(band: Band, sink: &'a mut dyn FnMut(&[u8])) -> Writer<'a> {
        Writer {
            band,
            buffer: Vec::with_capacity(MAX_CHUNK),
            sink,
        }
    }

    pub fn write(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let room = MAX_CHUNK - self.buffer.len();
            let take = room.min(data.len());
            self.buffer.extend_from_slice(&data[..take]);
            data = &data[take..];
            let full = self.buffer.len() == MAX_CHUNK;
            if full {
                self.emit();
            }
        }
    }

    pub fn finish(mut self) {
        self.emit();
    }

    fn emit(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let packet = encode(self.band, &self.buffer);
        (self.sink)(&packet);
        self.buffer.clear();
    }
}

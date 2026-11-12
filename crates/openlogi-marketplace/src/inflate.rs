//! A small DEFLATE (RFC 1951) decoder for `.lplug4` entries.
//!
//! Plugin packages are ordinary zip files, and every entry worth reading is
//! either stored or deflated. Decoding here keeps the package reader free of
//! dependencies and lets it enforce an output bound while it decodes, so a
//! hostile package cannot expand past the size its directory declared.

use crate::package_error::PackageError;

/// Longest code length DEFLATE allows.
const MAX_BITS: usize = 15;
/// Size of the literal/length alphabet, including the two unused codes.
const LITERAL_CODES: usize = 288;
/// Size of the distance alphabet, including the two unused codes.
const DISTANCE_CODES: usize = 30;

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// Order in which a dynamic block transmits its code-length code lengths.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Inflate a raw DEFLATE stream, producing at most `limit` bytes.
///
/// Fails rather than truncates when the stream is malformed, ends early, or
/// would produce more than `limit` bytes.
pub(crate) fn inflate(input: &[u8], limit: usize) -> Result<Vec<u8>, PackageError> {
    let mut reader = BitReader::new(input);
    let mut out = Vec::new();
    loop {
        let last = reader.bits(1)? == 1;
        match reader.bits(2)? {
            0 => stored_block(&mut reader, &mut out, limit)?,
            1 => {
                let (literals, distances) = fixed_tables()?;
                compressed_block(&mut reader, &mut out, limit, &literals, &distances)?;
            }
            2 => {
                let (literals, distances) = dynamic_tables(&mut reader)?;
                compressed_block(&mut reader, &mut out, limit, &literals, &distances)?;
            }
            _ => return Err(corrupt("reserved block type")),
        }
        if last {
            return Ok(out);
        }
    }
}

fn corrupt(reason: &'static str) -> PackageError {
    PackageError::CorruptDeflate { reason }
}

/// LSB-first bit cursor over the compressed bytes.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buffer: u32,
    count: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buffer: 0,
            count: 0,
        }
    }

    /// Read `need` bits (at most 16), least significant first.
    fn bits(&mut self, need: u32) -> Result<u32, PackageError> {
        while self.count < need {
            let byte = *self
                .data
                .get(self.pos)
                .ok_or_else(|| corrupt("stream ends mid-block"))?;
            self.pos += 1;
            self.buffer |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let value = self.buffer & ((1u32 << need) - 1);
        self.buffer >>= need;
        self.count -= need;
        Ok(value)
    }

    /// Drop the bits left in the current byte and take `len` whole bytes.
    fn aligned_bytes(&mut self, len: usize) -> Result<&'a [u8], PackageError> {
        self.buffer = 0;
        self.count = 0;
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| corrupt("stored block runs past the stream"))?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }
}

/// A canonical Huffman code: how many codes exist per length, and the symbols
/// in code order.
struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, PackageError> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &len in lengths {
            let slot = counts
                .get_mut(usize::from(len))
                .ok_or_else(|| corrupt("code length above 15"))?;
            *slot += 1;
        }
        // More codes of some length than the prefix space leaves room for.
        let mut left: i32 = 1;
        for &count in &counts[1..] {
            left = (left << 1) - i32::from(count);
            if left < 0 {
                return Err(corrupt("over-subscribed Huffman code"));
            }
        }
        let mut offsets = [0u16; MAX_BITS + 1];
        for len in 1..MAX_BITS {
            offsets[len + 1] = offsets[len] + counts[len];
        }
        let coded = lengths.iter().filter(|len| **len != 0).count();
        let mut symbols = vec![0u16; coded];
        for (symbol, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let offset = &mut offsets[usize::from(len)];
            let symbol = u16::try_from(symbol).map_err(|_| corrupt("alphabet too large"))?;
            symbols[usize::from(*offset)] = symbol;
            *offset += 1;
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16, PackageError> {
        let mut code: u32 = 0;
        let mut first: u32 = 0;
        let mut index: u32 = 0;
        for &count in &self.counts[1..] {
            code |= reader.bits(1)?;
            let count = u32::from(count);
            if code < first + count {
                let slot = usize::try_from(index + (code - first))
                    .map_err(|_| corrupt("symbol index out of range"))?;
                return self
                    .symbols
                    .get(slot)
                    .copied()
                    .ok_or_else(|| corrupt("symbol index out of range"));
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(corrupt("code not present in the Huffman table"))
    }
}

fn fixed_tables() -> Result<(Huffman, Huffman), PackageError> {
    let mut lengths = [0u8; LITERAL_CODES];
    for (symbol, len) in lengths.iter_mut().enumerate() {
        *len = match symbol {
            0..=143 | 280..=287 => 8,
            144..=255 => 9,
            _ => 7,
        };
    }
    Ok((
        Huffman::new(&lengths)?,
        Huffman::new(&[5u8; DISTANCE_CODES])?,
    ))
}

fn dynamic_tables(reader: &mut BitReader<'_>) -> Result<(Huffman, Huffman), PackageError> {
    let literal_count = reader.bits(5)? as usize + 257;
    let distance_count = reader.bits(5)? as usize + 1;
    let code_length_count = reader.bits(4)? as usize + 4;
    if literal_count > LITERAL_CODES - 2 || distance_count > DISTANCE_CODES {
        return Err(corrupt("too many codes in a dynamic block"));
    }

    let mut code_lengths = [0u8; 19];
    for &slot in &CODE_LENGTH_ORDER[..code_length_count] {
        code_lengths[slot] = three_bits(reader)?;
    }
    let code_length_code = Huffman::new(&code_lengths)?;

    let total = literal_count + distance_count;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        let symbol = code_length_code.decode(reader)?;
        let (value, repeat) = match symbol {
            0..=15 => (
                u8::try_from(symbol).map_err(|_| corrupt("bad code length"))?,
                1,
            ),
            16 => {
                let previous = *lengths
                    .last()
                    .ok_or_else(|| corrupt("repeat with no previous length"))?;
                (previous, reader.bits(2)? as usize + 3)
            }
            17 => (0, reader.bits(3)? as usize + 3),
            18 => (0, reader.bits(7)? as usize + 11),
            _ => return Err(corrupt("bad code-length symbol")),
        };
        if lengths.len() + repeat > total {
            return Err(corrupt("code lengths overrun the block header"));
        }
        lengths.extend(std::iter::repeat_n(value, repeat));
    }
    if lengths[256] == 0 {
        return Err(corrupt("block has no end-of-block code"));
    }
    let (literal_lengths, distance_lengths) = lengths.split_at(literal_count);
    Ok((
        Huffman::new(literal_lengths)?,
        Huffman::new(distance_lengths)?,
    ))
}

fn three_bits(reader: &mut BitReader<'_>) -> Result<u8, PackageError> {
    u8::try_from(reader.bits(3)?).map_err(|_| corrupt("bad code length"))
}

fn stored_block(
    reader: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), PackageError> {
    let header = reader.aligned_bytes(4)?;
    let len = u16::from_le_bytes([header[0], header[1]]);
    let complement = u16::from_le_bytes([header[2], header[3]]);
    if len != !complement {
        return Err(corrupt("stored block length check failed"));
    }
    let bytes = reader.aligned_bytes(usize::from(len))?;
    if out.len() + bytes.len() > limit {
        return Err(PackageError::TooLarge { limit });
    }
    out.extend_from_slice(bytes);
    Ok(())
}

fn compressed_block(
    reader: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    limit: usize,
    literals: &Huffman,
    distances: &Huffman,
) -> Result<(), PackageError> {
    loop {
        let symbol = usize::from(literals.decode(reader)?);
        if symbol < 256 {
            if out.len() >= limit {
                return Err(PackageError::TooLarge { limit });
            }
            out.push(u8::try_from(symbol).map_err(|_| corrupt("bad literal"))?);
            continue;
        }
        if symbol == 256 {
            return Ok(());
        }
        let slot = symbol - 257;
        let (Some(&base), Some(&extra)) = (LENGTH_BASE.get(slot), LENGTH_EXTRA.get(slot)) else {
            return Err(corrupt("bad length symbol"));
        };
        let len = usize::from(base) + reader.bits(u32::from(extra))? as usize;

        let slot = usize::from(distances.decode(reader)?);
        let (Some(&base), Some(&extra)) = (DISTANCE_BASE.get(slot), DISTANCE_EXTRA.get(slot))
        else {
            return Err(corrupt("bad distance symbol"));
        };
        let distance = usize::from(base) + reader.bits(u32::from(extra))? as usize;

        if distance > out.len() {
            return Err(corrupt("distance reaches before the start of the output"));
        }
        if out.len() + len > limit {
            return Err(PackageError::TooLarge { limit });
        }
        // Byte by byte: a match may overlap the bytes it is producing.
        let start = out.len() - distance;
        for offset in 0..len {
            out.push(out[start + offset]);
        }
    }
}

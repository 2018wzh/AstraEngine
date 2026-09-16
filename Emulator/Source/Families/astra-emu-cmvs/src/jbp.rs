mod entry;
pub(crate) use entry::decode_jbp;
mod color;
use color::*;

use astra_emu_sdk::CoreError;
use std::ops::Range;

use crate::pb::PbDecodedImage;

const MAX_PIXELS: usize = 64 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;
const JBP_HEADER_BYTES: usize = 0x24;
const JBP_TABLE_BYTES: usize = 0x110;
const ZIGZAG_ORDER: [usize; 64] = [
    1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 0,
];

/// Decodes the shared JBP payload used by the documented PB2/PB3 variants.
///
/// Container headers own the JBP and alpha offsets.  This function deliberately
/// does not interpret a PB version or subtype: doing so here would make a
/// validated PB2/PB3 boundary indistinguishable from a guessed one.
#[derive(Debug, Clone)]
pub(crate) struct JbpBounds {
    pub payload_end: usize,
    pub alpha: Option<Range<usize>>,
}

pub(crate) fn decode_pb3_jbp(
    source: &[u8],
    width: u16,
    height: u16,
    bits_per_pixel: u16,
    payload_end: usize,
) -> Result<PbDecodedImage, CoreError> {
    let alpha_offset = read_u32(source, 0x2c)?;
    let alpha_size = read_u32(source, 0x30)?;
    let alpha_offset = if bits_per_pixel == 32 && alpha_offset != 0 {
        Some(alpha_offset)
    } else {
        None
    };
    let alpha_end = if let Some(alpha_offset) = alpha_offset {
        alpha_offset
            .checked_add(alpha_size)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_LAYOUT", "JBP alpha range overflowed"))?
    } else {
        payload_end
    };
    decode_jbp(
        source,
        0x34,
        width,
        height,
        bits_per_pixel,
        JbpBounds {
            payload_end,
            alpha: alpha_offset.map(|offset| offset..alpha_end),
        },
    )
}

struct JbpDecoder<'a> {
    source: &'a [u8],
    data_offset: usize,
    format: u32,
    aligned_height: usize,
    stride: usize,
    blocks_x: usize,
    blocks_y: usize,
    bitstream_bytes: usize,
}

impl<'a> JbpDecoder<'a> {
    fn new(source: &'a [u8], offset: usize, bitstream_end: usize) -> Result<Self, CoreError> {
        checked_range(
            source,
            offset,
            JBP_HEADER_BYTES,
            "ASTRA_EMU_CMVS_JBP_HEADER",
        )?;
        let data_offset = offset
            .checked_add(read_u32(source, offset + 4)?)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_LAYOUT", "JBP table offset overflowed"))?;
        checked_range(
            source,
            data_offset,
            JBP_TABLE_BYTES,
            "ASTRA_EMU_CMVS_JBP_TABLE",
        )?;
        let format = u32::try_from(read_u32(source, offset + 8)?)
            .map_err(|_| invalid("ASTRA_EMU_CMVS_JBP_FORMAT", "JBP format exceeds bounds"))?;
        let width = usize::from(read_u16(source, offset + 0x10)?);
        let height = usize::from(read_u16(source, offset + 0x12)?);
        if width == 0 || height == 0 {
            return Err(invalid(
                "ASTRA_EMU_CMVS_JBP_DIMENSIONS",
                "JBP dimensions are zero",
            ));
        }
        let (aligned_width, aligned_height) = match (format >> 28) & 3 {
            0 => (align(width, 8)?, align(height, 8)?),
            1 => (align(width, 16)?, align(height, 16)?),
            2 => (align(width, 32)?, align(height, 16)?),
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_CMVS_JBP_FORMAT",
                    "JBP sampling layout is unsupported",
                ));
            }
        };
        let pixels = aligned_width.checked_mul(aligned_height).ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_JBP_DIMENSIONS",
                "JBP aligned pixel count overflowed",
            )
        })?;
        if pixels > MAX_PIXELS {
            return Err(invalid(
                "ASTRA_EMU_CMVS_JBP_DIMENSIONS",
                "JBP aligned pixel count exceeds the decoder budget",
            ));
        }
        let stride = aligned_width
            .checked_mul(4)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_OUTPUT", "JBP stride overflowed"))?;
        if stride
            .checked_mul(aligned_height)
            .is_none_or(|size| size > MAX_DECODED_BYTES)
        {
            return Err(invalid(
                "ASTRA_EMU_CMVS_JBP_OUTPUT",
                "JBP output exceeds the decoder budget",
            ));
        }
        // The declared DC/AC segment sizes (offsets 0x1c/0x20) only size the
        // estimated split; the decode uses one continuous bit stream.
        let _declared_dc_bytes = read_u32(source, offset + 0x1c)?;
        let _declared_ac_bytes = read_u32(source, offset + 0x20)?;
        let bits_offset = data_offset + JBP_TABLE_BYTES;
        if bitstream_end > source.len() {
            return Err(invalid(
                "ASTRA_EMU_CMVS_JBP_BITS",
                "JBP bit stream boundaries are invalid",
            ));
        }
        checked_range(
            source,
            bits_offset,
            bitstream_end - bits_offset,
            "ASTRA_EMU_CMVS_JBP_BITS",
        )?;
        Ok(Self {
            source,
            data_offset,
            format,
            aligned_height,
            stride,
            blocks_x: aligned_width / 16,
            blocks_y: aligned_height / 16,
            bitstream_bytes: bitstream_end - bits_offset,
        })
    }

    fn decode(&mut self) -> Result<Vec<u8>, CoreError> {
        let tree_data_offset = self.data_offset + 0x80;
        let mut base = [0_u8; 16];
        for (index, value) in base.iter_mut().enumerate() {
            *value = self.source[tree_data_offset + index]
                .checked_add(1)
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_JBP_TREE",
                        "JBP Huffman code length overflowed",
                    )
                })?;
        }
        let tree_dc_freq = read_frequencies(self.source, self.data_offset)?;
        let tree_dc = HuffmanTree::new(base, tree_dc_freq)?;
        let tree_ac = HuffmanTree::new(
            base,
            read_frequencies(self.source, self.data_offset + 0x40)?,
        )?;
        let quant_offset = tree_data_offset + 0x10;
        let mut quant_y = [0_i16; 64];
        let mut quant_c = [0_i16; 64];
        if self.format & 0x0800_0000 != 0 {
            for index in 0..64 {
                quant_y[index] = i16::from(self.source[quant_offset + index]);
                quant_c[index] = i16::from(self.source[quant_offset + 0x40 + index]);
            }
        } else {
            return Err(invalid(
                "ASTRA_EMU_CMVS_JBP_QUANTIZATION",
                "JBP stream does not declare its quantization tables",
            ));
        }
        let bits_offset = quant_offset + 0x80;
        let mut bits_dc = JBitStream::new(
            self.source,
            bits_offset,
            self.bitstream_bytes,
            "ASTRA_EMU_CMVS_JBP_DC_BITS",
        )?;
        // The DC and AC coefficient streams share one continuous bit
        // stream: AC resumes from wherever the DC pass stops rather than
        // from a fixed offset, so a DC pass that legitimately runs past its
        // declared size simply pushes the AC start deeper into the file.
        let mut bits_ac = JBitStream::new(
            self.source,
            bits_dc.position,
            self.source.len() - bits_dc.position,
            "ASTRA_EMU_CMVS_JBP_AC_BITS",
        )?;
        let total_blocks = self
            .blocks_x
            .checked_mul(self.blocks_y)
            .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_BLOCKS", "JBP block count overflowed"))?;
        let mut blocks = vec![[0_i16; 6]; total_blocks];
        let mut previous = 0_u32;
        for block in &mut blocks {
            for value in block {
                let bit_count = usize::from(tree_dc.read(&mut bits_dc)?);
                if bit_count == 0 || bit_count > 16 {
                    return Err(invalid(
                        "ASTRA_EMU_CMVS_JBP_DC",
                        "JBP DC code length is invalid",
                    ));
                }
                let mut decoded = bits_dc.get_bits(bit_count)?;
                if decoded < (1_u32 << (bit_count - 1)) {
                    decoded = decoded.wrapping_sub((1_u32 << bit_count) - 1);
                }
                previous = previous.wrapping_add(decoded);
                *value = (previous as u16) as i16;
            }
        }
        let mut output = vec![0_u8; self.stride * self.aligned_height];
        for block_y in 0..self.blocks_y {
            let mut dst1 = block_y * self.stride * 16;
            let mut dst2 = dst1 + self.stride * 9;
            for block_x in 0..self.blocks_x {
                let mut table = [[0_i16; 64]; 6];
                for plane in 0..6 {
                    table[plane][0] = blocks[block_y * self.blocks_x + block_x][plane];
                    let mut coefficient = 0_usize;
                    while coefficient < 63 {
                        // GARbro ImageJBP UnpackAC: the tree leaf is the
                        // stored byte plus one; the raw byte is the JPEG
                        // style compound code (run of zeros << 4 | size).
                        let raw = tree_ac.read(&mut bits_ac)?.checked_sub(1).ok_or_else(|| {
                            invalid("ASTRA_EMU_CMVS_JBP_AC", "JBP AC code is invalid")
                        })?;
                        if raw == 0 {
                            break;
                        }
                        let run = (raw >> 4) as usize;
                        let size = (raw & 0xF) as usize;
                        coefficient += run;
                        if coefficient > 63 {
                            return Err(invalid(
                                "ASTRA_EMU_CMVS_JBP_AC",
                                "JBP AC run exceeds its block",
                            ));
                        }
                        if size == 0 {
                            continue;
                        }
                        let mut value = bits_ac.get_bits(size)?;
                        if value < (1_u32 << (size - 1)) {
                            value = value.wrapping_sub((1_u32 << size) - 1);
                        }
                        table[plane][ZIGZAG_ORDER[coefficient]] = (value as u16) as i16;
                        coefficient += 1;
                    }
                }
                for plane in table.iter_mut().take(4) {
                    inverse_dct(plane, &quant_y)?;
                }
                inverse_dct(&mut table[4], &quant_c)?;
                inverse_dct(&mut table[5], &quant_c)?;
                ycc_to_bgr(
                    &mut output,
                    self.stride,
                    dst1,
                    dst1 + self.stride,
                    [&table[0], &table[4], &table[5]],
                    0,
                )?;
                ycc_to_bgr(
                    &mut output,
                    self.stride,
                    dst1 + 32,
                    dst1 + self.stride + 32,
                    [&table[1], &table[4], &table[5]],
                    4,
                )?;
                ycc_to_bgr(
                    &mut output,
                    self.stride,
                    dst2 - self.stride,
                    dst2,
                    [&table[2], &table[4], &table[5]],
                    32,
                )?;
                ycc_to_bgr(
                    &mut output,
                    self.stride,
                    dst2 - self.stride + 32,
                    dst2 + 32,
                    [&table[3], &table[4], &table[5]],
                    36,
                )?;
                dst1 += 64;
                dst2 += 64;
            }
        }
        Ok(output)
    }
}

struct HuffmanTree {
    base: [u8; 16],
    nodes: [[usize; 2]; 31],
    root: usize,
}

impl HuffmanTree {
    fn new(base: [u8; 16], mut frequency: [u32; 31]) -> Result<Self, CoreError> {
        let mut nodes = [[0_usize; 2]; 31];
        let mut depth = base.len();
        while depth < nodes.len() {
            let left = select_minimum(&frequency, depth, None)?;
            let right = select_minimum(&frequency, depth, Some(left))?;
            nodes[depth] = [left, right];
            frequency[depth] = frequency[left]
                .checked_add(frequency[right])
                .ok_or_else(|| {
                    invalid(
                        "ASTRA_EMU_CMVS_JBP_TREE",
                        "JBP Huffman frequency overflowed",
                    )
                })?;
            frequency[left] = u32::MAX;
            frequency[right] = u32::MAX;
            depth += 1;
        }
        Ok(Self {
            base,
            nodes,
            root: depth - 1,
        })
    }

    fn read(&self, bits: &mut JBitStream<'_>) -> Result<u8, CoreError> {
        let mut node = self.root;
        while node >= self.base.len() {
            let branch = usize::from(bits.next_bit()?);
            node = self.nodes[node][branch];
        }
        Ok(self.base[node])
    }
}

fn select_minimum(
    frequency: &[u32; 31],
    depth: usize,
    excluded: Option<usize>,
) -> Result<usize, CoreError> {
    let mut result = None;
    let mut minimum = u32::MAX - 1;
    for (index, value) in frequency.iter().take(depth).enumerate() {
        if Some(index) != excluded && *value < minimum {
            minimum = *value;
            result = Some(index);
        }
    }
    result.ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_TREE", "JBP Huffman tree is incomplete"))
}

struct JBitStream<'a> {
    source: &'a [u8],
    position: usize,
    end: usize,
    bits: u32,
    available: usize,
    code: &'static str,
}

impl<'a> JBitStream<'a> {
    fn new(
        source: &'a [u8],
        offset: usize,
        length: usize,
        code: &'static str,
    ) -> Result<Self, CoreError> {
        checked_range(source, offset, length, code)?;
        Ok(Self {
            source,
            position: offset,
            end: offset + length,
            bits: 0,
            available: 0,
            code,
        })
    }

    fn get_bits(&mut self, count: usize) -> Result<u32, CoreError> {
        if count > 24 {
            return Err(invalid(self.code, "JBP bit request exceeds bounds"));
        }
        while self.available < count {
            // GARbro's JBP reader treats the whole file tail as the bit
            // stream: the nominal segment length only sizes the DC/AC split,
            // and the AC coefficients legitimately read past it. The file
            // boundary is the only hard limit.
            let byte = *self
                .source
                .get(self.position)
                .ok_or_else(|| invalid(self.code, "JBP bit stream is truncated"))?;
            let _ = self.end;
            self.position += 1;
            self.bits = (self.bits << 8) | u32::from(byte);
            self.available += 8;
        }
        self.available -= count;
        let mask = if count == 0 { 0 } else { (1_u32 << count) - 1 };
        Ok((self.bits >> self.available) & mask)
    }

    fn next_bit(&mut self) -> Result<u8, CoreError> {
        Ok(self.get_bits(1)? as u8)
    }
}

fn read_frequencies(source: &[u8], offset: usize) -> Result<[u32; 31], CoreError> {
    checked_range(source, offset, 64, "ASTRA_EMU_CMVS_JBP_TREE")?;
    let mut result = [0_u32; 31];
    for (index, value) in result.iter_mut().take(16).enumerate() {
        *value = u32::try_from(read_u32(source, offset + index * 4)?)
            .map_err(|_| invalid("ASTRA_EMU_CMVS_JBP_TREE", "JBP frequency exceeds bounds"))?;
    }
    Ok(result)
}

fn align(value: usize, multiple: usize) -> Result<usize, CoreError> {
    value
        .checked_add(multiple - 1)
        .map(|value| value & !(multiple - 1))
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_DIMENSIONS", "JBP alignment overflowed"))
}

/// The original CMVS/GARbro JBP DCT stores every stage in a signed 16-bit
/// slot. Preserve that defined container behavior after computing in i64 so
/// Rust debug overflow checks cannot alter the decoded pixels.
fn wrap_i16(value: i64) -> i16 {
    value as i16
}

fn clamp(value: i64) -> u8 {
    value.clamp(0, 255) as u8
}

fn read_u16(source: &[u8], offset: usize) -> Result<u16, CoreError> {
    let bytes = source
        .get(offset..offset + 2)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_LAYOUT", "JBP integer is truncated"))?;
    Ok(u16::from_le_bytes(
        bytes.try_into().expect("bounded JBP u16"),
    ))
}

fn read_u32(source: &[u8], offset: usize) -> Result<usize, CoreError> {
    let bytes = source
        .get(offset..offset + 4)
        .ok_or_else(|| invalid("ASTRA_EMU_CMVS_JBP_LAYOUT", "JBP integer is truncated"))?;
    usize::try_from(u32::from_le_bytes(
        bytes.try_into().expect("bounded JBP u32"),
    ))
    .map_err(|_| {
        invalid(
            "ASTRA_EMU_CMVS_JBP_LAYOUT",
            "JBP integer exceeds platform bounds",
        )
    })
}

fn checked_range(
    source: &[u8],
    offset: usize,
    length: usize,
    code: &'static str,
) -> Result<(), CoreError> {
    if offset
        .checked_add(length)
        .is_none_or(|end| end > source.len())
    {
        return Err(invalid(code, "JBP range exceeds its source"));
    }
    Ok(())
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_runs_fill_exactly_the_declared_pixels() {
        let mut pixels = vec![0_u8; 12];
        apply_alpha(&[0, 2, 0xff, 1], 0, 4, &mut pixels).unwrap();
        assert_eq!(pixels[3], 0);
        assert_eq!(pixels[7], 0);
        assert_eq!(pixels[11], 0xff);
    }

    #[test]
    fn alpha_runs_cannot_exceed_the_image() {
        let mut pixels = vec![0_u8; 4];
        assert_eq!(
            apply_alpha(&[0, 2], 0, 2, &mut pixels).unwrap_err().code(),
            "ASTRA_EMU_CMVS_JBP_ALPHA"
        );
    }

    #[test]
    fn bit_stream_reads_msb_first() {
        let mut bits =
            JBitStream::new(&[0b0000_0011], 0, 1, "ASTRA_EMU_CMVS_JBP_TEST_BITS").unwrap();
        assert_eq!(bits.get_bits(2).unwrap(), 0b00);
        assert_eq!(bits.get_bits(2).unwrap(), 0b00);
        assert_eq!(bits.get_bits(3).unwrap(), 0b001);
        assert_eq!(bits.get_bits(1).unwrap(), 1);
    }
}

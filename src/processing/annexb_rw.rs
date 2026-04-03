/// AnnexB bitstream reader/writer — ported from `AnnexBBitstreamReaderWriter.ts`.
///
/// Used by `sps_vui.rs` to rewrite the SPS VUI timing info in H264 streams.

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

pub struct AnnexBBitstreamReader<'a> {
    buffer: &'a [u8],
    byte_offset: usize,
    bit_offset: u8,
}

impl<'a> AnnexBBitstreamReader<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            byte_offset: 0,
            bit_offset: 0,
        }
    }

    /// Read `count` bits (MSB first), skipping emulation-prevention bytes.
    pub fn read_bits(&mut self, mut count: u32) -> u32 {
        if count == 0 {
            return 0;
        }
        let mut result: u32 = 0;
        while count > 0 {
            assert!(
                self.byte_offset < self.buffer.len(),
                "AnnexBBitstreamReader: bad byte offset"
            );

            // Skip emulation prevention byte (0x00 0x00 0x03)
            if self.bit_offset == 0
                && self.byte_offset >= 2
                && self.buffer[self.byte_offset - 2] == 0
                && self.buffer[self.byte_offset - 1] == 0
                && self.buffer[self.byte_offset] == 3
            {
                self.byte_offset += 1;
            }

            if self.bit_offset == 0 && count >= 8 {
                // Byte-aligned — read a whole byte
                result = (result << 8) | self.buffer[self.byte_offset] as u32;
                self.byte_offset += 1;
                count -= 8;
            } else {
                let bits_to_read = count.min(8 - self.bit_offset as u32) as u8;
                let mask = (1u8 << bits_to_read) - 1;
                let new_bits = (self.buffer[self.byte_offset]
                    >> (8 - self.bit_offset - bits_to_read))
                    & mask;
                result = (result << bits_to_read) | new_bits as u32;
                count -= bits_to_read as u32;
                self.bit_offset += bits_to_read;
                if self.bit_offset == 8 {
                    self.bit_offset = 0;
                    self.byte_offset += 1;
                }
            }
        }
        result
    }

    #[inline]
    pub fn read_unsigned(&mut self, bits: u32) -> u32 {
        self.read_bits(bits)
    }

    #[inline]
    pub fn read_signed(&mut self, bits: u32) -> i32 {
        let unsigned = self.read_unsigned(bits);
        if unsigned & (1 << (bits - 1)) != 0 {
            unsigned as i32 - (1i32 << bits)
        } else {
            unsigned as i32
        }
    }

    /// Exponential-Golomb unsigned coding.
    pub fn read_ue(&mut self) -> u32 {
        let mut leading_zeros: u32 = 0;
        while self.read_bits(1) == 0 {
            leading_zeros += 1;
        }
        (1 << leading_zeros) + self.read_bits(leading_zeros) - 1
    }

    /// Exponential-Golomb signed coding.  
    /// Mapping: even → −x/2, odd → (x+1)/2
    pub fn read_se(&mut self) -> i32 {
        let unsigned = self.read_ue();
        if unsigned % 2 == 0 {
            -((unsigned / 2) as i32)
        } else {
            ((unsigned + 1) / 2) as i32
        }
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

pub struct AnnexBBitstreamWriter {
    arr: Vec<u8>,
    pending_byte: u8,
    bit_offset: u8,
}

impl AnnexBBitstreamWriter {
    pub fn new() -> Self {
        Self {
            arr: Vec::new(),
            pending_byte: 0,
            bit_offset: 0,
        }
    }

    pub fn to_vec(self) -> Vec<u8> {
        self.arr
    }

    /// Flush any pending partial byte (used after `rbsp_stop_one_bit`).
    pub fn flush_final(&mut self) {
        if self.bit_offset > 0 {
            self.flush();
        }
    }

    /// Flush the pending byte, inserting an emulation-prevention byte when needed.
    fn flush(&mut self) {
        if self.pending_byte <= 3
            && self.arr.len() >= 2
            && self.arr[self.arr.len() - 2] == 0
            && *self.arr.last().unwrap() == 0
        {
            self.arr.push(3);
        }
        self.arr.push(self.pending_byte);
        self.pending_byte = 0;
        self.bit_offset = 0;
    }

    pub fn write_bits(&mut self, bits: u32, mut count: u32) {
        while count > 0 {
            if self.bit_offset == 0 {
                if count >= 8 {
                    // Byte-aligned and ≥1 byte remaining
                    self.pending_byte = ((bits >> (count - 8)) & 0xFF) as u8;
                    count -= 8;
                    self.flush();
                } else {
                    // Less than one byte left
                    let mask = (1u32 << count) - 1;
                    self.pending_byte |= ((bits & mask) << (8 - count)) as u8;
                    self.bit_offset = count as u8;
                    count = 0;
                }
            } else {
                // Write enough bits to reach byte boundary
                let bits_to_write = (8 - self.bit_offset as u32).min(count);
                let to_write =
                    ((bits >> (count - bits_to_write)) & ((1 << bits_to_write) - 1)) as u8;
                self.pending_byte |= to_write << (8 - self.bit_offset - bits_to_write as u8);
                count -= bits_to_write;
                self.bit_offset += bits_to_write as u8;
                if self.bit_offset == 8 {
                    self.bit_offset = 0;
                    self.flush();
                }
            }
        }
    }

    #[inline]
    pub fn write_unsigned(&mut self, num: u32, count: u32) {
        self.write_bits(num, count);
    }

    pub fn write_signed(&mut self, num: i32, count: u32) {
        if count == 0 {
            return;
        }
        assert!(count <= 32, "write_signed supports up to 32 bits");
        let mask: u32 = if count == 32 { 0xFFFF_FFFF } else { (1u32 << count) - 1 };
        let unsigned = (num as u32) & mask;
        self.write_bits(unsigned, count);
    }

    /// Exponential-Golomb unsigned coding.
    pub fn write_ue(&mut self, num: u32) {
        let n = num + 1;
        let bit_count = 32 - n.leading_zeros(); // position of MSB
        self.write_bits(0, bit_count - 1); // leading zeros
        self.write_bits(n, bit_count);
    }

    /// Exponential-Golomb signed coding.
    pub fn write_se(&mut self, num: i32) {
        if num <= 0 {
            self.write_ue((-2 * num) as u32);
        } else {
            self.write_ue((2 * num - 1) as u32);
        }
    }
}

impl Default for AnnexBBitstreamWriter {
    fn default() -> Self {
        Self::new()
    }
}

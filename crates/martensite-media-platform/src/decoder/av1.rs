//! AV1 bitstream helpers for the VideoToolbox backend.
//!
//! CoreMedia has no `CMVideoFormatDescriptionCreateFromAV1ParameterSets`
//! equivalent: `av01` format descriptions are built with
//! `CMVideoFormatDescriptionCreate` plus an extensions dictionary holding the
//! `AV1CodecConfigurationRecord` as a `SampleDescriptionExtensionAtoms` atom
//! (`{"av1C": CFData}` — the same path FFmpeg's `videotoolbox_av1.c`,
//! WebKit's `CMUtilities`, and Chromium's `video_toolbox_av1_accelerator.cc`
//! use).
//!
//! This module provides the pure-Rust half of that bridge: it parses AV1 open
//! bitstream units (OBUs) in low-overhead format (each OBU carrying its
//! `leb128` size field, as IVF/Matroska temporal units and `av01` samples
//! require), extracts the sequence-header OBU, and parses the bit-level
//! sequence-header fields needed to assemble the `av1C` record per the
//! ISOBMFF AV1 codec-configuration spec.

/// One parsed OBU: header, optional extension byte, optional `leb128` size,
/// and payload — all views into the temporal unit it was parsed from.
pub(crate) struct Obu<'a> {
    /// `obu_type` from the OBU header (1 = `OBU_SEQUENCE_HEADER`).
    pub(crate) obu_type: u8,
    /// The complete OBU bytes: header + extension + size field + payload.
    pub(crate) raw: &'a [u8],
    /// The OBU payload (after header, extension, and size field).
    pub(crate) payload: &'a [u8],
    /// Whether the OBU carried `obu_has_size_field` (low-overhead format).
    pub(crate) has_size_field: bool,
}

/// `obu_type` of the sequence header OBU.
pub(crate) const OBU_SEQUENCE_HEADER: u8 = 1;

/// Decodes one AV1 unsigned `leb128` integer at the start of `data`.
///
/// Returns `(value, bytes_consumed)`, or `None` when the encoding runs past
/// the buffer or exceeds the 8-byte AV1 limit.
fn leb128(data: &[u8]) -> Option<(usize, usize)> {
    let mut value = 0u64;
    for (i, &byte) in data.iter().take(8).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((usize::try_from(value).ok()?, i + 1));
        }
    }
    None
}

/// Encodes `value` as an AV1 unsigned `leb128` integer.
fn leb128_encode(value: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut value = value as u64;
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            return out;
        }
    }
}

/// Parses the OBU sequence of one temporal unit (AV1 low-overhead bitstream
/// format). Parsing stops at the first malformed or truncated OBU; OBUs
/// without `obu_has_size_field` extend to the end of the buffer.
pub(crate) fn parse_obus(data: &[u8]) -> Vec<Obu<'_>> {
    let mut obus = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let header = data[pos];
        if header & 0x80 != 0 {
            break; // obu_forbidden_bit set — not an OBU stream.
        }
        let obu_type = (header >> 3) & 0x0f;
        let has_extension = header & 0x04 != 0;
        let has_size_field = header & 0x02 != 0;
        let mut cursor = pos + 1;
        if has_extension {
            if cursor >= data.len() {
                break;
            }
            cursor += 1;
        }
        let (payload, next) = if has_size_field {
            let Some((size, consumed)) = leb128(&data[cursor..]) else {
                break;
            };
            cursor += consumed;
            let Some(end) = cursor.checked_add(size) else {
                break;
            };
            if end > data.len() {
                break;
            }
            (&data[cursor..end], end)
        } else {
            (&data[cursor..], data.len())
        };
        obus.push(Obu {
            obu_type,
            raw: &data[pos..next],
            payload,
            has_size_field,
        });
        pos = next;
    }
    obus
}

/// Returns the first sequence-header OBU in a temporal unit, if present.
pub(crate) fn sequence_header_obu(data: &[u8]) -> Option<Obu<'_>> {
    parse_obus(data)
        .into_iter()
        .find(|obu| obu.obu_type == OBU_SEQUENCE_HEADER)
}

/// MSB-first bit reader over a byte slice.
struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl BitReader<'_> {
    fn new(data: &[u8]) -> BitReader<'_> {
        BitReader { data, bit_pos: 0 }
    }

    /// Reads `n` bits (≤ 32) as an unsigned big-endian value.
    fn bits(&mut self, n: u32) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..n {
            let byte = *self.data.get(self.bit_pos / 8)?;
            value = (value << 1) | u32::from((byte >> (7 - self.bit_pos % 8)) & 1);
            self.bit_pos += 1;
        }
        Some(value)
    }

    /// Reads a single flag bit.
    fn flag(&mut self) -> Option<bool> {
        self.bits(1).map(|v| v != 0)
    }

    /// Reads an AV1 `uvlc` value (leading-zero unary + suffix).
    fn uvlc(&mut self) -> Option<u32> {
        let mut leading_zeros = 0u32;
        while !self.flag()? {
            leading_zeros += 1;
            if leading_zeros >= 32 {
                return Some(u32::MAX);
            }
        }
        let suffix = self.bits(leading_zeros)?;
        Some((1u32 << leading_zeros) - 1 + suffix)
    }
}

/// The sequence-header fields the `AV1CodecConfigurationRecord` needs, plus
/// the coded dimensions used to size the format description.
pub(crate) struct SequenceHeader {
    /// `seq_profile` (3 bits).
    pub(crate) seq_profile: u8,
    /// `seq_level_idx[0]` (5 bits).
    pub(crate) seq_level_idx_0: u8,
    /// `seq_tier[0]` (1 bit).
    pub(crate) seq_tier_0: u8,
    /// `high_bitdepth` colour-config flag.
    pub(crate) high_bitdepth: bool,
    /// `twelve_bit` colour-config flag.
    pub(crate) twelve_bit: bool,
    /// `mono_chrome` colour-config flag.
    pub(crate) mono_chrome: bool,
    /// `subsampling_x` colour-config flag.
    pub(crate) subsampling_x: bool,
    /// `subsampling_y` colour-config flag.
    pub(crate) subsampling_y: bool,
    /// `chroma_sample_position` (2 bits).
    pub(crate) chroma_sample_position: u8,
    /// `initial_display_delay_present_flag` — recorded in `av1C` byte 3 as
    /// `initial_presentation_delay_present`.
    pub(crate) initial_display_delay_present: bool,
    /// `initial_display_delay_minus_1[0]` (4 bits; 0 when not present).
    pub(crate) initial_display_delay_minus_1: u8,
    /// `max_frame_width_minus_1 + 1`.
    pub(crate) max_frame_width: u32,
    /// `max_frame_height_minus_1 + 1`.
    pub(crate) max_frame_height: u32,
}

/// Parses `sequence_header_obu()` (AV1 spec §5.5.1) through `color_config()`
/// (§5.5.2), stopping once every field the `av1C` record needs has been read.
/// Returns `None` on truncation or a bitstream violation.
pub(crate) fn parse_sequence_header(payload: &[u8]) -> Option<SequenceHeader> {
    let mut r = BitReader::new(payload);

    let seq_profile = r.bits(3)? as u8;
    let _still_picture = r.flag()?;
    let reduced_still_picture_header = r.flag()?;

    let mut seq_tier_0 = 0u8;
    let mut initial_display_delay_present = false;
    let mut initial_display_delay_minus_1 = 0u8;

    let seq_level_idx_0 = if reduced_still_picture_header {
        r.bits(5)? as u8
    } else {
        let mut decoder_model_info_present = false;
        let mut buffer_delay_length = 0u32;
        if r.flag()? {
            // timing_info_present_flag → timing_info()
            r.bits(32)?; // num_units_in_display_tick
            r.bits(32)?; // time_scale
            if r.flag()? {
                // equal_picture_interval
                r.uvlc()?; // num_ticks_per_picture_minus_1
            }
            decoder_model_info_present = r.flag()?;
            if decoder_model_info_present {
                // decoder_model_info()
                buffer_delay_length = r.bits(5)? + 1;
                r.bits(32)?; // num_units_in_decoding_tick
                r.bits(5)?; // buffer_removal_time_length_minus_1
                r.bits(5)?; // buffer_removal_time_delay_minus_1
            }
        }
        initial_display_delay_present = r.flag()?;
        let operating_points_cnt_minus_1 = r.bits(5)?;
        let mut level_idx_0 = 0u8;
        for i in 0..=operating_points_cnt_minus_1 {
            r.bits(12)?; // operating_point_idc[i]
            let seq_level_idx = r.bits(5)? as u8;
            let seq_tier = if seq_level_idx > 7 {
                u8::from(r.flag()?)
            } else {
                0
            };
            if decoder_model_info_present {
                // decoder_model_present_for_this_op[i]
                if r.flag()? {
                    // operating_parameters_info(i)
                    r.bits(buffer_delay_length)?; // decoder_buffer_delay
                    r.bits(buffer_delay_length)?; // encoder_buffer_delay
                    r.flag()?; // low_delay_mode_flag
                }
            }
            if initial_display_delay_present {
                // initial_display_delay_present_for_this_op[i]
                if r.flag()? {
                    let delay = r.bits(4)? as u8;
                    if i == 0 {
                        initial_display_delay_minus_1 = delay;
                    }
                }
            }
            if i == 0 {
                level_idx_0 = seq_level_idx;
                seq_tier_0 = seq_tier;
            }
        }
        level_idx_0
    };

    let frame_width_bits = r.bits(4)? + 1;
    let frame_height_bits = r.bits(4)? + 1;
    let max_frame_width = r.bits(frame_width_bits)? + 1;
    let max_frame_height = r.bits(frame_height_bits)? + 1;

    if !reduced_still_picture_header && r.flag()? {
        // frame_id_numbers_present_flag
        r.bits(4)?; // delta_frame_id_length_minus_2
        r.bits(3)?; // additional_frame_id_length_minus_1
    }
    r.flag()?; // use_128x128_superblock
    r.flag()?; // enable_filter_intra
    r.flag()?; // enable_intra_edge_filter
    if !reduced_still_picture_header {
        r.flag()?; // enable_interintra_compound
        r.flag()?; // enable_masked_compound
        r.flag()?; // enable_warped_motion
        r.flag()?; // enable_dual_filter
        let enable_order_hint = r.flag()?;
        if enable_order_hint {
            r.flag()?; // enable_jnt_comp
            r.flag()?; // enable_ref_frame_mvs
        }
        let seq_force_screen_content_tools = if r.flag()? {
            // seq_choose_screen_content_tools → SELECT_SCREEN_CONTENT_TOOLS
            2
        } else {
            r.bits(2)?
        };
        if seq_force_screen_content_tools > 0 && !r.flag()? {
            // !seq_choose_integer_mv → seq_force_integer_mv
            r.bits(2)?;
        }
        if enable_order_hint {
            r.bits(3)?; // order_hint_bits_minus_1
        }
    }
    r.flag()?; // enable_superres
    r.flag()?; // enable_cdef
    r.flag()?; // enable_restoration

    // color_config() — only through chroma_sample_position is needed.
    let high_bitdepth = r.flag()?;
    let twelve_bit = if seq_profile == 2 && high_bitdepth {
        r.flag()?
    } else {
        false
    };
    let bit_depth_12 = twelve_bit;
    let mono_chrome = if seq_profile == 1 { false } else { r.flag()? };
    let (primaries, transfer, matrix) = if r.flag()? {
        // color_description_present_flag
        (r.bits(8)?, r.bits(8)?, r.bits(8)?)
    } else {
        (2, 2, 2) // CP/TC/MC_UNSPECIFIED
    };
    let subsampling_x;
    let subsampling_y;
    let mut chroma_sample_position = 0u8;
    if mono_chrome {
        r.flag()?; // color_range
        subsampling_x = true;
        subsampling_y = true;
    } else if primaries == 1 && transfer == 13 && matrix == 0 {
        // CP_BT_709 + TC_SRGB + MC_IDENTITY: full-range identity, no
        // subsampling bits in the stream.
        subsampling_x = false;
        subsampling_y = false;
    } else {
        r.flag()?; // color_range
        let (sx, sy) = match seq_profile {
            0 => (true, true),
            1 => (false, false),
            _ => {
                if bit_depth_12 {
                    let sx = r.flag()?;
                    let sy = if sx { r.flag()? } else { false };
                    (sx, sy)
                } else {
                    (true, false)
                }
            }
        };
        subsampling_x = sx;
        subsampling_y = sy;
        if sx && sy {
            chroma_sample_position = r.bits(2)? as u8;
        }
    }

    Some(SequenceHeader {
        seq_profile,
        seq_level_idx_0,
        seq_tier_0,
        high_bitdepth,
        twelve_bit,
        mono_chrome,
        subsampling_x,
        subsampling_y,
        chroma_sample_position,
        initial_display_delay_present,
        initial_display_delay_minus_1,
        max_frame_width,
        max_frame_height,
    })
}

/// Builds an `AV1CodecConfigurationRecord` (`av1C` atom payload) from a
/// sequence-header OBU and its parsed header — the 4-byte record header
/// followed by the sequence-header OBU itself in low-overhead form, matching
/// FFmpeg's `ff_videotoolbox_av1c_extradata_create` and the ISOBMFF `av1C`
/// spec.
///
/// `seq` must be the result of [`parse_sequence_header`] on `seq_obu.payload`
/// (kept as a parameter so callers needing the parsed fields don't parse
/// twice).
pub(crate) fn codec_config_record(seq_obu: &Obu<'_>, seq: &SequenceHeader) -> Vec<u8> {
    let mut rec = Vec::with_capacity(seq_obu.raw.len() + 8);
    rec.push(0x81); // marker (1) | version (1)
    rec.push(seq.seq_profile << 5 | seq.seq_level_idx_0);
    rec.push(
        seq.seq_tier_0 << 7
            | u8::from(seq.high_bitdepth) << 6
            | u8::from(seq.twelve_bit) << 5
            | u8::from(seq.mono_chrome) << 4
            | u8::from(seq.subsampling_x) << 3
            | u8::from(seq.subsampling_y) << 2
            | seq.chroma_sample_position,
    );
    rec.push(if seq.initial_display_delay_present {
        seq.initial_display_delay_minus_1 | 0x10
    } else {
        0
    });

    // configOBUs: the sequence-header OBU. ISOBMFF stores OBUs in
    // low-overhead form; when the source OBU lacked obu_has_size_field the
    // `leb128` size is synthesized so the stored OBU is self-delimiting.
    if seq_obu.has_size_field {
        rec.extend_from_slice(seq_obu.raw);
    } else {
        // `raw` always holds at least the header byte (and the extension
        // byte when obu_extension_flag is set) — `parse_obus` guarantees it.
        let header = seq_obu.raw[0];
        rec.push(header | 0x02);
        if header & 0x04 != 0 {
            rec.push(seq_obu.raw[1]);
        }
        rec.extend_from_slice(&leb128_encode(seq_obu.payload.len()));
        rec.extend_from_slice(seq_obu.payload);
    }
    rec
}

/// Minimal sanity check for an `AV1CodecConfigurationRecord` supplied as
/// extradata: the record needs the 4-byte header (marker/version `0x81`)
/// plus at least one configOBU byte.
pub(crate) fn is_valid_av1c(record: &[u8]) -> bool {
    record.len() >= 5 && record[0] == 0x81
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sequence-header OBU captured from `av1-4k120.bin` (libaom, profile 0,
    /// 3840x2160, level_idx 14): header `0x0a`, size `0x0c`, 12-byte payload.
    const SEQ_OBU: &[u8] = &[
        0x0a, 0x0c, 0x00, 0x00, 0x00, 0x72, 0xef, 0xbf, 0xe1, 0xbc, 0x6a, 0xf9, 0x00, 0x40,
    ];

    #[test]
    fn leb128_roundtrip() {
        for v in [0usize, 1, 0x7f, 0x80, 0x3fff, 156_930, 1 << 21] {
            let enc = leb128_encode(v);
            assert_eq!(leb128(&enc), Some((v, enc.len())));
        }
        assert_eq!(leb128(&[0x82, 0xca, 0x09]), Some((156_930, 3)));
        // Unterminated encoding → None.
        assert_eq!(leb128(&[0x80]), None);
    }

    #[test]
    fn parses_low_overhead_temporal_unit() {
        // TD OBU (type 2, size 0) + sequence header + frame OBU (type 6).
        let mut tu = vec![0x12, 0x00];
        tu.extend_from_slice(SEQ_OBU);
        tu.extend_from_slice(&[0x32, 0x05, 0xde, 0xad, 0xbe, 0xef, 0x00]);
        let obus = parse_obus(&tu);
        assert_eq!(obus.len(), 3);
        assert_eq!(obus[0].obu_type, 2);
        assert_eq!(obus[1].obu_type, OBU_SEQUENCE_HEADER);
        assert_eq!(obus[1].raw, SEQ_OBU);
        assert_eq!(obus[2].obu_type, 6);
        assert_eq!(obus[2].payload, &[0xde, 0xad, 0xbe, 0xef, 0x00]);
    }

    #[test]
    fn obu_without_size_field_extends_to_end() {
        // Header 0x08: type 1, no extension, obu_has_size_field = 0.
        let obus = parse_obus(&[0x08, 0xaa, 0xbb]);
        assert_eq!(obus.len(), 1);
        assert!(!obus[0].has_size_field);
        assert_eq!(obus[0].payload, &[0xaa, 0xbb]);
    }

    #[test]
    fn sequence_header_fields() {
        let mut tu = vec![0x12, 0x00];
        tu.extend_from_slice(SEQ_OBU);
        let obu = sequence_header_obu(&tu).expect("seq header");
        let seq = parse_sequence_header(obu.payload).expect("parseable");
        assert_eq!(seq.seq_profile, 0);
        assert_eq!(seq.seq_level_idx_0, 14);
        assert_eq!(seq.seq_tier_0, 0);
        assert!(!seq.high_bitdepth);
        assert!(!seq.mono_chrome);
        assert!(seq.subsampling_x);
        assert!(seq.subsampling_y);
        assert_eq!(seq.chroma_sample_position, 0);
        assert!(!seq.initial_display_delay_present);
        assert_eq!(seq.max_frame_width, 3840);
        assert_eq!(seq.max_frame_height, 2160);
    }

    #[test]
    fn builds_av1c_record() {
        let obus = parse_obus(SEQ_OBU);
        let seq = parse_sequence_header(obus[0].payload).expect("parseable");
        let rec = codec_config_record(&obus[0], &seq);
        assert_eq!(&rec[..4], &[0x81, 0x0e, 0x0c, 0x00]);
        assert_eq!(&rec[4..], SEQ_OBU);
        assert!(is_valid_av1c(&rec));
        assert!(!is_valid_av1c(&[0x81, 0, 0, 0]));
        assert!(!is_valid_av1c(&[0x82, 0x0e, 0x04, 0x00, 0x0a]));
    }

    #[test]
    fn av1c_synthesizes_missing_size_field() {
        // Same OBU payload, has_size_field=0 → payload runs to end of buffer.
        let mut raw = vec![0x08]; // type 1, no extension, no size field
        raw.extend_from_slice(&SEQ_OBU[2..]);
        let obus = parse_obus(&raw);
        assert_eq!(obus.len(), 1);
        assert!(!obus[0].has_size_field);
        let seq = parse_sequence_header(obus[0].payload).expect("parseable");
        let rec = codec_config_record(&obus[0], &seq);
        assert_eq!(&rec[4..], SEQ_OBU);
    }

    #[test]
    fn rejects_truncated_sequence_header() {
        assert!(parse_sequence_header(&[0x00, 0x00]).is_none());
        assert!(sequence_header_obu(&[0xff, 0x00, 0x0a]).is_none());
    }
}

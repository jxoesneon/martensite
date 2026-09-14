//! Shared helpers for the real-decode conformance tests.
//!
//! Each `tests/*.rs` file is its own crate; helpers live here so the ffmpeg
//! and VideoToolbox suites share the Annex-B splitter and fixture path.
//! Not every suite uses every helper.
#![allow(dead_code)]

/// The checked-in 320x240@30fps baseline H.264 Annex-B stream
/// (30 frames, ~10 KB) used by every real-decode test.
pub const STREAM_320X240: &[u8] = include_bytes!("../fixtures/test-320x240.h264");

/// Frame interval of the fixture stream (30 fps).
pub const FRAME_NS: u64 = 33_333_333;

/// Splits an Annex-B byte stream into access units: a new AU starts at each
/// VCL NAL (type 1–5) that follows a VCL NAL — SPS/PPS/SEI/AUD prefix NALs
/// attach to the AU they precede.
pub fn annex_b_access_units(stream: &[u8]) -> Vec<&[u8]> {
    annex_b_split(stream, |header| (1..=5).contains(&(header & 0x1f)))
}

/// Splits an Annex-B HEVC byte stream into access units using the same rule
/// as [`annex_b_access_units`]: a new AU starts at each VCL NAL (types 0–31)
/// that follows a VCL NAL — VPS/SPS/PPS/AUD/SEI prefix NALs attach to the AU
/// they precede. The HEVC NAL header is two bytes and the unit type lives in
/// bits 1–6 of its first byte.
pub fn hevc_access_units(stream: &[u8]) -> Vec<&[u8]> {
    annex_b_split(stream, |header| ((header >> 1) & 0x3f) <= 31)
}

/// Shared Annex-B access-unit splitter: `is_vcl` classifies the NAL header
/// byte (H.264 `type & 0x1f` ∈ 1–5, HEVC `(type >> 1) & 0x3f` ∈ 0–31). A new
/// AU starts at each VCL NAL that follows a VCL NAL; non-VCL prefix NALs
/// attach to the AU they precede.
fn annex_b_split(stream: &[u8], is_vcl: impl Fn(u8) -> bool) -> Vec<&[u8]> {
    // (slice_start, nal_header) per NAL: `slice_start` is the first byte of
    // the start code, `nal_header` the byte after it.
    let mut nals: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i + 3 < stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            let header = i + 3;
            let start = if i > 0 && stream[i - 1] == 0 {
                i - 1
            } else {
                i
            };
            if header < stream.len() {
                nals.push((start, header));
            }
            i = header;
        } else {
            i += 1;
        }
    }

    let mut aus: Vec<&[u8]> = Vec::new();
    let mut au_start = 0usize;
    let mut au_has_vcl = false;
    for &(start, header) in &nals {
        let vcl = is_vcl(stream[header]);
        if vcl && au_has_vcl {
            aus.push(&stream[au_start..start]);
            au_start = start;
            au_has_vcl = false;
        }
        au_has_vcl |= vcl;
    }
    if au_start < stream.len() {
        aus.push(&stream[au_start..]);
    }
    aus
}

/// Returns `(payload_start, payload_end)` for every NAL unit in an Annex-B
/// stream — payload excludes the start code (and any trailing zero of a
/// 4-byte start code belongs to the next NAL's prefix, not this payload).
fn annex_b_nals(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts: Vec<(usize, usize)> = Vec::new(); // (start_code, payload)
    let mut i = 0;
    while i + 3 < stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            let header = i + 3;
            starts.push((i, header));
            i = header;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(idx, &(_, payload))| {
            let end = starts.get(idx + 1).map(|&(s, _)| s).unwrap_or(stream.len());
            &stream[payload..end]
        })
        .collect()
}

/// Builds an `AVCDecoderConfigurationRecord` (`avcC`) from the SPS/PPS NAL
/// units of an Annex-B H.264 stream. `None` when no SPS is present.
pub fn avcc_record(stream: &[u8]) -> Option<Vec<u8>> {
    let nals = annex_b_nals(stream);
    let mut sps: Vec<&[u8]> = Vec::new();
    let mut pps: Vec<&[u8]> = Vec::new();
    for nal in nals {
        match nal.first()? & 0x1f {
            7 => sps.push(nal),
            8 => pps.push(nal),
            _ => {}
        }
    }
    let first_sps = *sps.first()?;

    let mut rec = vec![
        0x01,                   // configurationVersion
        first_sps[1],           // AVCProfileIndication
        first_sps[2],           // profile_compatibility
        first_sps[3],           // AVCLevelIndication
        0xFF,                   // reserved + lengthSizeMinusOne = 3 (4-byte lengths)
        0xE0 | sps.len() as u8, // reserved + numOfSequenceParameterSets
    ];
    for s in &sps {
        rec.extend_from_slice(&(s.len() as u16).to_be_bytes());
        rec.extend_from_slice(s);
    }
    rec.push(pps.len() as u8); // numOfPictureParameterSets
    for p in &pps {
        rec.extend_from_slice(&(p.len() as u16).to_be_bytes());
        rec.extend_from_slice(p);
    }
    Some(rec)
}

/// Builds a minimal `HEVCDecoderConfigurationRecord` (`hvcC`) from the
/// VPS/SPS/PPS NAL units of an Annex-B HEVC stream. `None` when no SPS is
/// present.
///
/// The record is deliberately minimal: the only fields the consumers in this
/// workspace read are `lengthSizeMinusOne` (byte 21, fixed to 4-byte
/// lengths) and the parameter-set arrays starting at byte 22, so the
/// profile/level/constraint bytes (1–20) are left zero — decoders re-parse
/// the SPS itself. NAL units are stored complete with their two-byte
/// headers, matching the `avcC` convention of including the NAL header.
pub fn hvcc_record(stream: &[u8]) -> Option<Vec<u8>> {
    let nals = annex_b_nals(stream);
    // Parameter sets in NAL-type order: VPS (32), SPS (33), PPS (34).
    let mut groups: [(u8, Vec<&[u8]>); 3] = [(32, Vec::new()), (33, Vec::new()), (34, Vec::new())];
    for nal in nals {
        if nal.len() < 2 {
            continue; // HEVC NAL headers are two bytes; a shorter run is padding.
        }
        for (nal_type, group) in &mut groups {
            if (nal[0] >> 1) & 0x3f == *nal_type {
                group.push(nal);
            }
        }
    }
    if groups[1].1.is_empty() {
        return None; // no SPS → nothing a decoder can configure from
    }

    let num_arrays = groups.iter().filter(|(_, g)| !g.is_empty()).count() as u8;
    let mut rec = vec![0x01]; // configurationVersion
    rec.extend_from_slice(&[0; 20]); // profile/level/constraints fields (unused)
    rec.push(0xFF); // reserved + lengthSizeMinusOne = 3 (4-byte lengths)
    rec.push(num_arrays);
    for (nal_type, group) in &groups {
        if group.is_empty() {
            continue;
        }
        rec.push(0x80 | nal_type); // array_completeness + NAL_unit_type
        rec.extend_from_slice(&(group.len() as u16).to_be_bytes());
        for nal in group {
            rec.extend_from_slice(&(nal.len() as u16).to_be_bytes());
            rec.extend_from_slice(nal);
        }
    }
    Some(rec)
}

/// Converts one Annex-B access unit to 4-byte length-prefixed (`avcC`
/// sample) form for APIs that require it (VideoToolbox).
pub fn au_to_avcc(au: &[u8]) -> Vec<u8> {
    au_to_length_prefixed(au)
}

/// Converts one Annex-B HEVC access unit to 4-byte length-prefixed (`hvcC`
/// sample) form for APIs that require it (VideoToolbox).
///
/// Unlike [`au_to_avcc`] this omits parameter-set NALs (VPS/SPS/PPS, types
/// 32–34): a format description built from `hvcC` parameter sets describes
/// `hvc1` samples, which must not carry those NALs in-band — the record
/// already supplies them, and strict decoders (VideoToolbox) answer with
/// `kVTVideoDecoderBadDataErr` when a sample repeats them. VCL, AUD and SEI
/// NALs pass through unchanged.
pub fn au_to_hvcc(au: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(au.len());
    for nal in annex_b_nals(au) {
        if nal.len() >= 2 && matches!((nal[0] >> 1) & 0x3f, 32..=34) {
            continue;
        }
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

/// Shared Annex-B → 4-byte length-prefixed converter backing
/// [`au_to_avcc`]; [`au_to_hvcc`] uses its own loop so it can filter
/// parameter-set NALs out of `hvc1` samples.
fn au_to_length_prefixed(au: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(au.len());
    for nal in annex_b_nals(au) {
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

/// Directory holding the generated `*-4k120.bin` sample assets.
///
/// `$MARTENSITE_MEDIA_SAMPLES` overrides the default
/// `<workspace>/target/media-samples` location (`CARGO_MANIFEST_DIR` is
/// `crates/martensite-media-test`, two levels below the workspace root).
/// The samples are gitignored build artifacts — tests that need them skip
/// gracefully when the directory or file is absent.
pub fn samples_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("MARTENSITE_MEDIA_SAMPLES") {
        return std::path::PathBuf::from(dir);
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join("media-samples")
}

/// Parses an IVF file (despite the `.bin` extension used by the sample
/// assets) into owned per-frame payloads.
///
/// IVF is a trivial container: a fixed-size header opening with the `DKIF`
/// signature, then a sequence of `[u32 LE size][u64 LE pts][payload]` frame
/// records. Returns `None` when the signature is absent or no complete frame
/// is present; a truncated tail record simply stops parsing.
pub fn ivf_frames(file: &[u8]) -> Option<Vec<Vec<u8>>> {
    if file.len() < 32 || file[..4] != *b"DKIF" {
        return None;
    }
    // The header size is a u16 LE at offset 6 (32 in every conforming file);
    // honour it so streams with extension headers still parse.
    let header_len = usize::from(u16::from_le_bytes([file[6], file[7]]));
    let mut pos = header_len.max(32);
    let mut frames = Vec::new();
    while pos + 12 <= file.len() {
        let size = u32::from_le_bytes([file[pos], file[pos + 1], file[pos + 2], file[pos + 3]]);
        pos += 12; // 4-byte size + 8-byte timestamp
        let Some(end) = pos.checked_add(size as usize) else {
            break;
        };
        if end > file.len() {
            break;
        }
        frames.push(file[pos..end].to_vec());
        pos = end;
    }
    if frames.is_empty() {
        return None;
    }
    Some(frames)
}

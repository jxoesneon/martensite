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
        let nal_type = stream[header] & 0x1f;
        let is_vcl = (1..=5).contains(&nal_type);
        if is_vcl && au_has_vcl {
            aus.push(&stream[au_start..start]);
            au_start = start;
            au_has_vcl = false;
        }
        au_has_vcl |= is_vcl;
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

/// Converts one Annex-B access unit to 4-byte length-prefixed (`avcC`
/// sample) form for APIs that require it (VideoToolbox).
pub fn au_to_avcc(au: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(au.len());
    for nal in annex_b_nals(au) {
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

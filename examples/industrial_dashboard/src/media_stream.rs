//! Real H.264 decode source for the media panel.
//!
//! Packetizes the checked-in 320×240@30fps Annex-B fixture from
//! `martensite-media-test` into `avcC` (length-prefixed) access units —
//! the framing every platform decoder here expects — with SPS/PPS
//! extracted into out-of-band `codec_config`, mirroring what a real
//! demuxer delivers. The fixture covers one second; `MediaPanel` loops
//! it continuously.

use martensite::media::decoder::{DecoderConfig, EncodedPacket, VideoCodec, VideoDecoder};

/// The checked-in 320×240@30fps H.264 Annex-B stream (30 frames).
const STREAM: &[u8] =
    include_bytes!("../../../crates/martensite-media-test/tests/fixtures/test-320x240.h264");

/// Coded dimensions of the fixture stream.
pub const STREAM_W: u32 = 320;
pub const STREAM_H: u32 = 240;

/// Frame interval of the fixture stream (30 fps).
pub const FRAME_NS: u64 = 33_333_333;

/// One packetized clip pass, ready to feed a [`VideoDecoder`].
pub struct ClipStream {
    /// `avcC` (length-prefixed) access units in decode order.
    packets: Vec<Vec<u8>>,
    /// Whether each access unit carries an IDR NAL.
    keyframe: Vec<bool>,
    /// `avcC` configuration record (SPS/PPS) from the first AU with an
    /// SPS — mirrors demuxer-supplied extradata.
    codec_config: Vec<u8>,
}

impl ClipStream {
    /// Packetizes the fixture; `None` when no SPS is found.
    pub fn load() -> Option<Self> {
        let aus = annex_b_access_units(STREAM);
        let codec_config = aus.iter().find_map(|au| avcc_record(au))?;
        let keyframe = aus.iter().map(|au| au_has_idr(au)).collect();
        let packets = aus.iter().map(|au| au_to_avcc(au)).collect();
        Some(Self {
            packets,
            keyframe,
            codec_config,
        })
    }

    /// Number of access units in one clip pass.
    pub fn len(&self) -> usize {
        self.packets.len()
    }

    /// Whether the clip carries no access units.
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    /// Builds the packet for `index` with the given presentation time.
    pub fn packet(&self, index: usize, pts_nanos: u64) -> EncodedPacket {
        let pkt = EncodedPacket::new(self.packets[index].clone(), pts_nanos, FRAME_NS);
        if self.keyframe[index] {
            pkt
        } else {
            pkt.delta()
        }
    }

    /// Decoder configuration carrying the fixture's `avcC` extradata.
    pub fn decoder_config(&self) -> DecoderConfig {
        DecoderConfig::new(VideoCodec::H264, STREAM_W, STREAM_H)
            .with_codec_config(self.codec_config.clone())
    }
}

/// Builds the platform hardware decoder for this target, or `None` when
/// the backend is unavailable (feature off, wrong OS, or init failure —
/// the panel falls back to the honest empty state).
pub fn platform_decoder(clip: &ClipStream) -> Option<Box<dyn VideoDecoder>> {
    let config = clip.decoder_config();
    #[cfg(target_os = "macos")]
    {
        use martensite::media::decoder::videotoolbox::VideoToolboxDecoder;
        if let Ok(dec) = VideoToolboxDecoder::init(config.clone()) {
            return Some(Box::new(dec));
        }
    }
    #[cfg(target_os = "windows")]
    {
        use martensite::media::decoder::mediafoundation::MediaFoundationDecoder;
        if let Ok(dec) = MediaFoundationDecoder::init(config.clone()) {
            return Some(Box::new(dec));
        }
    }
    #[cfg(target_os = "linux")]
    {
        use martensite::media::decoder::vaapi::VaapiDecoder;
        if let Ok(dec) = VaapiDecoder::init(config.clone()) {
            return Some(Box::new(dec));
        }
    }
    let _ = config;
    None
}

/// Payload slice of every NAL unit in an Annex-B stream — the payload
/// excludes the start code (and the extra leading zero of a 4-byte
/// start code trails into the previous NAL's payload, matching the
/// demuxer-conformance helper).
fn annex_b_nals(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts: Vec<(usize, usize)> = Vec::new(); // (start_code, payload)
    let mut i = 0;
    while i + 3 < stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            starts.push((i, i + 3));
            i += 3;
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

/// Splits an Annex-B H.264 stream into access units: a new AU starts at
/// each VCL NAL (types 1–5) once the current AU already carries a VCL —
/// SPS/PPS/AUD/SEI prefix NALs attach to the AU they precede.
fn annex_b_access_units(stream: &[u8]) -> Vec<&[u8]> {
    // (slice_start, nal_header) per NAL: `slice_start` is the first byte
    // of the start code, `nal_header` the byte after it.
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
        let vcl = (1..=5).contains(&(stream[header] & 0x1f));
        if vcl && au_has_vcl {
            aus.push(&stream[au_start..start]);
            au_start = start;
        }
        au_has_vcl |= vcl;
    }
    if au_has_vcl {
        aus.push(&stream[au_start..]);
    }
    aus
}

/// Whether the access unit carries an IDR (type-5) NAL.
fn au_has_idr(au: &[u8]) -> bool {
    annex_b_nals(au)
        .iter()
        .any(|nal| nal.first().is_some_and(|h| h & 0x1f == 5))
}

/// Rewrites one access unit from Annex-B start codes to `avcC`
/// (4-byte big-endian length) framing.
fn au_to_avcc(au: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(au.len());
    for nal in annex_b_nals(au) {
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

/// Builds the `avcC` configuration record from the SPS/PPS NAL units of
/// an access unit. `None` when no SPS is present.
fn avcc_record(au: &[u8]) -> Option<Vec<u8>> {
    let nals = annex_b_nals(au);
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
    if first_sps.len() < 4 {
        return None;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_packetizes_fixture() {
        let clip = ClipStream::load().expect("fixture must packetize");
        assert_eq!(clip.len(), 30);
    }

    #[test]
    fn videotoolbox_init_succeeds_with_avcc_extradata() {
        let clip = ClipStream::load().expect("clip");
        #[cfg(target_os = "macos")]
        {
            use martensite::media::decoder::videotoolbox::VideoToolboxDecoder;
            match VideoToolboxDecoder::init(clip.decoder_config()) {
                Ok(_) => {}
                Err(e) => panic!("VT init failed: {e:?}"),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = clip;
        }
    }
}

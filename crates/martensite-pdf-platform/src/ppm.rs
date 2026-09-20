//! Binary PPM (`P6`) decoding — the interchange format `pdftoppm
//! -ppm` and `mutool draw -o out.ppm` emit. Parsing stays in safe
//! Rust; no image-codec dependency is needed because the rasterizers
//! output raw samples.
//!
//! # Examples
//!
//! ```
//! use martensite_pdf_platform::ppm::decode_ppm;
//!
//! // 1×1 red pixel.
//! let ppm = b"P6\n1 1\n255\n\xFF\x00\x00";
//! let (w, h, rgba) = decode_ppm(ppm).unwrap();
//! assert_eq!((w, h), (1, 1));
//! assert_eq!(rgba, vec![255, 0, 0, 255]);
//! ```

/// Decode only the `P6` header — `(width, height)` without touching
/// the pixel payload. Used for page-size probes that render a page
/// at 72 dpi (where px == pt) and only need dimensions.
///
/// ```
/// use martensite_pdf_platform::ppm::decode_ppm_header;
///
/// assert_eq!(decode_ppm_header(b"P6\n4 2\n255\n"), Some((4, 2)));
/// assert_eq!(decode_ppm_header(b"P3\n4 2\n255\n"), None);
/// ```
pub fn decode_ppm_header(data: &[u8]) -> Option<(u32, u32)> {
    let mut pos = 0usize;
    if next_token(data, &mut pos)? != b"P6" {
        return None;
    }
    let w = parse_u32(next_token(data, &mut pos)?)?;
    let h = parse_u32(next_token(data, &mut pos)?)?;
    (w > 0 && h > 0).then_some((w, h))
}

/// Decode a `P6` PPM stream to `(width, height, rgba8)`.
///
/// Returns `None` on any malformed input — wrong magic, missing
/// header fields, `maxval != 255`, or a truncated pixel payload.
/// `#` comment lines anywhere in the header are skipped per the
/// netpbm spec.
///
/// ```
/// use martensite_pdf_platform::ppm::decode_ppm;
///
/// assert!(decode_ppm(b"P3\n1 1\n255\n0 0 0").is_none()); // ASCII PPM
/// assert!(decode_ppm(b"P6\n1 1\n255\n\x00\x00").is_none()); // short
/// ```
pub fn decode_ppm(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut pos = 0usize;
    let magic = next_token(data, &mut pos)?;
    if magic != b"P6" {
        return None;
    }
    let w = parse_u32(next_token(data, &mut pos)?)?;
    let h = parse_u32(next_token(data, &mut pos)?)?;
    let maxval = parse_u32(next_token(data, &mut pos)?)?;
    if w == 0 || h == 0 || maxval != 255 {
        return None;
    }
    // Exactly one whitespace byte separates maxval from the payload.
    if pos >= data.len() || !data[pos].is_ascii_whitespace() {
        return None;
    }
    pos += 1;
    let need = w.checked_mul(h)?.checked_mul(3)? as usize;
    if data.len() - pos < need {
        return None;
    }
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for px in data[pos..pos + need].as_chunks::<3>().0 {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Some((w, h, rgba))
}

/// Read the next whitespace-delimited token, skipping `#` comment
/// lines (which run to end-of-line per the netpbm spec).
fn next_token<'a>(data: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    loop {
        while *pos < data.len() && data[*pos].is_ascii_whitespace() {
            *pos += 1;
        }
        if *pos < data.len() && data[*pos] == b'#' {
            while *pos < data.len() && data[*pos] != b'\n' {
                *pos += 1;
            }
            continue;
        }
        break;
    }
    if *pos >= data.len() {
        return None;
    }
    let start = *pos;
    while *pos < data.len() && !data[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    Some(&data[start..*pos])
}

fn parse_u32(tok: &[u8]) -> Option<u32> {
    std::str::from_utf8(tok).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_multi_pixel() {
        // 2×1: black + white.
        let ppm = b"P6 2 1 255 \x00\x00\x00\xFF\xFF\xFF";
        let (w, h, rgba) = decode_ppm(ppm).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, vec![0, 0, 0, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn skips_header_comments() {
        let ppm = b"P6\n# written by test\n1 1\n255\n\x01\x02\x03";
        assert_eq!(decode_ppm(ppm).unwrap().2, vec![1, 2, 3, 255]);
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(decode_ppm(b"").is_none());
        assert!(decode_ppm(b"P6\n0 1\n255\n").is_none());
        assert!(decode_ppm(b"P6\n1 1\n65535\n\x00\x00\x00").is_none());
        assert!(decode_ppm(b"P6\n1 1\n255").is_none()); // no separator
    }
}

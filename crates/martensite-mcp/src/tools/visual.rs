//! Category G (part 2) — Visual Verification tools (spec §3.18–3.19).

use std::borrow::Cow;
use std::time::Duration;

use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::McpError;
use crate::server::MartensiteMcp;

/// Image formats accepted by `martensite_capture_node` (spec §3.18).
const CAPTURE_FORMATS: &[&str] = &["png", "jpeg"];

/// Timeout for the headless render harness subprocess.
const HEADLESS_TIMEOUT: Duration = Duration::from_secs(120);

/// Maximum bytes of captured subprocess output retained in the result tail.
const OUTPUT_TAIL_BYTES: usize = 4096;

/// Standard base64 alphabet (RFC 4648).
const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Parameters for `martensite_capture_node`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CaptureNodeParams {
    /// Target `WidgetId` or `debug_name` to rasterize.
    pub node_id: String,
    /// `"png"` (default) or `"jpeg"`.
    pub format: Option<String>,
    /// DPI scale multiplier (default 1.0).
    pub scale: Option<f64>,
    /// Return inline Base64 instead of an artifact file URI (default false).
    pub include_base64: Option<bool>,
}

/// Structured result of `martensite_capture_node`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::visual::CaptureNodeOutput;
///
/// let out = CaptureNodeOutput {
///     artifact_uri: "file:///run/martensite/captures/root.png".to_string(),
///     logical_size: [100.0, 40.0],
///     physical_size: [200, 80],
///     base64: None,
/// };
/// assert!(out.artifact_uri.starts_with("file://"));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct CaptureNodeOutput {
    /// `file://` URI of the persisted capture artifact.
    pub artifact_uri: String,
    /// Logical `[width, height]` in density-independent pixels.
    pub logical_size: [f32; 2],
    /// Physical `[width, height]` in device pixels.
    #[schemars(schema_with = "crate::types::schema_strip::u32p")]
    pub physical_size: [u32; 2],
    /// Inline base64 image payload — only populated when the caller passed
    /// `include_base64: true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
}

/// `martensite_capture_node`: render a widget subtree to an image artifact.
///
/// Efficiency invariant (spec §3.18): writes to `$XDG_RUNTIME_DIR/martensite/
/// captures/` and returns a file URI; inline base64 only on explicit request.
pub struct CaptureNodeTool;

impl ToolBase for CaptureNodeTool {
    type Parameter = CaptureNodeParams;
    type Output = CaptureNodeOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_capture_node".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Render an individual widget subtree to an image without window \
             chrome; returns a local artifact file URI by default (inline \
             base64 only when include_base64=true)."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::artifact_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for CaptureNodeTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        if param.node_id.trim().is_empty() {
            return Err(McpError::InvalidParameter(
                "`node_id` must not be empty".to_string(),
            ));
        }
        let format = param
            .format
            .as_deref()
            .unwrap_or("png")
            .to_ascii_lowercase();
        if !CAPTURE_FORMATS.contains(&format.as_str()) {
            return Err(McpError::InvalidParameter(format!(
                "format `{format}` is invalid; expected `png` or `jpeg`"
            )));
        }
        let scale = param.scale.unwrap_or(1.0);
        if !scale.is_finite() || scale <= 0.0 {
            return Err(McpError::InvalidParameter(format!(
                "scale must be a positive finite number, got {scale}"
            )));
        }
        let include_base64 = param.include_base64.unwrap_or(false);

        // Live tool (spec §3.18): rasterization happens inside the dev app.
        let result = service.live_call(
            "capture_node",
            serde_json::json!({
                "node_id": param.node_id,
                "format": format,
                "scale": scale,
                "include_base64": include_base64,
            }),
        )?;

        // Logical/physical dims, with cross-derivation when only one side is
        // reported (`physical = logical * scale`).
        let logical = parse_f32_pair(result.get("logical_size"));
        let physical = parse_u32_pair(result.get("physical_size"));
        let logical_size = logical.unwrap_or_else(|| {
            physical.map_or([0.0, 0.0], |[w, h]| {
                [(w as f64 / scale) as f32, (h as f64 / scale) as f32]
            })
        });
        let physical_size = physical.unwrap_or_else(|| {
            logical.map_or([0, 0], |[w, h]| {
                [
                    (f64::from(w) * scale).round().max(0.0) as u32,
                    (f64::from(h) * scale).round().max(0.0) as u32,
                ]
            })
        });

        // Inline base64 payloads are recognised under several common keys —
        // `bytes_base64` is the wire name the dev-session probe emits.
        let inline_b64 = [
            "bytes_base64",
            "base64",
            "data_base64",
            "image_base64",
            "image",
            "data",
        ]
        .iter()
        .find_map(|key| result.get(key).and_then(serde_json::Value::as_str));
        let raw_bytes = result
            .get("bytes")
            .and_then(|v| serde_json::from_value::<Vec<u8>>(v.clone()).ok());
        let decoded = match (raw_bytes, inline_b64) {
            (Some(bytes), _) => Some(bytes),
            (None, Some(b64)) => Some(decode_base64(b64)?),
            (None, None) => None,
        };

        // Persist the artifact: either the app already wrote a file and
        // reported `path`, or we received image bytes to write ourselves.
        let artifact_path = if let Some(bytes) = &decoded {
            let captures_dir = service.offline().captures_dir();
            std::fs::create_dir_all(&captures_dir).map_err(McpError::Io)?;
            let path = captures_dir.join(format!(
                "{}.{format}",
                sanitize_file_component(&param.node_id)
            ));
            std::fs::write(&path, bytes).map_err(McpError::Io)?;
            path
        } else if let Some(path) = result.get("path").and_then(serde_json::Value::as_str) {
            std::path::PathBuf::from(path)
        } else {
            return Err(McpError::RenderError(
                "`capture_node` response contained neither image bytes/base64 \
                 nor an artifact path"
                    .to_string(),
            ));
        };
        let artifact_uri = format!("file://{}", artifact_path.display());

        // Only embed base64 when explicitly requested (spec §3.18 efficiency
        // invariant); never synthesize it for file-path payloads we did not
        // read back.
        let base64 = if include_base64 {
            inline_b64
                .map(str::to_string)
                .or_else(|| decoded.as_deref().map(encode_base64))
                .or_else(|| {
                    std::fs::read(&artifact_path)
                        .ok()
                        .map(|b| encode_base64(&b))
                })
        } else {
            None
        };

        Ok(CaptureNodeOutput {
            artifact_uri,
            logical_size,
            physical_size,
            base64,
        })
    }
}

/// Parameters for `martensite_render_headless`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RenderHeadlessParams {
    /// Target package or example name (`^[a-zA-Z0-9_-]+$`).
    pub crate_target: String,
    /// `[width, height]` viewport in physical pixels.
    #[schemars(schema_with = "crate::types::schema_strip::u32p")]
    pub viewport_size: [u32; 2],
    /// Advance the virtual clock by N ms before capture.
    #[schemars(schema_with = "crate::types::schema_strip::opt_u64")]
    pub virtual_time_ms: Option<u64>,
}

/// Structured result of `martensite_render_headless`.
///
/// # Examples
///
/// ```
/// use martensite_mcp::tools::visual::RenderHeadlessOutput;
///
/// let out = RenderHeadlessOutput {
///     artifact_path: "/tmp/martensite/captures/demo.png".to_string(),
///     paint_audit: Some("0 findings".to_string()),
///     diff: None,
///     success: true,
///     stdout_tail: "done".to_string(),
///     stderr_tail: String::new(),
/// };
/// assert!(out.success);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct RenderHeadlessOutput {
    /// Absolute path of the golden-frame artifact
    /// (`<captures_dir>/<crate_target>.png`).
    pub artifact_path: String,
    /// Paint-audit summary emitted by the harness, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint_audit: Option<String>,
    /// Perceptual diff report against a reference frame, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Whether the harness exited successfully and produced the artifact.
    pub success: bool,
    /// Tail of the harness stdout (bounded to the last ~4 KiB).
    pub stdout_tail: String,
    /// Tail of the harness stderr (bounded to the last ~4 KiB).
    pub stderr_tail: String,
}

/// `martensite_render_headless`: headless golden-frame rendering under
/// `VirtualClock`, spawned via direct `Command` (never a shell).
pub struct RenderHeadlessTool;

impl ToolBase for RenderHeadlessTool {
    type Parameter = RenderHeadlessParams;
    type Output = RenderHeadlessOutput;
    type Error = McpError;

    fn name() -> Cow<'static, str> {
        "martensite_render_headless".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            "Spin up a headless Martensite harness and render a widget to a \
             golden frame under VirtualClock; returns the artifact path, \
             paint audit, and perceptual diff when a reference exists."
                .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        crate::tools::artifact_annotations()
    }
}

impl AsyncTool<MartensiteMcp> for RenderHeadlessTool {
    async fn invoke(
        service: &MartensiteMcp,
        param: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        // Safety gate (spec §3.19): `^[a-zA-Z0-9_-]+$` keeps the target free
        // of separators and shell metacharacters even though no shell is used.
        let valid_target = !param.crate_target.is_empty()
            && param
                .crate_target
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid_target {
            return Err(McpError::InvalidParameter(format!(
                "crate_target `{}` is invalid; must match ^[a-zA-Z0-9_-]+$",
                param.crate_target
            )));
        }

        let captures_dir = service.offline().captures_dir();
        std::fs::create_dir_all(&captures_dir).map_err(McpError::Io)?;
        let artifact = captures_dir.join(format!("{}.png", param.crate_target));
        let [width, height] = param.viewport_size;

        // Offline-capable (spec §3.19): segregated arguments, direct
        // `Command` spawn — never routed through a shell interpreter.
        let mut cmd = tokio::process::Command::new("cargo");
        cmd.arg("run")
            .arg("-p")
            .arg(&param.crate_target)
            .current_dir(service.offline().workspace_root())
            .env("MARTENSITE_HEADLESS", "1")
            .env("MARTENSITE_VIEWPORT", format!("{width}x{height}"))
            .env(
                "MARTENSITE_VIRTUAL_TIME_MS",
                param.virtual_time_ms.unwrap_or(0).to_string(),
            )
            .env("MARTENSITE_CAPTURE_PATH", &artifact)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = cmd.spawn().map_err(|e| {
            McpError::ExecutionFailed(format!(
                "failed to spawn `cargo run -p {}`: {e}",
                param.crate_target
            ))
        })?;
        let output = tokio::time::timeout(HEADLESS_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| {
                McpError::ExecutionFailed(format!(
                    "`cargo run -p {}` timed out after {}s",
                    param.crate_target,
                    HEADLESS_TIMEOUT.as_secs()
                ))
            })?
            .map_err(McpError::Io)?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let success = output.status.success() && artifact.is_file();

        Ok(RenderHeadlessOutput {
            artifact_path: artifact.display().to_string(),
            paint_audit: extract_harness_field(&stdout, "paint_audit"),
            diff: extract_harness_field(&stdout, "diff"),
            success,
            stdout_tail: tail(&stdout, OUTPUT_TAIL_BYTES),
            stderr_tail: tail(&stderr, OUTPUT_TAIL_BYTES),
        })
    }
}

/// Keeps only filesystem-safe characters (`[a-zA-Z0-9._-]`) in a node id used
/// as an artifact file name; everything else becomes `_`.
fn sanitize_file_component(input: &str) -> String {
    let out: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Separators are already replaced, so embedded `..` is inert — but a
    // component of only dots would resolve as a traversal segment.
    if out.is_empty() || out.bytes().all(|b| b == b'.') {
        "node".to_string()
    } else {
        out
    }
}

/// Parses a `[w, h]` pair of floats from a wire array or `{width, height}`
/// object.
fn parse_f32_pair(v: Option<&serde_json::Value>) -> Option<[f32; 2]> {
    match v? {
        serde_json::Value::Array(a) if a.len() >= 2 => {
            Some([a[0].as_f64()? as f32, a[1].as_f64()? as f32])
        }
        serde_json::Value::Object(o) => {
            let w = o.get("width").and_then(serde_json::Value::as_f64)?;
            let h = o.get("height").and_then(serde_json::Value::as_f64)?;
            Some([w as f32, h as f32])
        }
        _ => None,
    }
}

/// Parses a `[w, h]` pair of pixel counts from a wire array or
/// `{width, height}` object.
fn parse_u32_pair(v: Option<&serde_json::Value>) -> Option<[u32; 2]> {
    parse_f32_pair(v).map(|[w, h]| [w.max(0.0) as u32, h.max(0.0) as u32])
}

/// Encodes bytes as standard base64 (RFC 4648, with padding).
fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = b0 << 16 | b1 << 8 | b2;
        out.push(B64_ALPHABET[(n >> 18) as usize & 0x3f] as char);
        out.push(B64_ALPHABET[(n >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[(n >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[n as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

/// Decodes a standard base64 string, tolerating surrounding whitespace and
/// missing padding.
fn decode_base64(input: &str) -> Result<Vec<u8>, McpError> {
    fn value_of(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let cleaned: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(cleaned.len() / 4 * 3 + 3);
    for chunk in cleaned.chunks(4) {
        if chunk.len() == 1 {
            return Err(McpError::RenderError(
                "malformed base64 capture payload (dangling trailing character)".to_string(),
            ));
        }
        let mut acc: u32 = 0;
        for (i, &c) in chunk.iter().enumerate() {
            let v = value_of(c).ok_or_else(|| {
                McpError::RenderError(format!(
                    "invalid base64 character `{}` in capture payload",
                    c as char
                ))
            })?;
            acc |= u32::from(v) << (18 - 6 * i);
        }
        let bytes = [(acc >> 16) as u8, (acc >> 8) as u8, acc as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    Ok(out)
}

/// Extracts a harness-emitted field (`paint_audit`, `diff`) from captured
/// stdout: the last JSON-object line carrying the key wins, falling back to
/// a case-insensitive `KEY: value` line.
fn extract_harness_field(output: &str, key: &str) -> Option<String> {
    let json_hit = output.lines().rev().find_map(|line| {
        let line = line.trim();
        if !line.starts_with('{') {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        v.get(key).and_then(|f| match f {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Null => None,
            other => Some(other.to_string()),
        })
    });
    if json_hit.is_some() {
        return json_hit;
    }
    let prefix = format!("{key}:");
    output.lines().rev().find_map(|line| {
        let line = line.trim();
        let head = line.get(..prefix.len())?;
        head.eq_ignore_ascii_case(&prefix)
            .then(|| line[prefix.len()..].trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

/// Returns the last `max_bytes` of `text`, cut on a UTF-8 boundary.
fn tail(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::McpServerOptions;

    #[tokio::test]
    async fn capture_rejects_bad_format() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let params = CaptureNodeParams {
            node_id: "root".to_string(),
            format: Some("tiff".to_string()),
            ..CaptureNodeParams::default()
        };
        let res = CaptureNodeTool::invoke(&server, params).await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));
    }

    #[tokio::test]
    async fn capture_rejects_non_positive_scale() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        for scale in [0.0, -1.5, f64::NAN, f64::INFINITY] {
            let params = CaptureNodeParams {
                node_id: "root".to_string(),
                scale: Some(scale),
                ..CaptureNodeParams::default()
            };
            let res = CaptureNodeTool::invoke(&server, params).await;
            assert!(
                matches!(res, Err(McpError::InvalidParameter(_))),
                "scale {scale} should be rejected"
            );
        }
    }

    #[tokio::test]
    async fn capture_rejects_empty_node_id() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        let params = CaptureNodeParams {
            node_id: "   ".to_string(),
            ..CaptureNodeParams::default()
        };
        let res = CaptureNodeTool::invoke(&server, params).await;
        assert!(matches!(res, Err(McpError::InvalidParameter(_))));
    }

    #[tokio::test]
    async fn headless_rejects_shell_metachar_target() {
        let server = MartensiteMcp::new(McpServerOptions::offline());
        for bad in ["demo;rm -rf /", "../escape", "a b", "x$HOME", ""] {
            let params = RenderHeadlessParams {
                crate_target: bad.to_string(),
                viewport_size: [800, 600],
                virtual_time_ms: None,
            };
            let res = RenderHeadlessTool::invoke(&server, params).await;
            assert!(
                matches!(res, Err(McpError::InvalidParameter(_))),
                "target `{bad}` should be rejected"
            );
        }
    }

    #[test]
    fn base64_round_trip() {
        let data: Vec<u8> = (0u8..=255).collect();
        let encoded = encode_base64(&data);
        let decoded = decode_base64(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn base64_decodes_without_padding() {
        assert_eq!(decode_base64("aGk").expect("decode"), b"hi");
        assert_eq!(decode_base64("aGVsbG8=").expect("decode"), b"hello");
    }

    #[test]
    fn sanitizer_strips_path_separators() {
        assert_eq!(sanitize_file_component("a/b/../c"), "a_b_.._c");
        assert_eq!(sanitize_file_component(""), "node");
    }

    #[test]
    fn harness_field_extraction() {
        let stdout =
            "noise\n{\"paint_audit\": \"clean\", \"diff\": null}\nmore\nDIFF: 2% pixels differ\n";
        assert_eq!(
            extract_harness_field(stdout, "paint_audit"),
            Some("clean".to_string())
        );
        assert_eq!(
            extract_harness_field(stdout, "diff"),
            Some("2% pixels differ".to_string())
        );
    }

    #[test]
    fn tail_respects_boundaries() {
        let text = "é".repeat(3000);
        let t = tail(&text, 100);
        assert!(t.len() <= 100);
        assert_eq!(std::str::from_utf8(t.as_bytes()).expect("utf8"), t);
    }
}

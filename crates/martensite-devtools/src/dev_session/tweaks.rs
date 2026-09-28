//! Tweak/theme handlers: list/set tweaks, theme overrides, and guarded
//! source write-back with optimistic revision locking.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{DevSession, SessionResult};
use crate::tweak::{SourcePatch, TweakEntry, TweakValue};

/// Context lines emitted around each changed line in a `tweaks_sync` diff.
const DIFF_CONTEXT: usize = 3;

impl DevSession {
    /// `tweaks_list` — registered tweaks with values, source spans, dirty flag.
    pub fn tweaks_list(&self, params: &Value) -> SessionResult {
        let _ = params;
        let registry = self.tweaks.lock().expect("tweaks mutex");
        let mut entries = registry.all_entries();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let tweaks: Vec<Value> = entries.iter().map(entry_to_json).collect();
        Ok(json!({
            "tweaks": tweaks,
            "revision": self.current_revision(),
        }))
    }

    /// `tweak_set` — apply a live parameter override (`{name, value}`).
    pub fn tweak_set(&self, params: &Value) -> SessionResult {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing `name` parameter".to_string())?;
        let value = params
            .get("value")
            .ok_or_else(|| "missing `value` parameter".to_string())?;

        let registry = self.tweaks.lock().expect("tweaks mutex");
        let entry = registry
            .get_entry(name)
            .ok_or_else(|| format!("unknown tweak `{name}`"))?;
        let new_value = coerce_tweak_value(value, &entry.current_value).ok_or_else(|| {
            format!(
                "type mismatch for tweak `{name}`: expected {}",
                entry.current_value.type_name()
            )
        })?;
        registry.set_value(name, new_value);
        let revision = self.bump_revision();
        Ok(json!({
            "applied": true,
            "name": name,
            "revision": revision,
        }))
    }

    /// `theme_set` — switch theme mode and/or override design tokens.
    ///
    /// Theme state lives in the running app, so this delegates to the
    /// session's [`ArenaProbe`](super::ArenaProbe).
    pub fn theme_set(&self, params: &Value) -> SessionResult {
        self.probe
            .lock()
            .expect("probe mutex")
            .probe_theme_apply(params)
    }

    /// `theme_get` — current theme mode and effective tokens.
    pub fn theme_get(&self, params: &Value) -> SessionResult {
        let _ = params;
        self.probe.lock().expect("probe mutex").probe_theme_tokens()
    }

    /// `tweaks_sync` — write tweak overrides back to source files.
    ///
    /// Honors `dry_run` (preview-only by default), `confirmed` (mandatory
    /// disk-write gate), and `expected_revision` (optimistic lock →
    /// `revision_conflict` in the result). For confirmed writes the MCP
    /// layer first issues a `dry_run: true` preflight, confines every
    /// reported `files` path to the workspace, and only then dispatches
    /// the write pinned to the preflight's `revision`; this handler just
    /// splices the recorded span lines.
    pub fn tweaks_sync(&self, params: &Value) -> SessionResult {
        // Optimistic lock: a stale token short-circuits before any work.
        if let Some(expected) = params.get("expected_revision") {
            let expected = parse_revision(expected)
                .ok_or_else(|| format!("invalid `expected_revision`: {expected}"))?;
            let actual = self.current_revision();
            if expected != actual {
                return Ok(json!({
                    "revision_conflict": true,
                    "actual_revision": actual.to_string(),
                    "revision": actual,
                    "applied": false,
                }));
            }
        }

        let dry_run = params
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let confirmed = params
            .get("confirmed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !dry_run && !confirmed {
            return Err("unconfirmed mutation: set confirmed:true to write".to_string());
        }

        let names: Vec<String> = params
            .get("names")
            .or_else(|| params.get("tweak_names"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let entries = self.tweaks.lock().expect("tweaks mutex").all_entries();

        // Select the target entries: the requested names, or every dirty
        // tweak when `names` is absent/empty.
        let mut selected: Vec<&TweakEntry> = Vec::new();
        let mut skipped: Vec<Value> = Vec::new();
        if names.is_empty() {
            selected.extend(entries.iter().filter(|e| e.is_modified()));
        } else {
            for name in &names {
                match entries.iter().find(|e| e.name == *name) {
                    Some(entry) => selected.push(entry),
                    None => skipped.push(skip_json(name, "unknown tweak")),
                }
            }
        }

        // Compile the source patches; entries lacking callsite spans cannot
        // be written back and are reported in `skipped`.
        let mut by_file: BTreeMap<String, Vec<(String, SourcePatch)>> = BTreeMap::new();
        for entry in selected {
            if !entry.is_modified() {
                skipped.push(skip_json(&entry.name, "not modified"));
                continue;
            }
            let Some(span) = entry.source_span.clone() else {
                skipped.push(skip_json(&entry.name, "no source span recorded"));
                continue;
            };
            let patch = SourcePatch::new(
                span,
                entry.property_or_inferred(),
                entry.default_value.format_value(),
                entry.current_value.format_value(),
            );
            by_file
                .entry(patch.span.file.clone())
                .or_default()
                .push((entry.name.clone(), patch));
        }

        // Splice every patch into its file, collecting per-file diffs and
        // the staged writes. Reads never mutate; disk writes happen only
        // after the confirmed-write gate below.
        let mut files: Vec<Value> = Vec::new();
        let mut combined_diff = String::new();
        let mut pending_writes: Vec<(PathBuf, String)> = Vec::new();
        for (file, patches) in &by_file {
            let path = PathBuf::from(file);
            let original =
                fs::read_to_string(&path).map_err(|e| format!("io: reading {file}: {e}"))?;
            let mut lines: Vec<String> = original.split('\n').map(str::to_string).collect();

            let mut applied_lines: Vec<u32> = Vec::new();
            for (name, patch) in patches {
                match apply_patch(&mut lines, patch) {
                    Ok(()) => applied_lines.push(patch.span.line),
                    Err(reason) => skipped.push(skip_json(name, &reason)),
                }
            }
            if applied_lines.is_empty() {
                continue;
            }

            let rewritten = lines.join("\n");
            let diff = unified_diff(file, &original, &rewritten);
            combined_diff.push_str(&diff);
            files.push(json!({
                "path": file,
                "span_start": applied_lines.iter().min().copied().unwrap_or(0),
                "span_end": applied_lines.iter().max().copied().unwrap_or(0),
                "diff": diff,
            }));
            pending_writes.push((path, rewritten));
        }

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "applied": false,
                "diff": combined_diff,
                "files": files,
                "skipped": skipped,
                "revision": self.current_revision(),
            }));
        }

        // Confirmed write: commit each staged file atomically (sibling
        // tempfile + rename), preserving all content outside the patched
        // span lines.
        for (path, content) in &pending_writes {
            write_atomic(path, content)
                .map_err(|e| format!("io: writing {}: {e}", path.display()))?;
        }

        Ok(json!({
            "dry_run": false,
            "applied": true,
            "diff": combined_diff,
            "files": files,
            "skipped": skipped,
            "revision": self.bump_revision(),
        }))
    }
}

/// Serializes one registry entry into the `tweaks_list` wire shape.
fn entry_to_json(entry: &TweakEntry) -> Value {
    let source = entry.source_span.as_ref().map(|span| {
        json!({
            "file": span.file,
            "line_start": span.line,
            "line_end": span.line,
        })
    });
    let (source_file, source_line) = entry
        .source_span
        .as_ref()
        .map(|s| (Value::from(s.file.as_str()), Value::from(s.line)))
        .unwrap_or((Value::Null, Value::Null));
    json!({
        "name": entry.name,
        "value": tweak_value_to_json(&entry.current_value),
        // Aliases matching the `martensite-mcp` `TweakDescriptor` schema.
        "current_value": tweak_value_to_json(&entry.current_value),
        "modified": entry.is_modified(),
        "kind": entry.current_value.type_name(),
        "source": source,
        "source_file": source_file,
        "source_line": source_line,
        "dirty": entry.is_modified(),
    })
}

/// Serializes a [`TweakValue`] into its wire JSON form.
fn tweak_value_to_json(value: &TweakValue) -> Value {
    match value {
        TweakValue::F32(v, _) => json!(v),
        TweakValue::F64(v, _) => json!(v),
        TweakValue::I32(v) => json!(v),
        TweakValue::U32(v) => json!(v),
        TweakValue::Bool(v) => json!(v),
        TweakValue::Color(_) => json!(value.format_value()),
        TweakValue::String(s) => json!(s),
    }
}

/// Converts a JSON `value` into a [`TweakValue`] matching the variant
/// registered for `hint`, or `None` when the JSON shape does not fit.
fn coerce_tweak_value(value: &Value, hint: &TweakValue) -> Option<TweakValue> {
    match hint {
        TweakValue::F32(..) => value.as_f64().map(|v| TweakValue::f32(v as f32)),
        TweakValue::F64(..) => value.as_f64().map(TweakValue::f64),
        TweakValue::I32(..) => value
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .map(TweakValue::i32),
        TweakValue::U32(..) => value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .map(TweakValue::u32),
        TweakValue::Bool(..) => value.as_bool().map(TweakValue::bool),
        TweakValue::Color(..) => coerce_color(value),
        TweakValue::String(..) => value.as_str().map(TweakValue::string),
    }
}

/// Parses a JSON color literal (`"#rrggbb[aa]"` string or `[r,g,b,a?]`
/// byte array) into a [`TweakValue::Color`].
fn coerce_color(value: &Value) -> Option<TweakValue> {
    match value {
        Value::String(s) => TweakValue::color_hex(s),
        Value::Array(items) if (3..=4).contains(&items.len()) => {
            let mut rgba = [0u8; 4];
            rgba[3] = 255;
            for (i, channel) in items.iter().enumerate() {
                rgba[i] = u8::try_from(channel.as_u64()?).ok()?;
            }
            Some(TweakValue::Color(rgba))
        }
        _ => None,
    }
}

/// Parses an `expected_revision` token (u64 number or decimal string).
fn parse_revision(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

/// Builds a `skipped` entry for the `tweaks_sync` wire result.
fn skip_json(name: &str, reason: &str) -> Value {
    json!({ "name": name, "reason": reason })
}

/// Splices a [`SourcePatch`] into `lines` (0-indexed file lines): on the
/// span's line, the first `.prop(old)` occurrence becomes `.prop(new)`.
/// Returns `Err(reason)` when the span is out of range or the compiled-in
/// literal no longer matches the source text.
fn apply_patch(lines: &mut [String], patch: &SourcePatch) -> Result<(), String> {
    if patch.span.line == 0 {
        return Err(format!("invalid span line 0 in {}", patch.span.file));
    }
    let index = (patch.span.line - 1) as usize;
    let Some(line) = lines.get_mut(index) else {
        return Err(format!("span {} is out of range", patch.span.file_line()));
    };
    let needle = format!(".{}({})", patch.method_or_prop, patch.old_value);
    let replacement = format!(".{}({})", patch.method_or_prop, patch.new_value);
    if line.contains(&needle) {
        *line = line.replacen(&needle, &replacement, 1);
        Ok(())
    } else {
        Err(format!(
            "pattern `{needle}` not found at {}",
            patch.span.display()
        ))
    }
}

/// Emits a unified diff (`--- a/…` / `+++ b/…` hunks) for a line-preserving
/// rewrite — tweak write-back only substitutes literals within a line.
fn unified_diff(path: &str, old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.split('\n').collect();
    let new_lines: Vec<&str> = new.split('\n').collect();
    let len = old_lines.len().min(new_lines.len());
    let changed: Vec<usize> = (0..len).filter(|&i| old_lines[i] != new_lines[i]).collect();
    if changed.is_empty() {
        return String::new();
    }

    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    let mut i = 0;
    while i < changed.len() {
        let hunk_first = changed[i];
        let mut hunk_last = hunk_first;
        let mut j = i;
        while j + 1 < changed.len() && changed[j + 1] <= hunk_last + 2 * DIFF_CONTEXT + 1 {
            j += 1;
            hunk_last = changed[j];
        }
        let start = hunk_first.saturating_sub(DIFF_CONTEXT);
        let end = (hunk_last + DIFF_CONTEXT + 1).min(len);
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            start + 1,
            end - start,
            start + 1,
            end - start
        ));
        for k in start..end {
            if old_lines[k] == new_lines[k] {
                out.push(' ');
                out.push_str(old_lines[k]);
            } else {
                out.push('-');
                out.push_str(old_lines[k]);
                out.push('\n');
                out.push('+');
                out.push_str(new_lines[k]);
            }
            out.push('\n');
        }
        i = j + 1;
    }
    out
}

/// Sibling temp-file path for an atomic write of `target`.
fn temp_sibling(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    target.with_file_name(format!(".{name}.tweaks-sync-{}.tmp", std::process::id()))
}

/// Writes `content` to `path` atomically: a sibling tempfile is written,
/// flushed, and renamed over the target so readers never observe a
/// truncated file.
fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = temp_sibling(path);
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev_session::NullProbe;
    use crate::tweak::SourceSpan;

    fn session() -> DevSession {
        DevSession::new(Box::new(NullProbe))
    }

    /// Writes `body` to a unique temp file and returns its path.
    fn temp_source(tag: &str, body: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "martensite-tweaks-sync-{tag}-{}-{unique}.rs",
            std::process::id()
        ));
        fs::write(&path, body).expect("write temp source");
        path
    }

    /// Registers `ui/pad` (f32 default 12.0) bound to `file` line 2, then
    /// bumps it to 16.0 through `tweak_set` so it is dirty.
    fn dirty_padding(session: &DevSession, file: &Path) {
        let span = SourceSpan::new(file.to_string_lossy().into_owned(), 2, 5);
        session
            .tweaks
            .lock()
            .expect("tweaks mutex")
            .register_or_get_with_span("ui/pad", 12.0f32, span, "padding");
        let res = session.tweak_set(&json!({"name": "ui/pad", "value": 16.0}));
        assert!(res.expect("tweak_set")["applied"]
            .as_bool()
            .unwrap_or(false));
    }

    #[test]
    fn tweak_set_applies_and_bumps_revision() {
        let session = session();
        session
            .tweaks
            .lock()
            .expect("tweaks mutex")
            .register_or_get("ui/pad", 12.0f32);
        assert_eq!(session.current_revision(), 0);

        let res = session
            .tweak_set(&json!({"name": "ui/pad", "value": 16.0}))
            .expect("tweak_set");
        assert_eq!(res["applied"], json!(true));
        assert_eq!(res["name"], json!("ui/pad"));
        assert_eq!(res["revision"], json!(1));
        assert_eq!(session.current_revision(), 1);
        assert_eq!(
            session
                .tweaks
                .lock()
                .expect("tweaks mutex")
                .get::<f32>("ui/pad"),
            Some(16.0)
        );
    }

    #[test]
    fn tweak_set_unknown_tweak_errors() {
        let session = session();
        let err = session
            .tweak_set(&json!({"name": "nope", "value": 1.0}))
            .expect_err("must fail");
        assert_eq!(err, "unknown tweak `nope`");
        assert_eq!(session.current_revision(), 0);
    }

    #[test]
    fn tweaks_list_reports_registered_entries() {
        let session = session();
        {
            let registry = session.tweaks.lock().expect("tweaks mutex");
            let span = SourceSpan::new("src/ui.rs", 10, 5);
            registry.register_or_get_with_span("ui/pad", 12.0f32, span, "padding");
            registry.register_or_get("ui/flag", true);
        }
        let res = session.tweaks_list(&json!({})).expect("tweaks_list");
        assert_eq!(res["revision"], json!(0));
        let tweaks = res["tweaks"].as_array().expect("tweaks array");
        assert_eq!(tweaks.len(), 2);
        assert_eq!(tweaks[0]["name"], json!("ui/flag"));
        assert_eq!(tweaks[0]["kind"], json!("bool"));
        assert_eq!(tweaks[0]["dirty"], json!(false));
        assert_eq!(tweaks[0]["source"], Value::Null);
        assert_eq!(tweaks[1]["name"], json!("ui/pad"));
        assert_eq!(tweaks[1]["source"]["file"], json!("src/ui.rs"));
        assert_eq!(tweaks[1]["source"]["line_start"], json!(10));
    }

    #[test]
    fn tweaks_sync_reports_revision_conflict() {
        let session = session();
        let file = temp_source("conflict", "fn view() {\n    .padding(12.0)\n}\n");
        dirty_padding(&session, &file);

        let res = session
            .tweaks_sync(&json!({"expected_revision": "99", "confirmed": true, "dry_run": false}))
            .expect("conflict is a result, not an error");
        assert_eq!(res["revision_conflict"], json!(true));
        assert_eq!(res["actual_revision"], json!("1"));
        assert_eq!(res["applied"], json!(false));

        let _ = fs::remove_file(&file);
    }

    #[test]
    fn tweaks_sync_dry_run_emits_diff_without_writing() {
        let session = session();
        let body = "fn view() {\n    .padding(12.0)\n}\n";
        let file = temp_source("dry-run", body);
        dirty_padding(&session, &file);

        let res = session.tweaks_sync(&json!({})).expect("dry run");
        assert_eq!(res["dry_run"], json!(true));
        assert_eq!(res["applied"], json!(false));
        assert_eq!(res["revision"], json!(1));
        let diff = res["diff"].as_str().expect("diff text");
        assert!(diff.contains(".padding(12.0)"), "diff: {diff}");
        assert!(diff.contains(".padding(16.0)"), "diff: {diff}");
        let files = res["files"].as_array().expect("files array");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["span_start"], json!(2));
        assert_eq!(files[0]["span_end"], json!(2));
        assert_eq!(
            fs::read_to_string(&file).expect("read back"),
            body,
            "dry run must not write"
        );

        let _ = fs::remove_file(&file);
    }

    #[test]
    fn tweaks_sync_unconfirmed_write_is_rejected() {
        let session = session();
        let body = "fn view() {\n    .padding(12.0)\n}\n";
        let file = temp_source("unconfirmed", body);
        dirty_padding(&session, &file);

        let err = session
            .tweaks_sync(&json!({"dry_run": false}))
            .expect_err("unconfirmed write must fail");
        assert_eq!(err, "unconfirmed mutation: set confirmed:true to write");
        assert_eq!(fs::read_to_string(&file).expect("read back"), body);

        let _ = fs::remove_file(&file);
    }

    #[test]
    fn tweaks_sync_confirmed_write_splices_source() {
        let session = session();
        let body = "fn view() {\n    .padding(12.0)\n}\n";
        let file = temp_source("confirmed", body);
        dirty_padding(&session, &file);
        let before = session.current_revision();

        let res = session
            .tweaks_sync(&json!({
                "names": ["ui/pad"],
                "dry_run": false,
                "confirmed": true,
                "expected_revision": before,
            }))
            .expect("confirmed write");
        assert_eq!(res["applied"], json!(true));
        assert_eq!(res["revision"], json!(before + 1));
        assert_eq!(
            fs::read_to_string(&file).expect("read back"),
            "fn view() {\n    .padding(16.0)\n}\n"
        );

        let _ = fs::remove_file(&file);
    }

    #[test]
    fn tweaks_sync_skips_entries_without_spans() {
        let session = session();
        session
            .tweaks
            .lock()
            .expect("tweaks mutex")
            .register_or_get("ui/orphan", 4.0f32);
        session
            .tweak_set(&json!({"name": "ui/orphan", "value": 8.0}))
            .expect("tweak_set");

        let res = session.tweaks_sync(&json!({})).expect("dry run");
        let skipped = res["skipped"].as_array().expect("skipped array");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0]["name"], json!("ui/orphan"));
        assert!(res["files"].as_array().expect("files").is_empty());
    }

    #[test]
    fn theme_calls_delegate_to_probe() {
        let session = session();
        let err = session
            .theme_set(&json!({"mode": "dark"}))
            .expect_err("NullProbe has no theme");
        assert!(err.starts_with("not_implemented:"), "{err}");
        let err = session.theme_get(&json!({})).expect_err("no theme tokens");
        assert!(err.starts_with("not_implemented:"), "{err}");
    }
}

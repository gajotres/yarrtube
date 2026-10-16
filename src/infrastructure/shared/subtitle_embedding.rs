use crate::domain::video::subtitle_sidecar::subtitle_suffix;
use anyhow::{Context, Result, bail};
use isolang::Language;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Embed the final, retimed SRTs, removing sidecars only after successful placement. Native yt-dlp
/// embedding is deliberately disabled because it runs before our timing repair.
pub fn embed_subtitles(
    media: &Path,
    requested: Option<&str>,
    manual: Option<&str>,
    creator_enabled: bool,
) -> Result<()> {
    embed_with(
        Path::new("ffmpeg"),
        media,
        requested,
        manual,
        creator_enabled,
    )
}

struct Track {
    path: PathBuf,
    title: String,
    language: String,
}

fn dictionary(raw: Option<&str>) -> Result<Map<String, Value>> {
    match raw.map(str::trim) {
        None | Some("NA" | "null" | "") => Ok(Map::new()),
        Some(raw) => serde_json::from_str(raw).context("invalid subtitle track metadata"),
    }
}

fn track_label(code: &str, metadata: &Value, generated: bool) -> (String, String) {
    let base = code
        .split(['-', '_'])
        .next()
        .unwrap_or(code)
        .to_ascii_lowercase();
    let language = Language::from_639_1(&base).or_else(|| Language::from_639_3(&base));
    let name = language
        .map(|language| language.to_name())
        .or_else(|| metadata["name"].as_str())
        .unwrap_or("Unknown");
    let name = name.replace(['\r', '\n', '\0'], "");
    let title = format!("{name}-{}SRT", if generated { "GENERATED-" } else { "" });
    let code = language
        .map_or("und", |language| language.to_639_3())
        .to_string();
    (title, code)
}

fn embed_with(
    ffmpeg: &Path,
    media: &Path,
    requested: Option<&str>,
    manual: Option<&str>,
    creator_enabled: bool,
) -> Result<()> {
    let parent = media.parent().context("subtitle media has no parent")?;
    let mut sidecars = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file() && subtitle_suffix(media, &path).is_some() {
            sidecars.push(path);
        }
    }
    if sidecars.is_empty() {
        return Ok(());
    }
    sidecars.sort();
    let requested = dictionary(requested)?;
    let manual = dictionary(manual)?;
    let tracks = sidecars
        .into_iter()
        .map(|path| {
            let suffix = subtitle_suffix(media, &path).context("invalid subtitle filename")?;
            let code = suffix
                .trim_start_matches('.')
                .strip_suffix(".srt")
                .context("invalid SRT filename")?;
            let metadata = requested
                .get(code)
                .with_context(|| format!("missing metadata for subtitle language {code}"))?;
            // yt-dlp prefers creator captions for an exact language when requested.
            let generated = !(creator_enabled && manual.contains_key(code));
            let (title, language) = track_label(code, metadata, generated);
            Ok(Track {
                path,
                title,
                language,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let output_path = media.with_file_name(format!(
        "{}.subtitle-embedded.mp4",
        media
            .file_stem()
            .context("missing media filename")?
            .to_string_lossy()
    ));
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(media);
    for track in &tracks {
        cmd.arg("-i").arg(&track.path);
    }
    // Retain video, audio, cover art and metadata; subtitle selection is owned here.
    cmd.args(["-map", "0", "-map", "-0:s?"]);
    for index in 0..tracks.len() {
        cmd.arg("-map").arg(format!("{}:0", index + 1));
    }
    cmd.args([
        "-c",
        "copy",
        "-c:s",
        "mov_text",
        "-disposition:s",
        "0",
        "-map_metadata",
        "0",
        "-map_chapters",
        "0",
        "-movflags",
        "+faststart",
    ]);
    for (index, track) in tracks.iter().enumerate() {
        for (key, value) in [
            ("title", &track.title),
            ("handler_name", &track.title),
            ("language", &track.language),
        ] {
            cmd.arg(format!("-metadata:s:s:{index}"))
                .arg(format!("{key}={value}"));
        }
    }
    cmd.arg(&output_path);
    let result = cmd
        .output()
        .context("failed to start FFmpeg for subtitle embedding")?;
    if !result.status.success() {
        let _ = std::fs::remove_file(&output_path);
        bail!(
            "subtitle embedding failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    std::fs::rename(&output_path, media).context("failed to place subtitle-embedded video")?;
    for track in &tracks {
        match std::fs::remove_file(&track.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to remove embedded subtitle sidecar"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_use_long_language_names_and_generated_marker() {
        assert_eq!(
            track_label("en-orig", &Value::Null, true),
            ("English-GENERATED-SRT".into(), "eng".into())
        );
        assert_eq!(
            track_label("de", &Value::Null, false),
            ("German-SRT".into(), "deu".into())
        );
        assert_eq!(track_label("es-MX", &Value::Null, false).0, "Spanish-SRT");
        assert_eq!(
            track_label(
                "custom",
                &serde_json::json!({"name":"Custom Language"}),
                true
            )
            .0,
            "Custom Language-GENERATED-SRT"
        );
    }

    #[test]
    #[cfg(unix)]
    fn embeds_with_source_labels_and_removes_sidecars_only_on_success() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("id.mp4");
        std::fs::write(&media, "original video").unwrap();
        std::fs::write(dir.path().join("id.en.srt"), "captions").unwrap();
        std::fs::write(dir.path().join("id.de.srt"), "captions").unwrap();
        let ffmpeg = dir.path().join("ffmpeg");
        let captured = dir.path().join("args");
        std::fs::write(&ffmpeg, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nfor output do :; done\ncp '{}' \"$output\"\n", captured.display(), media.display())).unwrap();
        std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
        embed_with(
            &ffmpeg,
            &media,
            Some(r#"{"en":{},"de":{}}"#),
            Some(r#"{"en":[]}"#),
            true,
        )
        .unwrap();
        let args = std::fs::read_to_string(&captured).unwrap();
        assert!(args.contains("handler_name=English-SRT"));
        assert!(args.contains("handler_name=German-GENERATED-SRT"));
        assert!(!dir.path().join("id.en.srt").exists());
        assert!(!dir.path().join("id.de.srt").exists());
        assert_eq!(std::fs::read_to_string(&media).unwrap(), "original video");
        // Creator metadata can be present even when only automatic captions were requested.
        std::fs::write(dir.path().join("id.en.srt"), "captions").unwrap();
        std::fs::write(dir.path().join("id.de.srt"), "captions").unwrap();
        embed_with(
            &ffmpeg,
            &media,
            Some(r#"{"en":{},"de":{}}"#),
            Some(r#"{"en":[]}"#),
            false,
        )
        .unwrap();
        assert!(
            std::fs::read_to_string(&captured)
                .unwrap()
                .contains("handler_name=English-GENERATED-SRT")
        );
        std::fs::write(dir.path().join("id.en.srt"), "captions").unwrap();
        std::fs::write(dir.path().join("id.de.srt"), "captions").unwrap();
        std::fs::write(&ffmpeg, "#!/bin/sh\necho failed >&2\nexit 7\n").unwrap();
        assert!(embed_with(&ffmpeg, &media, Some(r#"{"en":{},"de":{}}"#), None, false).is_err());
        assert!(dir.path().join("id.en.srt").is_file());
        assert!(dir.path().join("id.de.srt").is_file());
        assert_eq!(std::fs::read_to_string(&media).unwrap(), "original video");
    }
}

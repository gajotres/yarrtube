use crate::domain::video_transcript::{CaptionSource, CaptionTrack};
use anyhow::{Context, Result, bail};
use isolang::Language;
use std::path::Path;
use std::process::Command;

/// Called only by the separate upstream subtitle task. All output is written
/// into that task's staging directory and atomically replaces the video on success.
pub fn embed_subtitle(media: &Path, subtitle: &Path, track: &CaptionTrack) -> Result<()> {
    embed_with(Path::new("ffmpeg"), media, subtitle, track)
}

fn track_label(code: &str, generated: bool) -> (String, String) {
    let base = code
        .split(['-', '_'])
        .next()
        .unwrap_or(code)
        .to_ascii_lowercase();
    let language = Language::from_639_1(&base).or_else(|| Language::from_639_3(&base));
    let name = language
        .map(|language| language.to_name())
        .unwrap_or("Unknown");
    let name = name.replace(['\r', '\n', '\0'], "");
    let title = format!("{name}-{}SRT", if generated { "GENERATED-" } else { "" });
    let code = language
        .map_or("und", |language| language.to_639_3())
        .to_string();
    (title, code)
}

fn embed_with(ffmpeg: &Path, media: &Path, subtitle: &Path, track: &CaptionTrack) -> Result<()> {
    let (title, language) = track_label(
        track.language.as_str(),
        track.source == CaptionSource::Automatic,
    );
    let output = subtitle
        .parent()
        .context("subtitle has no staging directory")?
        .join(format!("{}.embedded.mp4", uuid::Uuid::new_v4()));
    tracing::info!(title = %title, language = %language, "Starting separate subtitle embedding");
    let result = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(media)
        .arg("-i")
        .arg(subtitle)
        .args([
            "-map",
            "0",
            "-map",
            "-0:s?",
            "-map",
            "1:0",
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
            "-metadata:s:s:0",
        ])
        .arg(format!("title={title}"))
        .arg("-metadata:s:s:0")
        .arg(format!("handler_name={title}"))
        .arg("-metadata:s:s:0")
        .arg(format!("language={language}"))
        .arg(&output)
        .output()
        .context("failed to start FFmpeg for subtitle embedding")?;
    if !result.status.success() {
        let _ = std::fs::remove_file(&output);
        bail!(
            "subtitle embedding failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    if let Err(error) = std::fs::rename(&output, media) {
        let _ = std::fs::remove_file(&output);
        return Err(error).context("failed to place subtitle-embedded video");
    }
    tracing::info!("Subtitle embedding completed; external SRT retained");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::video_transcript::CaptionLanguage;

    #[test]
    #[ignore = "requires local ffmpeg and ffprobe; exercised explicitly during validation"]
    fn real_ffmpeg_embeds_one_labelled_track_and_keeps_external_srt() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("video.mp4");
        let subtitle = dir.path().join("video.en.srt");
        let result = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x64:r=1:d=2",
                "-c:v",
                "mpeg4",
                "-y",
            ])
            .arg(&media)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        std::fs::write(&subtitle, "1\n00:00:00,000 --> 00:00:01,000\nHello\n").unwrap();
        let track = CaptionTrack {
            key: "en-orig".into(),
            language: CaptionLanguage::new("en").unwrap(),
            source: CaptionSource::Automatic,
        };
        // Repeated processing replaces the subtitle stream instead of duplicating it.
        for _ in 0..2 {
            embed_subtitle(&media, &subtitle, &track).unwrap();
        }
        let output = Command::new("ffprobe")
            .args(["-v", "error", "-show_streams", "-of", "json"])
            .arg(&media)
            .output()
            .unwrap();
        assert!(output.status.success());
        let info: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let streams = info["streams"].as_array().unwrap();
        assert_eq!(
            streams
                .iter()
                .filter(|s| s["codec_type"] == "video")
                .count(),
            1
        );
        let subtitles: Vec<_> = streams
            .iter()
            .filter(|s| s["codec_type"] == "subtitle")
            .collect();
        assert_eq!(subtitles.len(), 1);
        assert_eq!(subtitles[0]["codec_name"], "mov_text");
        assert_eq!(subtitles[0]["tags"]["language"], "eng");
        assert_eq!(
            subtitles[0]["tags"]["handler_name"],
            "English-GENERATED-SRT"
        );
        assert!(
            std::fs::read_to_string(&subtitle)
                .unwrap()
                .contains("Hello")
        );
    }

    #[test]
    fn full_language_and_generated_labels_are_preserved() {
        assert_eq!(
            track_label("en", false),
            ("English-SRT".into(), "eng".into())
        );
        assert_eq!(
            track_label("en", true),
            ("English-GENERATED-SRT".into(), "eng".into())
        );
        assert_eq!(
            track_label("de", true),
            ("German-GENERATED-SRT".into(), "deu".into())
        );
    }

    #[test]
    #[cfg(unix)]
    fn failed_embedding_keeps_the_original_video_and_srt() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("video.mp4");
        let subtitle = dir.path().join("video.en.srt");
        let ffmpeg = dir.path().join("ffmpeg");
        std::fs::write(&media, "original media").unwrap();
        std::fs::write(&subtitle, "original captions").unwrap();
        std::fs::write(
            &ffmpeg,
            r#"#!/bin/sh
for output do :; done
echo partial > "$output"
exit 1
"#,
        )
        .unwrap();
        std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
        let track = CaptionTrack {
            key: "en-orig".into(),
            language: CaptionLanguage::new("en").unwrap(),
            source: CaptionSource::Automatic,
        };
        let error = embed_with(&ffmpeg, &media, &subtitle, &track).unwrap_err();
        assert!(error.to_string().contains("subtitle embedding failed"));
        assert_eq!(std::fs::read_to_string(media).unwrap(), "original media");
        assert_eq!(
            std::fs::read_to_string(subtitle).unwrap(),
            "original captions"
        );
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|e| {
            let path = e.unwrap().path();
            path.extension().is_some_and(|ext| ext == "mp4")
                && path.file_name().unwrap() != "video.mp4"
        }));
    }
}

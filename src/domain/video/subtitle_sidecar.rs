use std::path::{Path, PathBuf};

/// The associated media stem of a language-tagged SRT sidecar. Rejects unrelated
/// files and nested suffixes so one episode cannot claim another's subtitles.
pub fn subtitle_media_stem(path: &Path) -> Option<PathBuf> {
    if path.extension()?.to_str()? != "srt" {
        return None;
    }
    let without_format = path.with_extension("");
    let language = without_format.extension()?.to_str()?;
    if language.is_empty()
        || !language
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return None;
    }
    Some(without_format.with_extension(""))
}

/// The language/format suffix (e.g. `.en.srt`) when the sidecar belongs to media.
pub fn subtitle_suffix(media: &Path, sidecar: &Path) -> Option<String> {
    if subtitle_media_stem(sidecar)? != media.with_extension("") {
        return None;
    }
    Some(
        sidecar
            .file_name()?
            .to_str()?
            .strip_prefix(media.file_stem()?.to_str()?)?
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_episode_stems_and_language_tags() {
        let media = Path::new("Season 1/Episode.mp4");
        for language in ["en", "en-orig", "en-US", "de", "zh_Hans"] {
            let name = format!("Season 1/Episode.{language}.srt");
            assert_eq!(
                subtitle_suffix(media, Path::new(&name)),
                Some(format!(".{language}.srt"))
            );
        }
        for name in [
            "Season 1/Episode 2.en.srt",
            "Season 2/Episode.en.srt",
            "Season 1/Episode.en.srt.part",
            "Season 1/Episode.en.extra.srt",
            "Season 1/Episode.nfo",
            "Season 1/Episode.srt",
        ] {
            assert!(
                subtitle_suffix(media, Path::new(name)).is_none(),
                "matched {name}"
            );
        }
    }
}

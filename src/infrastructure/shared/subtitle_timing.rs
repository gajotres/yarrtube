use crate::domain::video::subtitle_sidecar::subtitle_suffix;
use anyhow::{Context, Result, bail};
use std::path::Path;

/// Rebuild sidecars from yt-dlp's preserved originals: FFmpeg concat can drop
/// sparse subtitle streams. Originals exist only when ModifyChapters cut them.
pub fn repair_cut_subtitles(media: &Path, chapters: Option<&str>) -> Result<()> {
    let parent = media.parent().context("subtitle media has no parent")?;
    let mut originals = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(base) = name.strip_suffix(".uncut.srt") else {
            continue;
        };
        let target = parent.join(format!("{base}.srt"));
        if subtitle_suffix(media, &target).is_some() && entry.file_type()?.is_file() {
            originals.push((entry.path(), target));
        }
    }
    if originals.is_empty() {
        return Ok(());
    }
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(chapters.context("missing SponsorBlock cut data")?)
            .context("invalid SponsorBlock cut data")?;
    let mut cuts = Vec::new();
    for chapter in raw {
        let start = chapter["start_time"].as_f64().context("cut has no start")?;
        let end = chapter["end_time"].as_f64().context("cut has no end")?;
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start || end > 1e12 {
            bail!("invalid SponsorBlock cut range");
        }
        cuts.push((
            (start * 1000.0).round() as i64,
            (end * 1000.0).round() as i64,
        ));
    }
    cuts.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (start, end) in cuts {
        if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    for (source, target) in originals {
        let text = std::fs::read_to_string(&source)?;
        std::fs::write(&target, retime_srt(&text, &merged)?)?;
    }
    Ok(())
}

fn retime_srt(text: &str, cuts: &[(i64, i64)]) -> Result<String> {
    let mut output = String::new();
    let mut index = 0;
    for block in text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .split("\n\n")
    {
        if block.trim().is_empty() {
            continue;
        }
        let mut lines = block.lines();
        let first = lines.next().context("empty subtitle cue")?;
        let timing = if first.contains("-->") {
            first
        } else {
            lines.next().context("missing subtitle timestamp")?
        };
        let (start, end) = timing
            .split_once("-->")
            .context("invalid subtitle timestamp")?;
        let start = parse_time(start.trim())?;
        let end = parse_time(
            end.split_whitespace()
                .next()
                .context("missing subtitle end")?,
        )?;
        if end < start {
            bail!("subtitle ends before it starts");
        }
        let caption = lines.collect::<Vec<_>>().join("\n");
        let mut cursor = start;
        for &(cut_start, cut_end) in cuts {
            if cut_end <= cursor {
                continue;
            }
            if cut_start >= end {
                break;
            }
            if cut_start > cursor {
                append_cue(
                    &mut output,
                    &mut index,
                    cursor,
                    cut_start.min(end),
                    &caption,
                    cuts,
                );
            }
            cursor = cursor.max(cut_end);
            if cursor >= end {
                break;
            }
        }
        if cursor < end {
            append_cue(&mut output, &mut index, cursor, end, &caption, cuts);
        }
    }
    Ok(output)
}

fn append_cue(
    output: &mut String,
    index: &mut usize,
    start: i64,
    end: i64,
    caption: &str,
    cuts: &[(i64, i64)],
) {
    let shifted = |time: i64| {
        time - cuts
            .iter()
            .map(|&(s, e)| (time - s).clamp(0, e - s))
            .sum::<i64>()
    };
    let (start, end) = (shifted(start), shifted(end));
    if start >= end {
        return;
    }
    *index += 1;
    output.push_str(&format!(
        "{}\n{} --> {}\n{}\n\n",
        index,
        format_time(start),
        format_time(end),
        caption
    ));
}

fn parse_time(value: &str) -> Result<i64> {
    let fields: Vec<_> = value.split([':', ',']).collect();
    if fields.len() != 4 {
        bail!("invalid SRT time");
    }
    let numbers = fields
        .iter()
        .map(|part| part.parse::<i64>())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if numbers[0] < 0
        || numbers[0] > 1_000_000
        || !(0..60).contains(&numbers[1])
        || !(0..60).contains(&numbers[2])
        || !(0..1000).contains(&numbers[3])
    {
        bail!("invalid SRT time");
    }
    Ok(((numbers[0] * 60 + numbers[1]) * 60 + numbers[2]) * 1000 + numbers[3])
}

fn format_time(ms: i64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifts_sparse_captions_and_trims_cues_crossing_cuts() {
        let text = "1\r\n00:00:01,000 --> 00:00:05,000\r\nCrossing\r\n\r\n2\r\n00:00:02,500 --> 00:00:03,500\r\nRemoved\r\n\r\n3\r\n00:00:05,000 --> 00:00:07,000\r\nAfter\r\n";
        let result = retime_srt(text, &[(2000, 4000)]).unwrap();
        assert!(result.contains("00:00:03,000 --> 00:00:05,000\nAfter"));
        assert!(!result.contains("Removed"));
        assert!(result.contains("00:00:01,000 --> 00:00:02,000\nCrossing"));
        assert!(result.contains("00:00:02,000 --> 00:00:03,000\nCrossing"));
        assert!(retime_srt("bad cue", &[]).is_err());
    }

    #[test]
    fn repairs_empty_outputs_and_merges_overlapping_cut_ranges() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("id.en.uncut.srt"),
            "1\n00:00:06,000 --> 00:00:08,000\nHello\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("id.en.srt"), "").unwrap();
        repair_cut_subtitles(
            &dir.path().join("id.mp4"),
            Some(r#"[{"start_time":2,"end_time":4},{"start_time":3,"end_time":5}]"#),
        )
        .unwrap();
        assert!(
            std::fs::read_to_string(dir.path().join("id.en.srt"))
                .unwrap()
                .contains("00:00:03,000 --> 00:00:05,000")
        );
        // No originals means no cuts were applied, so missing API metadata is harmless.
        repair_cut_subtitles(&dir.path().join("other.mp4"), None).unwrap();
    }
}

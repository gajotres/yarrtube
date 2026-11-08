use anyhow::{Context, Result, bail};

/// Apply the exact cuts from the saved video to captions fetched later.
/// Rebuild cues directly so FFmpeg concat cannot drop sparse subtitle streams.
pub fn retime_subtitles(text: &str, chapters: &str) -> Result<String> {
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(chapters).context("invalid SponsorBlock cut data")?;
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
    retime_srt(text, &merged)
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
    fn deferred_subtitles_use_the_saved_download_cut_ranges() {
        let original = "1\n00:00:05,000 --> 00:00:07,000\nHello\n";
        let retimed = retime_subtitles(original, r#"[{"start_time":2,"end_time":4}]"#).unwrap();
        assert!(retimed.contains("00:00:03,000 --> 00:00:05,000"));
        assert!(retime_subtitles(original, "invalid").is_err());
        assert!(retime_subtitles(original, r#"[{"start_time":4,"end_time":2}]"#).is_err());
    }
}

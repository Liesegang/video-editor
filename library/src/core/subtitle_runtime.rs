#[derive(Clone, PartialEq, Debug)]
pub struct SubtitleCue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub fn parse_srt(source: &str) -> Result<Vec<SubtitleCue>, String> {
    let normalized = source.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    for (block_index, block) in normalized.split("\n\n").enumerate() {
        let mut lines = block.lines().filter(|line| !line.trim().is_empty());
        let first = match lines.next() {
            Some(line) => line.trim(),
            None => continue,
        };
        let timing = if first.contains("-->") {
            first
        } else {
            lines
                .next()
                .ok_or_else(|| format!("Subtitle block {} has no timing", block_index + 1))?
                .trim()
        };
        let (start, end) = timing
            .split_once("-->")
            .ok_or_else(|| format!("Subtitle block {} has invalid timing", block_index + 1))?;
        let start = parse_timestamp(start.trim())?;
        let end = parse_timestamp(end.split_whitespace().next().unwrap_or_default())?;
        if end <= start {
            return Err(format!(
                "Subtitle block {} has no duration",
                block_index + 1
            ));
        }
        let text = lines.collect::<Vec<_>>().join("\n");
        if text.is_empty() {
            return Err(format!("Subtitle block {} has no text", block_index + 1));
        }
        cues.push(SubtitleCue { start, end, text });
    }
    if cues.is_empty() {
        return Err("SRT contains no subtitle cues".to_string());
    }
    Ok(cues)
}

fn parse_timestamp(value: &str) -> Result<f64, String> {
    let (clock, milliseconds) = value
        .split_once([',', '.'])
        .ok_or_else(|| format!("Invalid SRT timestamp '{value}'"))?;
    let mut clock = clock.split(':');
    let hours = clock.next().and_then(|value| value.parse::<u64>().ok());
    let minutes = clock.next().and_then(|value| value.parse::<u64>().ok());
    let seconds = clock.next().and_then(|value| value.parse::<u64>().ok());
    if clock.next().is_some() || hours.is_none() || minutes.is_none() || seconds.is_none() {
        return Err(format!("Invalid SRT timestamp '{value}'"));
    }
    let fraction = format!("0.{milliseconds}")
        .parse::<f64>()
        .map_err(|_| format!("Invalid SRT timestamp '{value}'"))?;
    Ok(hours.unwrap() as f64 * 3600.0
        + minutes.unwrap() as f64 * 60.0
        + seconds.unwrap() as f64
        + fraction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiline_srt_cues() {
        let cues = parse_srt(
            "1\r\n00:00:01,250 --> 00:00:03,000\r\nFirst\r\nline\r\n\r\n2\r\n00:00:04,000 --> 00:00:05,500\r\nSecond\r\n",
        )
        .expect("SRT");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "First\nline");
        assert_eq!(cues[0].start, 1.25);
        assert_eq!(cues[1].end, 5.5);
    }
}

//! Subtitles of a video (TODO 5.1.7): an `.srt` beside it, or a subtitle track of the file read
//! by `ffmpeg` as SRT. Each cue is its start, its end and its text, cleaned.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// The most read of a subtitle file or track, and the cues kept.
const LARGEST: usize = 4 * 1024 * 1024;
const MOST_CUES: usize = 50_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
}

/// The `.srt` beside `video` (same name), if there is one.
pub fn beside(video: &Path) -> Option<PathBuf> {
    let path = video.with_extension("srt");
    path.is_file().then_some(path)
}

/// The cues of an SRT file; what cannot be read is left out, never a panic.
pub fn parse_srt(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let mut lines = block.lines().skip_while(|line| !line.contains("-->"));
        let Some(timing) = lines.next() else { continue };
        let Some((start, end)) = timing.split_once("-->") else {
            continue;
        };
        let (Some(start), Some(end)) = (
            time(start),
            time(end.split_whitespace().next().unwrap_or("")),
        ) else {
            continue;
        };
        // Tags such as <i> go; control characters too.
        let words: String = lines.collect::<Vec<_>>().join(" ");
        let mut clean = String::new();
        let mut in_tag = false;
        for character in words.chars() {
            match character {
                '<' => in_tag = true,
                '>' if in_tag => in_tag = false,
                _ if in_tag || character.is_control() => {}
                _ => clean.push(character),
            }
        }
        let clean = clean.trim().chars().take(300).collect::<String>();
        if !clean.is_empty() && end >= start {
            cues.push(Cue {
                start,
                end,
                text: clean,
            });
        }
        if cues.len() >= MOST_CUES {
            break;
        }
    }
    cues.sort_by_key(|cue| cue.start);
    cues
}

/// `HH:MM:SS,mmm` (or `.mmm`).
fn time(text: &str) -> Option<Duration> {
    let text = text.trim().replace(',', ".");
    let mut parts = text.split(':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    (minutes < 60 && (0.0..60.0).contains(&seconds) && hours < 1000).then(|| {
        Duration::from_secs(hours * 3600 + minutes * 60) + Duration::from_secs_f64(seconds)
    })
}

/// The text shown at `at`.
pub fn at(cues: &[Cue], at: Duration) -> Option<&str> {
    let index = cues.partition_point(|cue| cue.start <= at);
    cues[..index]
        .iter()
        .rev()
        .take(4)
        .find(|cue| at <= cue.end)
        .map(|cue| cue.text.as_str())
}

/// Reads the `.srt` beside a video, at most `LARGEST`.
pub fn read_file(path: &Path) -> Vec<Cue> {
    if std::fs::metadata(path).is_ok_and(|file| file.len() as usize > LARGEST) {
        return Vec::new();
    }
    std::fs::read(path)
        .map(|bytes| parse_srt(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default()
}

/// Subtitle track `track` of `video` through `ffmpeg`, as SRT: `stdin` null, 20 s at most.
pub fn read_track(video: &Path, track: usize) -> Vec<Cue> {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(video)
        .args(["-map", &format!("0:s:{track}"), "-f", "srt", "-"]);
    crate::process::output(&mut command, Duration::from_secs(20))
        .filter(|output| output.stdout.len() <= LARGEST)
        .map(|output| parse_srt(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_cues_are_read_cleaned_and_found_by_time_and_hostile_ones_are_left_out() {
        let text = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>Hello</i>\r\nthere\r\n\r\n2\r\n00:00:03.000 --> 00:00:04,000 X1:0\r\nBye\u{1b}[31m\r\n\r\nbroken\r\n99:99:99,000 --> 00:00:01,000\r\nno\r\n\r\n3\r\n00:00:09,000 --> 00:00:08,000\r\nbackwards\r\n";
        let cues = parse_srt(text);
        assert_eq!(cues.len(), 2, "{cues:?}");
        assert_eq!(cues[0].text, "Hello there");
        assert_eq!(cues[1].text, "Bye[31m", "no control characters");
        assert_eq!(at(&cues, Duration::from_millis(1500)), Some("Hello there"));
        assert_eq!(at(&cues, Duration::from_millis(2700)), None);
        assert_eq!(at(&cues, Duration::from_secs(3)), Some("Bye[31m"));
        assert!(parse_srt("").is_empty() && parse_srt("-->\n\n-->").is_empty());
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("film.mp4");
        assert_eq!(beside(&video), None);
        std::fs::write(directory.path().join("film.srt"), text).unwrap();
        assert_eq!(read_file(&beside(&video).unwrap()).len(), 2);
    }
}

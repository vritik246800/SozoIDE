//! Videos through the system's `ffmpeg` (F23, Q41): a thread reads its frames, small and in
//! RGB, and the tab shows each when its time comes; `ffplay` plays the sound (Q51). Nothing links
//! FFmpeg into `sozo`; without the command the tab says how to get it. Seeking, the volume and
//! the speed start the processes again from the position on screen.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    thread,
    time::{Duration, Instant},
};

use image::{RgbImage, RgbaImage};

pub const FPS: u32 = 15;
/// The largest frame asked of `ffmpeg`: small frames (Modo 2) are quick to send to a terminal.
const LARGEST: (u32, u32) = (320, 180);
/// The speeds `[` and `]` step through.
const SPEEDS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
const NORMAL_SPEED: usize = 2;
/// The volume `-` and `+` change by, out of 100.
const VOLUME_STEP: u8 = 10;

/// What a key, a button or the wheel asks of a video.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Play or pause; once it ended, play again from the start.
    Play,
    /// Pause at the start.
    Stop,
    /// Seconds forward, or back when negative.
    Seek(i32),
    Quieter,
    Louder,
    Mute,
    Slower,
    Faster,
    /// The app shows the tab over the whole screen.
    Fullscreen,
}

/// What `ffprobe` says of a video: the frame size asked of `ffmpeg`, how long it is and whether
/// it has sound.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Info {
    size: (u32, u32),
    duration: Duration,
    sound: bool,
}

pub struct Video {
    path: PathBuf,
    info: Option<Info>,
    frames: Option<Receiver<RgbaImage>>,
    child: Option<Child>,
    /// `ffplay`, while the sound plays.
    sound: Option<Child>,
    pub frame: Option<RgbaImage>,
    /// Where `ffmpeg` started, when, and the frames shown since: frame `n` is due `n / FPS`
    /// after `started`.
    from: Duration,
    started: Instant,
    shown: u32,
    pub paused: bool,
    pub ended: bool,
    /// Why it does not play: no `ffmpeg`, or a file it cannot read.
    pub error: Option<String>,
    /// Out of 100.
    pub volume: u8,
    pub muted: bool,
    speed: usize,
    /// Where the mouse drags the progress bar to, from 0 to 1, before it lets go.
    pub scrub: Option<f32>,
}

impl Video {
    pub fn open(path: &Path) -> Self {
        let mut video = Self::new(path, None);
        match probe(path) {
            Ok(info) => {
                video.info = Some(info);
                video.start(Duration::ZERO);
            }
            Err(error) => video.error = Some(error),
        }
        video
    }

    fn new(path: &Path, info: Option<Info>) -> Self {
        Self {
            path: path.to_path_buf(),
            info,
            frames: None,
            child: None,
            sound: None,
            frame: None,
            from: Duration::ZERO,
            started: Instant::now(),
            shown: 0,
            paused: false,
            ended: false,
            error: None,
            volume: 100,
            muted: false,
            speed: NORMAL_SPEED,
            scrub: None,
        }
    }

    pub fn speed(&self) -> f32 {
        SPEEDS[self.speed]
    }

    pub fn duration(&self) -> Duration {
        self.info.map_or(Duration::ZERO, |info| info.duration)
    }

    /// The time of the frame on screen.
    pub fn position(&self) -> Duration {
        let played = Duration::from_secs(1).mul_f32(self.speed()) * self.shown / FPS;
        let position = self.from + played;
        if self.duration().is_zero() {
            position
        } else {
            position.min(self.duration())
        }
    }

    /// Starts `ffmpeg` at `at`, and `ffplay` when it plays.
    fn start(&mut self, at: Duration) {
        self.stop();
        let Some(info) = self.info else {
            return;
        };
        (self.ended, self.from, self.shown) = (false, at, 0);
        self.started = Instant::now();
        let child = Command::new("ffmpeg")
            .args(self.video_args(at, info.size))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(_) => {
                self.error = Some(MISSING.into());
                return;
            }
        };
        let (sender, receiver) = sync_channel(2);
        if let Some(stdout) = child.stdout.take() {
            let size = info.size;
            thread::spawn(move || read_frames(stdout, size, &sender));
        }
        self.frames = Some(receiver);
        self.child = Some(child);
        self.error = None;
        self.play_sound();
    }

    fn video_args(&self, at: Duration, (width, height): (u32, u32)) -> Vec<String> {
        let mut args = ["-v", "error", "-nostdin", "-ss"]
            .map(String::from)
            .to_vec();
        args.push(format!("{:.3}", at.as_secs_f64()));
        args.push("-i".into());
        args.push(self.path.to_string_lossy().into_owned());
        args.extend(["-an", "-vf"].map(String::from));
        args.push(format!(
            "setpts=PTS/{},fps={FPS},scale={width}:{height}",
            self.speed()
        ));
        args.extend(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"].map(String::from));
        args
    }

    /// The `ffplay` arguments for the sound from `at`; none when nothing should be heard.
    fn sound_args(&self, at: Duration) -> Option<Vec<String>> {
        let sound = self.info.is_some_and(|info| info.sound);
        if !sound || self.paused || self.ended || self.muted || self.volume == 0 {
            return None;
        }
        let mut args = [
            "-nodisp",
            "-vn",
            "-sn",
            "-autoexit",
            "-loglevel",
            "quiet",
            "-ss",
        ]
        .map(String::from)
        .to_vec();
        args.push(format!("{:.3}", at.as_secs_f64()));
        args.push("-volume".into());
        args.push(self.volume.to_string());
        if self.speed != NORMAL_SPEED {
            args.push("-af".into());
            args.push(format!("atempo={}", self.speed()));
        }
        args.push(self.path.to_string_lossy().into_owned());
        Some(args)
    }

    /// Stops the sound and plays it again from the position on screen, if it should play.
    fn play_sound(&mut self) {
        stop(&mut self.sound);
        if let Some(args) = self.sound_args(self.position()) {
            // Without `ffplay` the video plays without sound.
            self.sound = Command::new("ffplay")
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok();
        }
    }

    fn stop(&mut self) {
        stop(&mut self.child);
        stop(&mut self.sound);
        self.frames = None;
    }

    /// Shows the frame that is due, skipping the late ones; while paused, only the first frame
    /// after a seek. True when the picture changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(frames) = &self.frames else {
            return false;
        };
        let mut changed = false;
        while !self.ended && (!self.paused || self.shown == 0) {
            let due = self.started + Duration::from_secs(1) * self.shown / FPS;
            if !self.paused && now < due {
                break;
            }
            match frames.try_recv() {
                Ok(frame) => {
                    self.frame = Some(frame);
                    self.shown += 1;
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    self.stop();
                    if self.frame.is_none() {
                        self.error = Some("ffmpeg could not read this video".into());
                    }
                    return true;
                }
            }
        }
        changed
    }

    /// Does what a key or a button asks; false for what the video does not do itself.
    pub fn control(&mut self, control: Control) -> bool {
        match control {
            Control::Play if self.ended => {
                self.paused = false;
                self.start(Duration::ZERO);
            }
            Control::Play if self.paused => {
                // The clock goes on from the frame on screen.
                self.paused = false;
                self.started = Instant::now() - Duration::from_secs(1) * self.shown / FPS;
                self.play_sound();
            }
            Control::Play => {
                self.paused = true;
                stop(&mut self.sound);
            }
            Control::Stop => {
                self.paused = true;
                self.start(Duration::ZERO);
            }
            Control::Seek(seconds) => {
                let position = self.position().as_secs_f64() + f64::from(seconds);
                self.seek(Duration::from_secs_f64(position.max(0.0)));
            }
            Control::Quieter | Control::Louder => {
                self.volume = if control == Control::Louder {
                    self.volume.saturating_add(VOLUME_STEP).min(100)
                } else {
                    self.volume.saturating_sub(VOLUME_STEP)
                };
                self.muted = false;
                self.play_sound();
            }
            Control::Mute => {
                self.muted = !self.muted;
                self.play_sound();
            }
            Control::Slower | Control::Faster => {
                self.speed = if control == Control::Faster {
                    (self.speed + 1).min(SPEEDS.len() - 1)
                } else {
                    self.speed.saturating_sub(1)
                };
                self.seek(self.position());
            }
            Control::Fullscreen => return false,
        }
        true
    }

    /// Plays on from `at` (within the video), paused or not as it was.
    fn seek(&mut self, at: Duration) {
        let at = if self.duration().is_zero() {
            at
        } else {
            at.min(self.duration())
        };
        self.scrub = None;
        self.start(at);
    }

    /// Seeks to `fraction` (0 to 1) of the video: where the mouse let go of the bar.
    pub fn seek_to(&mut self, fraction: f32) {
        self.seek(self.duration().mul_f32(fraction.clamp(0.0, 1.0)));
    }

    /// The buttons under the video, left to right, each `[label]` with a space between.
    pub fn buttons(&self) -> Vec<(Control, &'static str)> {
        let play = match (self.ended, self.paused) {
            (true, _) => "Replay",
            (false, true) => "Play",
            (false, false) => "Pause",
        };
        vec![
            (Control::Play, play),
            (Control::Stop, "Stop"),
            (Control::Seek(-5), "-5s"),
            (Control::Seek(5), "+5s"),
            (Control::Quieter, "Vol-"),
            (Control::Louder, "Vol+"),
            (Control::Mute, if self.muted { "Unmute" } else { "Mute" }),
            (Control::Slower, "Slower"),
            (Control::Faster, "Faster"),
            (Control::Fullscreen, "Full screen"),
        ]
    }

    /// The button at `column` of the button row, from its left edge.
    pub fn button_at(&self, column: u16) -> Option<Control> {
        let mut left = 1;
        for (control, label) in self.buttons() {
            let right = left + label.chars().count() as u16 + 2;
            if (left..right).contains(&column) {
                return Some(control);
            }
            left = right + 1;
        }
        None
    }

    /// The volume and the speed, after the buttons.
    pub fn status(&self) -> String {
        let volume = if self.muted {
            "Muted".to_owned()
        } else {
            format!("Vol {}%", self.volume)
        };
        format!("{volume}  {}x", self.speed())
    }

    /// The progress row in `width` cells: the time on screen, the bar's first column and
    /// length, how much of it is played, and the length of the video.
    pub fn progress(&self, width: u16) -> (String, u16, u16, u16, String) {
        let hours = self.duration() >= Duration::from_secs(3600);
        let total = clock(self.duration(), hours);
        let fraction = self.scrub.unwrap_or_else(|| {
            let duration = self.duration().as_secs_f32();
            if duration > 0.0 {
                self.position().as_secs_f32() / duration
            } else {
                0.0
            }
        });
        let label = total.chars().count() as u16 + 2;
        let length = width.saturating_sub(label * 2);
        let played = (f32::from(length) * fraction.clamp(0.0, 1.0)).round() as u16;
        let shown = self.scrub.map_or(self.position(), |fraction| {
            self.duration().mul_f32(fraction.clamp(0.0, 1.0))
        });
        (clock(shown, hours), label, length, played, total)
    }

    /// The fraction of the video at `column` of the progress row, if it is on the bar.
    pub fn fraction_at(&self, width: u16, column: u16) -> Option<f32> {
        let (_, start, length, _, _) = self.progress(width);
        (length > 0 && (start..start + length).contains(&column))
            .then(|| f32::from(column - start) / f32::from(length.saturating_sub(1).max(1)))
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        self.stop();
    }
}

fn stop(child: &mut Option<Child>) {
    if let Some(mut child) = child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// `M:SS`, or `H:MM:SS` for a video an hour long or more.
fn clock(time: Duration, hours: bool) -> String {
    let seconds = time.as_secs();
    if hours {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}

pub const MISSING: &str =
    "Install ffmpeg to play videos: SozoIDE installs it by itself when it can";

/// The video's size, length and sound from `ffprobe`.
fn probe(path: &Path) -> Result<Info, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| MISSING.to_owned())?;
    parse_probe(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "ffmpeg could not read this video".into())
}

fn parse_probe(json: &str) -> Option<Info> {
    let value = serde_json::from_str::<serde_json::Value>(json).ok()?;
    let streams = value["streams"].as_array()?;
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")?;
    let size = (
        u32::try_from(video["width"].as_u64()?).ok()?,
        u32::try_from(video["height"].as_u64()?).ok()?,
    );
    let duration = value["format"]["duration"]
        .as_str()
        .and_then(|duration| duration.parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration >= 0.0)
        .map_or(Duration::ZERO, Duration::from_secs_f64);
    Some(Info {
        size: fit(size, LARGEST),
        duration,
        sound: streams.iter().any(|stream| stream["codec_type"] == "audio"),
    })
}

/// `size` scaled down to fit in `largest`, never up, in even numbers (what `ffmpeg` wants).
fn fit((width, height): (u32, u32), (most_width, most_height): (u32, u32)) -> (u32, u32) {
    let scale = (most_width as f32 / width.max(1) as f32)
        .min(most_height as f32 / height.max(1) as f32)
        .min(1.0);
    let even = |value: f32| ((value as u32) / 2 * 2).max(2);
    (even(width as f32 * scale), even(height as f32 * scale))
}

fn read_frames(mut stdout: impl Read, (width, height): (u32, u32), sender: &SyncSender<RgbaImage>) {
    let mut bytes = vec![0; (width * height * 3) as usize];
    while stdout.read_exact(&mut bytes).is_ok() {
        let Some(frame) = RgbImage::from_raw(width, height, bytes.clone()) else {
            return;
        };
        let frame = image::DynamicImage::ImageRgb8(frame).to_rgba8();
        if sender.send(frame).is_err() {
            return;
        }
    }
}

/// What `ffprobe` says of a 75.5 s video with sound, for the tests.
#[cfg(test)]
const PROBE: &str = r#"{"streams": [{"codec_type": "video", "width": 1920, "height": 1080},
    {"codec_type": "audio"}], "format": {"duration": "75.500000"}}"#;

/// That video, `clip.mp4`, before `ffmpeg` starts.
#[cfg(test)]
pub fn sample() -> Video {
    Video::new(Path::new("clip.mp4"), parse_probe(PROBE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A video of 75.5 s whose frames come from `frames` instead of `ffmpeg`.
    fn video(frames: Receiver<RgbaImage>) -> Video {
        let mut video = sample();
        video.frames = Some(frames);
        video
    }

    fn frame(value: u8) -> RgbaImage {
        RgbaImage::from_pixel(2, 2, image::Rgba([value, 0, 0, 255]))
    }

    #[test]
    fn frames_are_read_in_order_at_a_size_ffmpeg_accepts() {
        assert_eq!(fit((1920, 1080), LARGEST), (320, 180));
        assert_eq!(
            fit((101, 51), LARGEST),
            (100, 50),
            "never bigger, always even"
        );
        assert_eq!(fit((1080, 1920), LARGEST), (100, 180));
        // Two 2 × 1 frames on a pipe: red, then green; a cut-off third is dropped.
        let mut pipe = vec![255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 255, 0];
        pipe.extend([1, 2]);
        let (sender, receiver) = sync_channel(4);
        read_frames(pipe.as_slice(), (2, 1), &sender);
        drop(sender);
        let frames = receiver.iter().collect::<Vec<_>>();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].get_pixel(1, 0).0, [0, 255, 0, 255]);
    }

    #[test]
    fn ffprobe_gives_the_size_the_length_and_the_sound() {
        let info = parse_probe(PROBE).unwrap();
        assert_eq!(info.size, (320, 180));
        assert_eq!(info.duration, Duration::from_millis(75_500));
        assert!(info.sound);
        let silent = r#"{"streams": [{"codec_type": "video", "width": 64, "height": 48}],
            "format": {"duration": "N/A"}}"#;
        let info = parse_probe(silent).unwrap();
        assert!(!info.sound && info.duration.is_zero());
        assert!(parse_probe(r#"{"streams": [{"codec_type": "audio"}]}"#).is_none());
        assert!(parse_probe("").is_none());
    }

    #[test]
    fn a_video_plays_on_its_own_clock_skipping_late_frames() {
        let (sender, receiver) = sync_channel(8);
        for value in 1..=4 {
            sender.send(frame(value)).unwrap();
        }
        drop(sender);
        let mut video = video(receiver);
        let now = video.started;
        assert!(video.tick(now));
        assert!(!video.tick(now), "the next frame waits 1/15 s");
        // Two frames late: the latest due shows, the one before it is skipped.
        assert!(video.tick(now + Duration::from_secs(2) / FPS));
        assert_eq!(video.frame.as_ref().unwrap().get_pixel(0, 0).0[0], 3);
        assert_eq!(video.position(), Duration::from_secs(3) / FPS);
        video.paused = true;
        assert!(!video.tick(now + Duration::from_secs(9)), "paused");
        video.paused = false;
        let later = now + Duration::from_secs(9);
        assert!(
            video.tick(later) && video.ended,
            "the last frame, then the end"
        );
        assert_eq!(video.frame.as_ref().unwrap().get_pixel(0, 0).0[0], 4);
        assert!(video.error.is_none(), "it played: it just ended");
    }

    #[test]
    fn a_paused_video_shows_the_first_frame_after_a_seek() {
        let (sender, receiver) = sync_channel(4);
        sender.send(frame(7)).unwrap();
        sender.send(frame(8)).unwrap();
        let mut video = video(receiver);
        video.paused = true;
        assert!(video.tick(video.started));
        assert!(
            !video.tick(video.started + Duration::from_secs(5)),
            "one frame only"
        );
        assert_eq!(video.frame.as_ref().unwrap().get_pixel(0, 0).0[0], 7);
        drop(sender);
    }

    #[test]
    fn the_processes_are_asked_for_the_position_volume_and_speed() {
        let (_sender, receiver) = sync_channel(1);
        let mut video = video(receiver);
        let at = Duration::from_millis(12_500);
        let args = video.video_args(at, (320, 180)).join(" ");
        assert!(args.contains("-ss 12.500 -i clip.mp4"), "{args}");
        assert!(args.contains("setpts=PTS/1,fps=15,scale=320:180"), "{args}");
        let sound = video.sound_args(at).unwrap().join(" ");
        assert!(sound.contains("-nodisp") && sound.contains("-ss 12.500 -volume 100"));
        assert!(!sound.contains("atempo"), "{sound}");

        // Paused, so that no `ffplay` starts: speeds stop at both ends, the volume at 100.
        video.paused = true;
        for _ in 0..9 {
            video.control(Control::Faster);
        }
        assert_eq!(video.speed(), 2.0);
        assert!(
            video
                .video_args(at, (2, 2))
                .join(" ")
                .contains("setpts=PTS/2,")
        );
        video.paused = false;
        assert!(
            video
                .sound_args(at)
                .unwrap()
                .join(" ")
                .contains("-af atempo=2 ")
        );
        video.paused = true;
        video.control(Control::Quieter);
        assert_eq!(video.volume, 90);
        video.control(Control::Louder);
        video.control(Control::Louder);
        assert_eq!(video.volume, 100);
        video.control(Control::Mute);
        assert!(video.muted && video.status().starts_with("Muted"));
        video.control(Control::Louder);
        assert!(!video.muted, "the volume keys unmute");
        assert!(!video.control(Control::Fullscreen), "the app's to do");
        video.paused = false;
        video.muted = true;
        assert!(video.sound_args(at).is_none(), "muted");
        video.muted = false;
        video.paused = true;
        assert!(video.sound_args(at).is_none(), "paused");
        video.paused = false;
        video.info =
            parse_probe(r#"{"streams": [{"codec_type": "video", "width": 2, "height": 2}]}"#);
        assert!(video.sound_args(at).is_none(), "no sound in the file");
    }

    #[test]
    fn buttons_and_the_progress_bar_answer_to_their_columns() {
        let (_sender, receiver) = sync_channel(1);
        let mut video = video(receiver);
        // `[Pause] [Stop] [-5s] …` from column 1.
        assert_eq!(video.button_at(0), None);
        assert_eq!(video.button_at(1), Some(Control::Play));
        assert_eq!(video.button_at(7), Some(Control::Play));
        assert_eq!(video.button_at(8), None, "the space between");
        assert_eq!(video.button_at(9), Some(Control::Stop));
        assert_eq!(video.button_at(16), Some(Control::Seek(-5)));
        assert_eq!(video.buttons()[0].1, "Pause");
        video.paused = true;
        assert_eq!(video.buttons()[0].1, "Play");
        assert_eq!(video.status(), "Vol 100%  1x");

        // `00:00 ━━━… 01:15` in 27 cells: the bar is the 13 cells between the times.
        let (shown, start, length, played, total) = video.progress(27);
        assert_eq!((shown.as_str(), start, length, played), ("00:00", 7, 13, 0));
        assert_eq!(total, "01:15");
        assert_eq!(video.fraction_at(27, 6), None);
        assert_eq!(video.fraction_at(27, 7), Some(0.0));
        assert_eq!(video.fraction_at(27, 19), Some(1.0));
        video.scrub = Some(0.5);
        let (shown, _, _, played, _) = video.progress(27);
        assert_eq!((shown.as_str(), played), ("00:37", 7));
        assert_eq!(clock(Duration::from_secs(3725), true), "1:02:05");
    }
}

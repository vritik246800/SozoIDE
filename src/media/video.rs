//! Videos through the system's `ffmpeg` (F23, Q41): a thread reads its frames, small and in
//! RGB, and the tab shows one every 1/15 s. Nothing links FFmpeg into `sozo`; without the
//! command the tab says how to get it.

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
/// The largest frame asked of `ffmpeg`: a terminal shows fewer pixels than this.
const LARGEST: (u32, u32) = (320, 180);

pub struct Video {
    path: PathBuf,
    frames: Option<Receiver<RgbaImage>>,
    child: Option<Child>,
    pub frame: Option<RgbaImage>,
    next_at: Instant,
    pub paused: bool,
    pub ended: bool,
    /// Why it does not play: no `ffmpeg`, or a file it cannot read.
    pub error: Option<String>,
}

impl Video {
    pub fn open(path: &Path) -> Self {
        let mut video = Self {
            path: path.to_path_buf(),
            frames: None,
            child: None,
            frame: None,
            next_at: Instant::now(),
            paused: false,
            ended: false,
            error: None,
        };
        video.start();
        video
    }

    /// Starts `ffmpeg` from the beginning.
    fn start(&mut self) {
        self.stop();
        self.ended = false;
        let size = match frame_size(&self.path) {
            Ok(size) => size,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let child = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&self.path)
            .args([
                "-an",
                "-vf",
                &format!("fps={FPS},scale={}:{}", size.0, size.1),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-",
            ])
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
            thread::spawn(move || read_frames(stdout, size, &sender));
        }
        self.frames = Some(receiver);
        self.child = Some(child);
        self.error = None;
        self.next_at = Instant::now();
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.frames = None;
    }

    /// Shows the next frame when it is due; true when the picture changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.paused || self.ended || now < self.next_at {
            return false;
        }
        let Some(frames) = &self.frames else {
            return false;
        };
        match frames.try_recv() {
            Ok(frame) => {
                self.frame = Some(frame);
                self.next_at = now + Duration::from_secs(1) / FPS;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.ended = true;
                self.stop();
                if self.frame.is_none() {
                    self.error = Some("ffmpeg could not read this video".into());
                }
                true
            }
        }
    }

    /// Space: pauses, plays on, or plays again from the start once it ended.
    pub fn toggle(&mut self) {
        if self.ended {
            self.start();
        } else {
            self.paused = !self.paused;
            self.next_at = Instant::now();
        }
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        self.stop();
    }
}

pub const MISSING: &str =
    "Install ffmpeg to play videos: SozoIDE installs it by itself when it can";

/// The video's size from `ffprobe`, scaled down into `LARGEST`, in even numbers.
fn frame_size(path: &Path) -> Result<(u32, u32), String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| MISSING.to_owned())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut numbers = text
        .trim()
        .split(',')
        .map(|number| number.trim().parse::<u32>());
    let (Some(Ok(width)), Some(Ok(height))) = (numbers.next(), numbers.next()) else {
        return Err("ffmpeg could not read this video".into());
    };
    Ok(fit((width, height), LARGEST))
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_video_plays_pauses_and_ends_on_its_own_clock() {
        let (sender, receiver) = sync_channel(4);
        let frame = |value| RgbaImage::from_pixel(2, 2, image::Rgba([value, 0, 0, 255]));
        sender.send(frame(1)).unwrap();
        sender.send(frame(2)).unwrap();
        drop(sender);
        let now = Instant::now();
        let mut video = Video {
            path: PathBuf::from("clip.mp4"),
            frames: Some(receiver),
            child: None,
            frame: None,
            next_at: now,
            paused: false,
            ended: false,
            error: None,
        };
        assert!(video.tick(now));
        assert!(!video.tick(now), "the next frame waits 1/15 s");
        video.toggle();
        assert!(video.paused && !video.tick(now + Duration::from_secs(1)));
        video.paused = false;
        let later = now + Duration::from_secs(1);
        assert!(video.tick(later));
        assert_eq!(video.frame.as_ref().unwrap().get_pixel(0, 0).0[0], 2);
        assert!(video.tick(later + Duration::from_secs(1)) && video.ended);
        assert!(video.error.is_none(), "it played: it just ended");
    }
}

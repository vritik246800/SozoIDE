//! Pictures, GIFs, videos and 3D models in a tab, drawn as text (F03, F22, F23): each frame
//! becomes half blocks once per size and is kept until the picture changes.

use std::{
    fs::File,
    io::BufReader,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use image::{AnimationDecoder, RgbaImage, codecs::gif::GifDecoder};
use ratatui::text::Line;

mod ascii;
mod model;
mod video;

use model::Model;
#[cfg(test)]
pub use model::sample_glb;
pub use video::{Control, Video};

/// Pictures larger than this on a side are made smaller once, when they open: a terminal never
/// shows more pixels than that, and every later resize is quicker.
const LARGEST_PICTURE: u32 = 1024;
/// A GIF frame that gives no delay shows this long, as browsers do.
const DEFAULT_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Picture,
    Video,
    Model,
}

/// What a file opens as, by its extension.
pub fn kind(path: &Path) -> Option<Kind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "webp" | "tif" | "tiff" | "tga"
        | "ppm" | "pgm" | "pbm" | "pam" | "pnm" | "qoi" | "hdr" => Some(Kind::Picture),
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => Some(Kind::Video),
        "glb" | "gltf" => Some(Kind::Model),
        _ => None,
    }
}

enum Content {
    Picture(RgbaImage),
    /// The frames of a GIF with how long each shows, the one on screen and when it changes.
    Animation {
        frames: Vec<(RgbaImage, Duration)>,
        frame: usize,
        next_at: Instant,
    },
    Video(Video),
    Model(Model),
}

pub struct Media {
    content: Content,
    /// Bumped whenever the picture changes, so the drawing kept is drawn again.
    version: u64,
    drawn: Option<((u16, u16, u64), Vec<Line<'static>>)>,
}

impl Media {
    pub fn open(path: &Path) -> Result<Self> {
        let content = match kind(path).context("Not a picture, a video or a model")? {
            Kind::Video => Content::Video(Video::open(path)),
            Kind::Model => Content::Model(Model::load(path)?),
            Kind::Picture if is_gif(path) => animation(path)?,
            Kind::Picture => {
                let picture = image::ImageReader::open(path)
                    .with_context(|| format!("Could not open {}", path.display()))?
                    .with_guessed_format()?
                    .decode()
                    .with_context(|| format!("Could not decode {}", path.display()))?;
                let picture = if picture.width().max(picture.height()) > LARGEST_PICTURE {
                    picture.thumbnail(LARGEST_PICTURE, LARGEST_PICTURE)
                } else {
                    picture
                };
                Content::Picture(picture.to_rgba8())
            }
        };
        Ok(Self {
            content,
            version: 0,
            drawn: None,
        })
    }

    /// The name in the title of the tab.
    pub fn language(&self) -> &'static str {
        match self.content {
            Content::Picture(_) => "Image",
            Content::Animation { .. } => "GIF",
            Content::Video(_) => "Video",
            Content::Model(_) => "3D model",
        }
    }

    /// The keys this tab answers to, for its title.
    pub fn hint(&self) -> Option<String> {
        match &self.content {
            Content::Model(_) => Some("←/→ turn  ↑/↓ tilt  +/- zoom  drag, wheel".into()),
            Content::Video(_) => Some("Space  S  ←/→  +/-  M  [/]  F  Esc".into()),
            Content::Animation { frames, frame, .. } => {
                Some(format!("Frame {}/{}", frame + 1, frames.len()))
            }
            Content::Picture(_) => None,
        }
    }

    /// Moves a GIF or a video on; true when the picture changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let changed = match &mut self.content {
            Content::Animation {
                frames,
                frame,
                next_at,
            } if frames.len() > 1 && now >= *next_at => {
                *frame = (*frame + 1) % frames.len();
                // Late frames are not caught up one by one: the next one counts from now.
                *next_at = now + frames[*frame].1;
                true
            }
            Content::Video(video) => video.tick(now),
            _ => false,
        };
        if changed {
            self.version += 1;
        }
        changed
    }

    /// The keys of a model: arrows turn and tilt, + and - zoom.
    pub fn handle_key(&mut self, character: Option<char>, arrow: Option<(f32, f32)>) -> bool {
        let handled = match (&mut self.content, character, arrow) {
            (Content::Model(model), _, Some((yaw, pitch))) => {
                model.turn(yaw, pitch);
                true
            }
            (Content::Model(model), Some(character @ ('+' | '=' | '-' | '_')), _) => {
                model.zoom(matches!(character, '+' | '='));
                true
            }
            _ => false,
        };
        if handled {
            self.version += 1;
        }
        handled
    }

    /// The video in the tab, if it is one.
    pub fn video(&self) -> Option<&Video> {
        match &self.content {
            Content::Video(video) => Some(video),
            _ => None,
        }
    }

    /// Plays, pauses, seeks or changes the sound of a video (F23); false when nothing changed.
    pub fn control(&mut self, control: Control) -> bool {
        let Content::Video(video) = &mut self.content else {
            return false;
        };
        let handled = video.control(control);
        if handled {
            self.version += 1;
        }
        handled
    }

    /// The mouse drags a video's progress bar to `fraction` (0 to 1), or lets go of it there.
    pub fn scrub(&mut self, fraction: f32, release: bool) {
        if let Content::Video(video) = &mut self.content {
            if release {
                video.seek_to(fraction);
            } else {
                video.scrub = Some(fraction.clamp(0.0, 1.0));
            }
            self.version += 1;
        }
    }

    /// A mouse drag of `columns` × `rows` cells turns a model.
    pub fn drag(&mut self, columns: i32, rows: i32) {
        if let Content::Model(model) = &mut self.content {
            model.turn(columns as f32 * 0.5, rows as f32);
            self.version += 1;
        }
    }

    /// The wheel zooms a model and turns a video's volume up or down (Q52); false for the
    /// others.
    pub fn wheel(&mut self, up: bool) -> bool {
        match &mut self.content {
            Content::Model(model) => model.zoom(up),
            Content::Video(_) => {
                return self.control(if up {
                    Control::Louder
                } else {
                    Control::Quieter
                });
            }
            _ => return false,
        }
        self.version += 1;
        true
    }

    /// The tab's rows for `width` × `height` cells, drawn again only when the size or the
    /// picture changed.
    pub fn lines(&mut self, width: u16, height: u16) -> &[Line<'static>] {
        let key = (width, height, self.version);
        if self.drawn.as_ref().is_none_or(|(drawn, _)| *drawn != key) {
            let lines = match &self.content {
                Content::Picture(picture) => ascii::render(picture, width, height),
                Content::Animation { frames, frame, .. } => {
                    ascii::render(&frames[*frame].0, width, height)
                }
                Content::Model(model) => ascii::render(
                    &model.render(u32::from(width), u32::from(height) * 2),
                    width,
                    height,
                ),
                Content::Video(video) => match (&video.error, &video.frame) {
                    (Some(error), _) => message(error, height),
                    (None, Some(frame)) => ascii::render(frame, width, height),
                    (None, None) => message("Loading the video...", height),
                },
            };
            self.drawn = Some((key, lines));
        }
        self.drawn.as_ref().map_or(&[], |(_, lines)| lines)
    }
}

/// A video of 75.5 s with sound that has not started `ffmpeg`, for the tests.
#[cfg(test)]
pub fn sample_video() -> Media {
    Media {
        content: Content::Video(video::sample()),
        version: 0,
        drawn: None,
    }
}

fn is_gif(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"))
}

/// A GIF with its frames composed (`image`, over the `gif` decoder); one frame is a picture.
fn animation(path: &Path) -> Result<Content> {
    let file = File::open(path).with_context(|| format!("Could not open {}", path.display()))?;
    let frames = GifDecoder::new(BufReader::new(file))
        .and_then(|decoder| decoder.into_frames().collect_frames())
        .with_context(|| format!("Could not decode {}", path.display()))?
        .into_iter()
        .map(|frame| {
            let (numerator, denominator) = frame.delay().numer_denom_ms();
            let delay = Duration::from_millis(u64::from(numerator / denominator.max(1)));
            let delay = if delay.is_zero() {
                DEFAULT_DELAY
            } else {
                delay
            };
            (frame.into_buffer(), delay)
        })
        .collect::<Vec<_>>();
    match frames.len() {
        0 => anyhow::bail!("{} has no frames", path.display()),
        1 => Ok(Content::Picture(frames.into_iter().next().unwrap().0)),
        _ => Ok(Content::Animation {
            next_at: Instant::now() + frames[0].1,
            frames,
            frame: 0,
        }),
    }
}

/// A line of text in the middle of the tab.
fn message(text: &str, height: u16) -> Vec<Line<'static>> {
    let mut lines = vec![Line::default(); usize::from(height / 2)];
    lines.push(Line::from(text.to_owned()).centered());
    lines
}

#[cfg(test)]
mod tests {
    use image::{Delay, Frame, Rgba, codecs::gif::GifEncoder};

    use super::*;

    #[test]
    fn files_open_by_kind_and_a_gif_plays_its_frames_on_time() {
        assert_eq!(kind(Path::new("a.PNG")), Some(Kind::Picture));
        assert_eq!(kind(Path::new("photo.tiff")), Some(Kind::Picture));
        assert_eq!(kind(Path::new("clip.mp4")), Some(Kind::Video));
        assert_eq!(kind(Path::new("ship.glb")), Some(Kind::Model));
        assert_eq!(kind(Path::new("scene.gltf")), Some(Kind::Model));
        assert_eq!(kind(Path::new("notes.md")), None);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("blink.gif");
        let mut encoder = GifEncoder::new(File::create(&path).unwrap());
        for (colour, milliseconds) in [([255, 0, 0, 255], 200), ([0, 0, 255, 255], 0)] {
            let picture = RgbaImage::from_pixel(4, 4, Rgba(colour));
            let delay = Delay::from_numer_denom_ms(milliseconds, 1);
            encoder
                .encode_frame(Frame::from_parts(picture, 0, 0, delay))
                .unwrap();
        }
        drop(encoder);
        let mut gif = Media::open(&path).unwrap();
        assert_eq!(gif.language(), "GIF");
        assert_eq!(gif.hint().as_deref(), Some("Frame 1/2"));
        let red = gif.lines(4, 2).to_vec();
        let start = Instant::now();
        assert!(!gif.tick(start), "the first frame shows 200 ms");
        assert!(gif.tick(start + Duration::from_millis(250)));
        assert_ne!(gif.lines(4, 2), red, "the next frame is drawn");
        let Content::Animation { frames, .. } = &gif.content else {
            panic!("an animation");
        };
        assert_eq!(frames[1].1, DEFAULT_DELAY, "no delay shows 100 ms");

        // A model answers to its keys and the wheel; a picture to none.
        let model = directory.path().join("triangle.glb");
        std::fs::write(&model, sample_glb()).unwrap();
        let mut model = Media::open(&model).unwrap();
        let before = model.lines(20, 10).to_vec();
        assert!(model.handle_key(None, Some((1.0, 0.0))));
        assert!(model.handle_key(Some('+'), None));
        assert!(model.wheel(true));
        assert_ne!(model.lines(20, 10), before);
        let picture = directory.path().join("dot.png");
        RgbaImage::from_pixel(2, 2, Rgba([0, 255, 0, 255]))
            .save(&picture)
            .unwrap();
        let mut picture = Media::open(&picture).unwrap();
        assert!(!picture.handle_key(Some('+'), Some((1.0, 0.0))) && !picture.wheel(true));
        assert_eq!(picture.language(), "Image");
    }
}

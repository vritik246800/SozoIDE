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
mod captions;
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

/// A picture as half blocks in `width` × `height` cells (the images of Markdown, TODO 5.1.9).
pub fn half_blocks(picture: &RgbaImage, width: u16, height: u16) -> Vec<Line<'static>> {
    ascii::render(picture, width, height)
}

/// A picture on disk, read with the same limits as a tab's, made small: none when it cannot be.
pub fn small_picture(path: &Path, largest: u32) -> Option<RgbaImage> {
    let mut reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    reader.limits(limits());
    let picture = reader.decode().ok()?;
    Some(picture.thumbnail(largest, largest).to_rgba8())
}

/// What a file opens as, by its extension.
pub fn kind(path: &Path) -> Option<Kind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "webp" | "tif" | "tiff" | "tga"
        | "ppm" | "pgm" | "pbm" | "pam" | "pnm" | "qoi" | "hdr" | "svg" => Some(Kind::Picture),
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" | "mp3" | "flac" | "wav" | "ogg" | "m4a"
        | "opus" | "aac" => Some(Kind::Video),
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
    /// Boxed: with its two processes it is the largest by far, most of all on Windows (E33).
    Video(Box<Video>),
    /// Boxed too: its scene and animations are kept to pose it again (TODO 5.1.8).
    Model(Box<Model>),
}

pub struct Media {
    content: Content,
    /// What a picture is: its size in pixels, format and bytes on disk (TODO 5.1.5).
    info: String,
    /// How far a picture is zoomed (1 fits it) and the point of it in the middle (0 to 1).
    zoom: f32,
    centre: (f32, f32),
    /// Bumped whenever the picture changes, so the drawing kept is drawn again.
    version: u64,
    drawn: Option<((u16, u16, u64), Vec<Line<'static>>)>,
}

impl Media {
    pub fn open(path: &Path) -> Result<Self> {
        let content = match kind(path).context("Not a picture, a video or a model")? {
            Kind::Video => Content::Video(Box::new(Video::open(path))),
            Kind::Model => Content::Model(Box::new(Model::load(path)?)),
            Kind::Picture if is_gif(path) => animation(path)?,
            Kind::Picture if is_svg(path) => Content::Picture(svg(path)?),
            Kind::Picture => {
                let mut reader = image::ImageReader::open(path)
                    .with_context(|| format!("Could not open {}", path.display()))?
                    .with_guessed_format()?;
                reader.limits(limits());
                let picture = reader
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
        let info = match &content {
            Content::Picture(_) | Content::Animation { .. } => picture_info(path),
            _ => String::new(),
        };
        Ok(Self {
            content,
            info,
            zoom: 1.0,
            centre: (0.5, 0.5),
            version: 0,
            drawn: None,
        })
    }

    /// `+`/`-` zoom a picture, `0` fits it again, the arrows move it while zoomed; false when
    /// the key is not one of these or the tab is not a picture.
    fn picture_key(&mut self, character: Option<char>, arrow: Option<(f32, f32)>) -> bool {
        if !matches!(self.content, Content::Picture(_)) {
            return false;
        }
        match (character, arrow) {
            (Some('+' | '='), _) => self.zoom = (self.zoom * 1.25).min(16.0),
            (Some('-' | '_'), _) => self.zoom = (self.zoom / 1.25).max(1.0),
            (Some('0'), _) => (self.zoom, self.centre) = (1.0, (0.5, 0.5)),
            (_, Some((x, y))) if self.zoom > 1.0 => self.pan(x * 0.1, y * 0.1),
            _ => return false,
        }
        if self.zoom <= 1.0 {
            self.centre = (0.5, 0.5);
        }
        self.version += 1;
        true
    }

    /// Moves the middle of a zoomed picture by a fraction of what shows.
    fn pan(&mut self, x: f32, y: f32) {
        let half = 0.5 / self.zoom;
        self.centre.0 = (self.centre.0 + x / self.zoom).clamp(half, 1.0 - half);
        self.centre.1 = (self.centre.1 + y / self.zoom).clamp(half, 1.0 - half);
    }

    /// The part of `picture` that shows at this zoom.
    fn shown(&self, picture: &RgbaImage) -> RgbaImage {
        if self.zoom <= 1.0 {
            return picture.clone();
        }
        let (width, height) = picture.dimensions();
        let (part_width, part_height) = (
            ((width as f32) / self.zoom).max(1.0) as u32,
            ((height as f32) / self.zoom).max(1.0) as u32,
        );
        let x = ((self.centre.0 * width as f32) as u32)
            .saturating_sub(part_width / 2)
            .min(width - part_width);
        let y = ((self.centre.1 * height as f32) as u32)
            .saturating_sub(part_height / 2)
            .min(height - part_height);
        image::imageops::crop_imm(picture, x, y, part_width, part_height).to_image()
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
            Content::Model(model) => Some(match model.animation_name() {
                Some(name) => format!(
                    "{}  playing {name}  P stop  W wireframe  0 reset",
                    model.info
                ),
                None => format!(
                    "{}  ←/→ ↑/↓ turn  +/- zoom  W wireframe  0 reset  P animate",
                    model.info
                ),
            }),
            Content::Video(_) => Some("Space  S  ←/→  +/-  M  [/]  F  Esc".into()),
            Content::Animation { frames, frame, .. } => {
                Some(format!("Frame {}/{}", frame + 1, frames.len()))
            }
            Content::Picture(_) if self.zoom > 1.0 => Some(format!(
                "{}  {:.0}%  +/- zoom  arrows, drag move  0 fit",
                self.info,
                self.zoom * 100.0
            )),
            Content::Picture(_) => Some(format!("{}  +/- zoom  0 fit", self.info)),
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
            // An animation of a model moves on 15 times a second (TODO 5.1.8).
            Content::Model(model) if model.playing.is_some() => {
                let step = now.saturating_duration_since(model.last_pose.unwrap_or(now));
                if model.last_pose.is_none() || step >= Duration::from_millis(66) {
                    model.last_pose = Some(now);
                    model.advance(step)
                } else {
                    false
                }
            }
            _ => false,
        };
        if changed {
            self.version += 1;
        }
        changed
    }

    /// The keys of a model: arrows turn and tilt, + and - zoom.
    pub fn handle_key(&mut self, character: Option<char>, arrow: Option<(f32, f32)>) -> bool {
        if self.picture_key(character, arrow) {
            return true;
        }
        let handled = match (&mut self.content, character, arrow) {
            (Content::Model(model), _, Some((yaw, pitch))) => {
                model.turn(yaw, pitch);
                true
            }
            (Content::Model(model), Some(character @ ('+' | '=' | '-' | '_')), _) => {
                model.zoom(matches!(character, '+' | '='));
                true
            }
            (Content::Model(model), Some('w' | 'W'), _) => {
                model.wireframe = !model.wireframe;
                true
            }
            (Content::Model(model), Some('0'), _) => {
                model.reset();
                true
            }
            (Content::Model(model), Some('p' | 'P'), _) => {
                model.last_pose = None;
                model.toggle_animation()
            }
            _ => false,
        };
        if handled {
            self.version += 1;
        }
        handled
    }

    /// Plays a video on from where it was left; paused stays paused.
    pub fn resume_at(&mut self, at: Duration) {
        if let Content::Video(video) = &mut self.content {
            video.resume_at(at);
            self.version += 1;
        }
    }

    /// The video in the tab, if it is one.
    pub fn video(&self) -> Option<&Video> {
        match &self.content {
            Content::Video(video) => Some(video.as_ref()),
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
        } else if matches!(self.content, Content::Picture(_)) && self.zoom > 1.0 {
            // The picture follows the mouse: dragging right shows more of its left.
            self.pan(-(columns as f32) * 0.02, -(rows as f32) * 0.04);
            self.version += 1;
        }
    }

    /// The wheel zooms a model and turns a video's volume up or down (Q52); false for the
    /// others.
    pub fn wheel(&mut self, up: bool) -> bool {
        match &mut self.content {
            Content::Model(model) => model.zoom(up),
            Content::Picture(_) => return self.picture_key(Some(if up { '+' } else { '-' }), None),
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
        if let Content::Video(video) = &self.content {
            video.draw_in(width, height);
        }
        if self.drawn.as_ref().is_none_or(|(drawn, _)| *drawn != key) {
            let shown = match &self.content {
                Content::Picture(picture) if self.zoom > 1.0 => Some(self.shown(picture)),
                _ => None,
            };
            let lines = match &mut self.content {
                Content::Picture(picture) => {
                    ascii::render(shown.as_ref().unwrap_or(picture), width, height)
                }
                Content::Animation { frames, frame, .. } => {
                    ascii::render(&frames[*frame].0, width, height)
                }
                Content::Model(model) => ascii::render(
                    &model.render(u32::from(width), u32::from(height) * 2),
                    width,
                    height,
                ),
                Content::Video(video) => {
                    let mut lines = match (&video.error, &video.frame) {
                        (Some(error), _) => message(error, height),
                        (None, _) if video.audio_only() => message("Sound only", height),
                        // Made with the frame in the background, unless the tab changed size.
                        (None, Some(frame)) => match video.drawn.take() {
                            Some((cells, lines)) if cells == (width, height) => lines,
                            _ => ascii::render(frame, width, height),
                        },
                        (None, None) => message("Loading the video...", height),
                    };
                    // The subtitle on the last row, over the picture (TODO 5.1.7).
                    if let Some(caption) = video.caption() {
                        let row = usize::from(height.saturating_sub(1));
                        lines.resize(lines.len().max(row + 1), Line::default());
                        lines[row] = Line::from(caption.to_owned()).centered().style(
                            ratatui::style::Style::default()
                                .add_modifier(ratatui::style::Modifier::REVERSED),
                        );
                    }
                    lines
                }
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
        content: Content::Video(Box::new(video::sample())),
        info: String::new(),
        zoom: 1.0,
        centre: (0.5, 0.5),
        version: 0,
        drawn: None,
    }
}

/// That video with one green frame on its way from `ffmpeg`.
#[cfg(test)]
pub fn sample_video_with_a_frame() -> Media {
    Media {
        content: Content::Video(Box::new(video::sample_with_a_frame())),
        info: String::new(),
        zoom: 1.0,
        centre: (0.5, 0.5),
        version: 0,
        drawn: None,
    }
}

/// What a picture may ask of the memory before a pixel is read: a file of a few bytes can
/// say it is a hundred thousand pixels wide.
const MOST_PIXELS_ACROSS: u32 = 30_000;
const MOST_PICTURE_BYTES: u64 = 512 * 1024 * 1024;
/// The frames of a GIF kept in memory.
const MOST_FRAMES: usize = 2_000;
const MOST_FRAME_BYTES: usize = 256 * 1024 * 1024;
/// How long a picture, a GIF or a PDF may take to open.
pub const OPENING: Duration = Duration::from_secs(20);

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MOST_PIXELS_ACROSS);
    limits.max_image_height = Some(MOST_PIXELS_ACROSS);
    limits.max_alloc = Some(MOST_PICTURE_BYTES);
    limits
}

/// What `open` makes, on a thread of its own, or an error when it takes longer than `limit`:
/// a file made to keep a decoder busy does not stop the keys. The thread is left to end.
pub fn within<T: Send + 'static>(
    limit: Duration,
    open: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(open());
    });
    receiver
        .recv_timeout(limit)
        .unwrap_or_else(|_| anyhow::bail!("It takes too long to open: stopped"))
}

/// The largest SVG file read: a drawing, not a data dump (CLAUDE.md §7.2).
const LARGEST_SVG: u64 = 16 * 1024 * 1024;

fn is_svg(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

/// An SVG drawn into pixels, at most `LARGEST_PICTURE` on a side. Nothing it points to is
/// read: no other file, no network, no fonts (its text does not show).
fn svg(path: &Path) -> Result<RgbaImage> {
    if std::fs::metadata(path).is_ok_and(|file| file.len() > LARGEST_SVG) {
        anyhow::bail!("{} is larger than 16 MB: too large to draw", path.display());
    }
    let data = std::fs::read(path).with_context(|| format!("Could not open {}", path.display()))?;
    let options = resvg::usvg::Options {
        resources_dir: None,
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(&data, &options)
        .with_context(|| format!("Could not read the SVG {}", path.display()))?;
    let size = tree.size();
    let scale = (LARGEST_PICTURE as f32 / size.width().max(size.height())).min(4.0);
    let (width, height) = (
        ((size.width() * scale).ceil() as u32).max(1),
        ((size.height() * scale).ceil() as u32).max(1),
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).context("The SVG has no size")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia keeps colours premultiplied by their alpha; a picture keeps them straight.
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let colour = pixel.demultiply();
            [colour.red(), colour.green(), colour.blue(), colour.alpha()]
        })
        .collect();
    RgbaImage::from_raw(width, height, pixels).context("The SVG could not be drawn")
}

/// `1920×1080  PNG  2.3 MB` of a picture on disk.
fn picture_info(path: &Path) -> String {
    let format = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_uppercase())
        .unwrap_or_default();
    let dimensions = if is_svg(path) {
        None
    } else {
        image::ImageReader::open(path)
            .ok()
            .and_then(|reader| reader.with_guessed_format().ok())
            .and_then(|reader| reader.into_dimensions().ok())
    };
    let bytes = std::fs::metadata(path).map(|file| file.len()).unwrap_or(0);
    let size = match bytes {
        0..1_024 => format!("{bytes} B"),
        1_024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1_024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    };
    match dimensions {
        Some((width, height)) => format!("{width}×{height}  {format}  {size}"),
        None => format!("{format}  {size}"),
    }
}

fn is_gif(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"))
}

/// A GIF with its frames composed (`image`, over the `gif` decoder); one frame is a picture.
fn animation(path: &Path) -> Result<Content> {
    let file = File::open(path).with_context(|| format!("Could not open {}", path.display()))?;
    let mut decoder = GifDecoder::new(BufReader::new(file))
        .with_context(|| format!("Could not decode {}", path.display()))?;
    image::ImageDecoder::set_limits(&mut decoder, limits())
        .with_context(|| format!("{} is too large to show", path.display()))?;
    // A GIF says how many frames it has only by having them: the ones that fit are played.
    let mut decoded = Vec::new();
    let mut bytes = 0;
    for frame in decoder.into_frames().take(MOST_FRAMES) {
        let frame = frame.with_context(|| format!("Could not decode {}", path.display()))?;
        bytes += frame.buffer().as_raw().len();
        if bytes > MOST_FRAME_BYTES && !decoded.is_empty() {
            break;
        }
        decoded.push(frame);
    }
    let frames = decoded
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
    fn hostile_pictures_and_slow_files_do_not_take_the_memory_or_the_keys() {
        let directory = tempfile::tempdir().unwrap();
        // A bitmap of 54 bytes that says it is 100 000 pixels each way (30 GB).
        let mut header = Vec::new();
        header.extend_from_slice(b"BM");
        header.extend_from_slice(&54_u32.to_le_bytes());
        header.extend_from_slice(&[0; 4]);
        header.extend_from_slice(&54_u32.to_le_bytes());
        header.extend_from_slice(&40_u32.to_le_bytes());
        header.extend_from_slice(&100_000_i32.to_le_bytes());
        header.extend_from_slice(&100_000_i32.to_le_bytes());
        header.extend_from_slice(&1_u16.to_le_bytes());
        header.extend_from_slice(&24_u16.to_le_bytes());
        header.extend_from_slice(&[0; 24]);
        let huge = directory.path().join("huge.bmp");
        std::fs::write(&huge, header).unwrap();
        let started = Instant::now();
        assert!(Media::open(&huge).is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        // And a real one wider than any screen: one row of 40 000 pixels.
        let wide = directory.path().join("wide.png");
        RgbaImage::new(MOST_PIXELS_ACROSS + 10_000, 1)
            .save(&wide)
            .unwrap();
        assert!(Media::open(&wide).is_err());
        let fits = directory.path().join("fits.png");
        RgbaImage::new(2_000, 1).save(&fits).unwrap();
        assert!(Media::open(&fits).is_ok());

        // A GIF with more frames than are kept plays the ones that fit.
        let many = directory.path().join("many.gif");
        let mut encoder = GifEncoder::new(File::create(&many).unwrap());
        let frame = || {
            Frame::from_parts(
                RgbaImage::new(1, 1),
                0,
                0,
                Delay::from_numer_denom_ms(20, 1),
            )
        };
        encoder
            .encode_frames((0..MOST_FRAMES + 50).map(|_| frame()))
            .unwrap();
        drop(encoder);
        match Media::open(&many).unwrap().content {
            Content::Animation { frames, .. } => assert_eq!(frames.len(), MOST_FRAMES),
            _ => panic!("not an animation"),
        }

        // What takes too long to open is given up, and the caller goes on.
        let started = Instant::now();
        let slow = within(Duration::from_millis(100), || {
            std::thread::sleep(Duration::from_secs(3));
            Ok(1)
        });
        assert_eq!(
            slow.unwrap_err().to_string(),
            "It takes too long to open: stopped"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(within(OPENING, || Ok(7)).unwrap(), 7);
        assert!(within(OPENING, || -> Result<u8> { anyhow::bail!("broken") }).is_err());
    }

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
        // A picture zooms now (TODO 5.1.5); a key it does not know changes nothing.
        assert!(!picture.handle_key(Some('x'), None));
        assert!(picture.handle_key(Some('+'), None) && picture.wheel(true));
        assert_eq!(picture.language(), "Image");
    }

    #[test]
    fn a_picture_zooms_and_moves_says_what_it_is_and_an_svg_is_drawn_without_reaching_out() {
        let directory = tempfile::tempdir().unwrap();
        let png = directory.path().join("half.png");
        // Left half red, right half blue.
        let picture = RgbaImage::from_fn(40, 20, |x, _| {
            if x < 20 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        picture.save(&png).unwrap();
        let mut media = Media::open(&png).unwrap();
        assert!(
            media.hint().unwrap().starts_with("40×20  PNG  "),
            "{:?}",
            media.hint()
        );
        // Zoomed in and moved all the way left, only red shows.
        for _ in 0..4 {
            assert!(media.handle_key(Some('+'), None));
        }
        for _ in 0..20 {
            media.handle_key(None, Some((-1.0, 0.0)));
        }
        let Content::Picture(full) = &media.content else {
            panic!()
        };
        let part = media.shown(&full.clone());
        assert!(
            part.width() < 40 && part.pixels().all(|pixel| pixel.0[0] == 255),
            "only the red half"
        );
        assert!(media.hint().unwrap().contains("244%"), "{:?}", media.hint());
        assert!(media.handle_key(Some('0'), None));
        assert_eq!((media.zoom, media.centre), (1.0, (0.5, 0.5)));
        assert!(
            !media.handle_key(Some('-'), None) || media.zoom == 1.0,
            "never smaller than fit"
        );
        assert!(media.wheel(true) && media.zoom > 1.0);

        // An SVG is drawn; what it points to (a file, the network) is never read.
        let secret = directory.path().join("secret.png");
        picture.save(&secret).unwrap();
        let svg = directory.path().join("drawing.svg");
        std::fs::write(
            &svg,
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="20" height="10">
                <rect width="20" height="10" fill="#00ff00"/>
                <image xlink:href="{}" width="20" height="10"/>
                <image xlink:href="https://example.com/x.png" width="20" height="10"/></svg>"##,
                secret.display()
            ),
        )
        .unwrap();
        let drawing = Media::open(&svg).unwrap();
        let Content::Picture(pixels) = &drawing.content else {
            panic!("an SVG is a picture")
        };
        assert_eq!(pixels.width(), 2 * pixels.height());
        assert!(
            pixels.pixels().all(|pixel| pixel.0 == [0, 255, 0, 255]),
            "the rectangle, not the file it points to"
        );
        assert!(drawing.hint().unwrap().starts_with("SVG  "));
        // Broken and huge SVGs say so instead of drawing.
        let broken = directory.path().join("broken.svg");
        std::fs::write(&broken, "<svg><unclosed").unwrap();
        assert!(Media::open(&broken).is_err());
        let huge = directory.path().join("huge.svg");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(LARGEST_SVG + 1)
            .unwrap();
        assert!(
            Media::open(&huge)
                .err()
                .unwrap()
                .to_string()
                .contains("too large")
        );
    }
}

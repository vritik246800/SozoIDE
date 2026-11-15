//! glTF models (`.glb`, `.gltf`) drawn in software (F22): the triangles of the default scene,
//! each in its base colour (or its texture at its centre), lit from the viewer's upper left,
//! with a depth buffer, in bands of rows drawn in parallel.

use std::{collections::HashMap, path::Path, thread, time::Duration};

use anyhow::{Context, Result, bail};
use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
use image::RgbaImage;
use rayon::prelude::*;

/// The stack a model is read on. Its own thread also turns what a broken file panics on, now
/// that files are not validated, into an error (E54).
const LOADING_STACK: usize = 32 * 1024 * 1024;
/// Nodes inside nodes before the rest is left out: unvalidated files can loop (E54).
const DEEPEST_NODE: usize = 2_000;
/// Rows of pixels each thread draws at a time.
const BAND: usize = 8;
const FIELD_OF_VIEW: f32 = 50.0;
/// One step of the arrows, in radians, and of the zoom.
const TURN: f32 = 0.15;
const ZOOM: f32 = 0.85;

struct Triangle {
    points: [Vec3; 3],
    colour: Vec3,
}

/// What a model needs to be posed again by an animation (TODO 5.1.8).
struct Scene {
    document: gltf::Document,
    buffers: Vec<gltf::buffer::Data>,
    images: Vec<Option<gltf::image::Data>>,
    /// The centre and radius of the first pose: every pose is fitted the same way.
    centre: Vec3,
    radius: f32,
}

/// The keyframes of one property of one node.
struct Channel {
    node: usize,
    property: gltf::animation::Property,
    times: Vec<f32>,
    values: Vec<Vec4>,
}

struct Animation {
    name: String,
    length: f32,
    channels: Vec<Channel>,
}

/// The most keyframes read of a model's animations: a few seconds of many nodes.
const MOST_KEYFRAMES: usize = 1_000_000;

/// A model fitted in a sphere of radius 1 around the origin, and the camera around it.
pub struct Model {
    triangles: Vec<Triangle>,
    scene: Option<Scene>,
    animations: Vec<Animation>,
    /// The animation playing and how far into it, when one plays (`P`).
    pub playing: Option<(usize, f32)>,
    /// Edges only (`W`), and what the info line says (triangles, materials, animations).
    pub wireframe: bool,
    pub info: String,
    /// When the animation was last posed.
    pub last_pose: Option<std::time::Instant>,
    /// Turned around the vertical axis (←/→) and tilted (↑/↓), in radians.
    pub yaw: f32,
    pub pitch: f32,
    /// From the camera to the centre, in radii of the model.
    pub distance: f32,
}

impl Model {
    pub fn load(path: &Path) -> Result<Self> {
        thread::scope(|scope| {
            thread::Builder::new()
                .stack_size(LOADING_STACK)
                .spawn_scoped(scope, || Self::read(path))?
                .join()
                .unwrap_or_else(|_| bail!("{} is not a model that opens", path.display()))
        })
    }

    fn read(path: &Path) -> Result<Self> {
        let opening = || format!("Could not open {}", path.display());
        let file = std::fs::read(path).with_context(opening)?;
        // Extensions a model asks for do not stop it: what they add is not drawn (E54).
        let gltf::Gltf { document, blob } =
            gltf::Gltf::from_slice_without_validation(&file).with_context(opening)?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let buffers = gltf::import_buffers(&document, Some(base), blob).with_context(opening)?;
        // A texture that does not decode leaves its base colour.
        let images = document
            .images()
            .map(|image| gltf::image::Data::from_source(image.source(), Some(base), &buffers).ok())
            .collect::<Vec<_>>();
        let mut triangles = pose(&document, &buffers, &images, &HashMap::new())?;
        if triangles.is_empty() {
            bail!("{} has no triangles to draw", path.display());
        }
        // Centred and scaled to a radius of 1: every model starts in view.
        let (low, high) = triangles
            .iter()
            .flat_map(|triangle| triangle.points)
            .fold((Vec3::MAX, Vec3::MIN), |(low, high), point| {
                (low.min(point), high.max(point))
            });
        let centre = (low + high) / 2.0;
        let radius = triangles
            .iter()
            .flat_map(|triangle| triangle.points)
            .map(|point| point.distance(centre))
            .fold(f32::EPSILON, f32::max);
        for triangle in &mut triangles {
            for point in &mut triangle.points {
                *point = (*point - centre) / radius;
            }
        }
        let animations = read_animations(&document, &buffers);
        let info = format!(
            "{} triangles  {} materials  {} animations",
            triangles.len(),
            document.materials().len(),
            animations.len()
        );
        Ok(Self {
            triangles,
            scene: Some(Scene {
                document,
                buffers,
                images,
                centre,
                radius,
            }),
            animations,
            playing: None,
            wireframe: false,
            info,
            last_pose: None,
            yaw: 0.6,
            pitch: 0.3,
            distance: 3.0,
        })
    }

    /// `0`: the view it opened with.
    pub fn reset(&mut self) {
        (self.yaw, self.pitch, self.distance) = (0.6, 0.3, 3.0);
    }

    /// `P`: the first animation plays, or stops; false when the model has none.
    pub fn toggle_animation(&mut self) -> bool {
        if self.animations.is_empty() {
            return false;
        }
        self.playing = match self.playing {
            Some(_) => None,
            None => Some((0, 0.0)),
        };
        true
    }

    /// The name of the animation playing, for the title.
    pub fn animation_name(&self) -> Option<&str> {
        self.playing
            .map(|(index, _)| self.animations[index].name.as_str())
    }

    /// Moves the animation on by `step` and poses the model again (round the end); false when
    /// none plays.
    pub fn advance(&mut self, step: Duration) -> bool {
        let Some((index, time)) = self.playing else {
            return false;
        };
        let animation = &self.animations[index];
        let time = if animation.length > 0.0 {
            (time + step.as_secs_f32()) % animation.length
        } else {
            0.0
        };
        self.playing = Some((index, time));
        let overrides = animation.pose(time, self.scene.as_ref().map(|scene| &scene.document));
        let Some(scene) = &self.scene else {
            return false;
        };
        // Posed on a thread of its own, with the stack of the first one (deep models, E54).
        let posed = thread::scope(|scope| {
            thread::Builder::new()
                .stack_size(LOADING_STACK)
                .spawn_scoped(scope, || {
                    pose(&scene.document, &scene.buffers, &scene.images, &overrides)
                })
                .ok()?
                .join()
                .ok()?
                .ok()
        });
        let Some(mut triangles) = posed else {
            return false;
        };
        for triangle in &mut triangles {
            for point in &mut triangle.points {
                *point = (*point - scene.centre) / scene.radius;
            }
        }
        self.triangles = triangles;
        true
    }

    /// Turns by `yaw` and tilts by `pitch` steps; the tilt stops short of upside down.
    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        self.yaw = (self.yaw + yaw * TURN) % std::f32::consts::TAU;
        self.pitch = (self.pitch + pitch * TURN).clamp(-1.5, 1.5);
    }

    pub fn zoom(&mut self, closer: bool) {
        let factor = if closer { ZOOM } else { 1.0 / ZOOM };
        self.distance = (self.distance * factor).clamp(1.2, 30.0);
    }

    /// The model seen from the camera in `width` × `height` pixels; the background is clear.
    pub fn render(&self, width: u32, height: u32) -> RgbaImage {
        let (width, height) = (width as usize, height as usize);
        let mut pixels = vec![[0u8; 4]; width * height];
        let mut depth = vec![f32::INFINITY; width * height];
        if width == 0 || height == 0 {
            return RgbaImage::new(width as u32, height as u32);
        }
        let rotation = Mat3::from_rotation_x(self.pitch) * Mat3::from_rotation_y(self.yaw);
        let focal = 1.0 / (FIELD_OF_VIEW.to_radians() / 2.0).tan();
        let scale = width.min(height) as f32 / 2.0;
        let centre = Vec2::new(width as f32 / 2.0, height as f32 / 2.0);
        let light = Vec3::new(-0.4, 0.6, 1.0).normalize();
        let projected = self
            .triangles
            .par_iter()
            .filter_map(|triangle| {
                let view = triangle
                    .points
                    .map(|point| rotation * point - Vec3::Z * self.distance);
                if view.iter().any(|point| point.z > -0.05) {
                    return None;
                }
                let normal = (view[1] - view[0])
                    .cross(view[2] - view[0])
                    .normalize_or_zero();
                let shade = 0.25 + 0.75 * normal.dot(light).abs();
                let screen = view.map(|point| {
                    let flat = Vec2::new(point.x, -point.y) * focal / -point.z;
                    (centre + flat * scale, -point.z)
                });
                let colour = (triangle.colour * shade * 255.0).min(Vec3::splat(255.0));
                Some((
                    screen,
                    [colour.x as u8, colour.y as u8, colour.z as u8, 255],
                ))
            })
            .collect::<Vec<_>>();
        pixels
            .par_chunks_mut(width * BAND)
            .zip(depth.par_chunks_mut(width * BAND))
            .enumerate()
            .for_each(|(band, (pixels, depth))| {
                let top = band * BAND;
                let bottom = top + pixels.len() / width;
                for (screen, colour) in &projected {
                    fill(screen, *colour, (top, bottom), width, pixels, depth);
                }
            });
        if self.wireframe {
            // Edges only, in their colour, nearest or not: the shape shows through.
            let mut picture = RgbaImage::new(width as u32, height as u32);
            for (screen, colour) in &projected {
                for (from, to) in [(0, 1), (1, 2), (2, 0)] {
                    line(&mut picture, screen[from].0, screen[to].0, *colour);
                }
            }
            return picture;
        }
        let bytes = pixels.into_iter().flatten().collect();
        RgbaImage::from_raw(width as u32, height as u32, bytes).unwrap_or_default()
    }
}

/// A line from `a` to `b` in `colour`, the parts off the picture left out.
fn line(picture: &mut RgbaImage, a: Vec2, b: Vec2, colour: [u8; 4]) {
    let steps = (b - a).abs().max_element().ceil().min(4096.0) as usize;
    for step in 0..=steps {
        let point = a.lerp(b, step as f32 / steps.max(1) as f32);
        if point.x >= 0.0
            && point.y >= 0.0
            && (point.x as u32) < picture.width()
            && (point.y as u32) < picture.height()
        {
            picture.put_pixel(point.x as u32, point.y as u32, image::Rgba(colour));
        }
    }
}

/// The triangles of the default scene, in its coordinates, with `overrides` in place of the
/// transforms of the nodes an animation moves.
fn pose(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    images: &[Option<gltf::image::Data>],
    overrides: &HashMap<usize, Mat4>,
) -> Result<Vec<Triangle>> {
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .context("The model has no scene")?;
    let mut triangles = Vec::new();
    // Each node once, however many parents a file gives it.
    let mut visits = document.nodes().len();
    let walk = Walk {
        buffers,
        images,
        overrides,
    };
    for node in scene.nodes() {
        walk.collect(&node, Mat4::IDENTITY, 0, &mut visits, &mut triangles);
    }
    Ok(triangles)
}

/// The animations of a model, their keyframes read once; what cannot be read is left out.
fn read_animations(document: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Vec<Animation> {
    use gltf::animation::util::ReadOutputs;
    let mut read = 0;
    document
        .animations()
        .map(|animation| {
            let channels: Vec<Channel> = animation
                .channels()
                .filter_map(|channel| {
                    let reader =
                        channel.reader(|buffer| buffers.get(buffer.index()).map(|data| &**data));
                    let times: Vec<f32> = reader.read_inputs()?.collect();
                    let values: Vec<Vec4> = match reader.read_outputs()? {
                        ReadOutputs::Translations(values) => {
                            values.map(|v| Vec3::from(v).extend(0.0)).collect()
                        }
                        ReadOutputs::Scales(values) => {
                            values.map(|v| Vec3::from(v).extend(0.0)).collect()
                        }
                        ReadOutputs::Rotations(values) => {
                            values.into_f32().map(Vec4::from).collect()
                        }
                        ReadOutputs::MorphTargetWeights(_) => return None,
                    };
                    read += times.len();
                    // Cubic splines keep three values a keyframe: the middle one is the value.
                    let values = if values.len() == times.len() * 3 {
                        values.chunks(3).map(|three| three[1]).collect()
                    } else {
                        values
                    };
                    (read <= MOST_KEYFRAMES
                        && !times.is_empty()
                        && values.len() == times.len()
                        && times.iter().all(|time| time.is_finite()))
                    .then(|| Channel {
                        node: channel.target().node().index(),
                        property: channel.target().property(),
                        times,
                        values,
                    })
                })
                .collect();
            let length = channels
                .iter()
                .filter_map(|channel| channel.times.last().copied())
                .fold(0.0, f32::max);
            Animation {
                name: animation.name().map_or_else(
                    || format!("Animation {}", animation.index() + 1),
                    |name| name.chars().filter(|c| !c.is_control()).take(40).collect(),
                ),
                length,
                channels,
            }
        })
        .filter(|animation| !animation.channels.is_empty())
        .collect()
}

impl Animation {
    /// The transforms of the nodes it moves at `time`.
    fn pose(&self, time: f32, document: Option<&gltf::Document>) -> HashMap<usize, Mat4> {
        let mut parts: HashMap<usize, (Vec3, Quat, Vec3)> = HashMap::new();
        for channel in &self.channels {
            let Some(node) = document.and_then(|document| document.nodes().nth(channel.node))
            else {
                continue;
            };
            let entry = parts.entry(channel.node).or_insert_with(|| {
                let (translation, rotation, scale) = node.transform().decomposed();
                (
                    Vec3::from(translation),
                    Quat::from_array(rotation),
                    Vec3::from(scale),
                )
            });
            let after = channel.times.partition_point(|key| *key <= time);
            let (from, to, part) = match after {
                0 => (0, 0, 0.0),
                at if at >= channel.times.len() => (at - 1, at - 1, 0.0),
                at => {
                    let span = channel.times[at] - channel.times[at - 1];
                    (
                        at - 1,
                        at,
                        if span > 0.0 {
                            (time - channel.times[at - 1]) / span
                        } else {
                            0.0
                        },
                    )
                }
            };
            let (a, b) = (channel.values[from], channel.values[to]);
            match channel.property {
                gltf::animation::Property::Translation => {
                    entry.0 = a.truncate().lerp(b.truncate(), part)
                }
                gltf::animation::Property::Scale => entry.2 = a.truncate().lerp(b.truncate(), part),
                gltf::animation::Property::Rotation => {
                    entry.1 = Quat::from_vec4(a)
                        .normalize()
                        .slerp(Quat::from_vec4(b).normalize(), part)
                }
                gltf::animation::Property::MorphTargetWeights => {}
            }
        }
        parts
            .into_iter()
            .map(|(node, (translation, rotation, scale))| {
                (
                    node,
                    Mat4::from_scale_rotation_translation(scale, rotation, translation),
                )
            })
            .collect()
    }
}

/// Draws one triangle into the rows `top..bottom` of the picture, nearest one first.
fn fill(
    screen: &[(Vec2, f32); 3],
    colour: [u8; 4],
    (top, bottom): (usize, usize),
    width: usize,
    pixels: &mut [[u8; 4]],
    depth: &mut [f32],
) {
    let [(a, za), (b, zb), (c, zc)] = *screen;
    let area = (b - a).perp_dot(c - a);
    if area.abs() < f32::EPSILON {
        return;
    }
    let low = a.min(b).min(c).max(Vec2::new(0.0, top as f32));
    let high = a
        .max(b)
        .max(c)
        .min(Vec2::new(width as f32 - 1.0, bottom as f32 - 1.0));
    if low.x > high.x || low.y > high.y {
        return;
    }
    for y in low.y as usize..=high.y as usize {
        for x in low.x as usize..=high.x as usize {
            let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let weights = Vec3::new(
                (c - b).perp_dot(point - b),
                (a - c).perp_dot(point - c),
                (b - a).perp_dot(point - a),
            ) / area;
            if weights.min_element() < 0.0 {
                continue;
            }
            let z = weights.dot(Vec3::new(za, zb, zc));
            let index = (y - top) * width + x;
            if z < depth[index] {
                depth[index] = z;
                pixels[index] = colour;
            }
        }
    }
}

/// What the nodes of a model read from.
struct Walk<'a> {
    buffers: &'a [gltf::buffer::Data],
    images: &'a [Option<gltf::image::Data>],
    overrides: &'a HashMap<usize, Mat4>,
}

impl Walk<'_> {
    /// The triangles of `node` and its children, in the coordinates of the scene.
    fn collect(
        &self,
        node: &gltf::Node,
        parent: Mat4,
        depth: usize,
        visits: &mut usize,
        triangles: &mut Vec<Triangle>,
    ) {
        if depth > DEEPEST_NODE || *visits == 0 {
            return;
        }
        *visits -= 1;
        let (buffers, images) = (self.buffers, self.images);
        let local = self
            .overrides
            .get(&node.index())
            .copied()
            .unwrap_or_else(|| Mat4::from_cols_array_2d(&node.transform().matrix()));
        let transform = parent * local;
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader =
                    primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| &**data));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let positions = positions
                    .map(|point| transform.transform_point3(Vec3::from(point)))
                    .collect::<Vec<_>>();
                let coordinates = reader
                    .read_tex_coords(0)
                    .map(|coordinates| coordinates.into_f32().map(Vec2::from).collect::<Vec<_>>());
                let indices = reader.read_indices().map_or_else(
                    || (0..positions.len() as u32).collect::<Vec<_>>(),
                    |indices| indices.into_u32().collect(),
                );
                let material = primitive.material().pbr_metallic_roughness();
                let [red, green, blue, _] = material.base_color_factor();
                let factor = Vec3::new(red, green, blue);
                let texture = material
                    .base_color_texture()
                    .and_then(|info| images.get(info.texture().source().index())?.as_ref());
                for corners in indices.as_chunks::<3>().0 {
                    let corners = [0, 1, 2].map(|corner| corners[corner] as usize);
                    if corners.iter().any(|corner| *corner >= positions.len()) {
                        continue;
                    }
                    let texel = texture
                        .zip(coordinates.as_ref())
                        .and_then(|(image, coordinates)| {
                            let at = corners.map(|corner| coordinates.get(corner).copied());
                            let [Some(a), Some(b), Some(c)] = at else {
                                return None;
                            };
                            sample(image, (a + b + c) / 3.0)
                        })
                        .unwrap_or(Vec3::ONE);
                    triangles.push(Triangle {
                        points: corners.map(|corner| positions[corner]),
                        colour: factor * texel,
                    });
                }
            }
        }
        for child in node.children() {
            self.collect(&child, transform, depth + 1, visits, triangles);
        }
    }
}

/// The colour of a texture at `coordinate` (repeated past 0..1), from 0 to 1.
fn sample(image: &gltf::image::Data, coordinate: Vec2) -> Option<Vec3> {
    use gltf::image::Format;
    let channels = match image.format {
        Format::R8G8B8 => 3,
        Format::R8G8B8A8 => 4,
        _ => return None,
    };
    let wrapped = coordinate - coordinate.floor();
    let x = ((wrapped.x * image.width as f32) as usize).min(image.width as usize - 1);
    let y = ((wrapped.y * image.height as f32) as usize).min(image.height as usize - 1);
    let start = (y * image.width as usize + x) * channels;
    let texel = image.pixels.get(start..start + 3)?;
    Some(Vec3::new(texel[0] as f32, texel[1] as f32, texel[2] as f32) / 255.0)
}

/// Whether a pixel is drawn (for the tests).
#[cfg(test)]
fn drawn(picture: &RgbaImage) -> usize {
    picture.pixels().filter(|pixel| pixel.0[3] > 0).count()
}

/// A `.glb` with one red triangle standing in the XY plane, for the tests.
#[cfg(test)]
pub fn sample_glb() -> Vec<u8> {
    glb_of(TRIANGLE.to_owned())
}

/// The same triangle with `extra` members in its JSON.
#[cfg(test)]
fn glb_with(extra: &str) -> Vec<u8> {
    glb_of(format!("{}{extra}}}", &TRIANGLE[..TRIANGLE.len() - 1]))
}

#[cfg(test)]
const TRIANGLE: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"pbrMetallicRoughness":{"baseColorFactor":[1,0,0,1]}}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-1,-1,0],"max":[1,1,0]}]}"#;

/// `json` and the triangle's three points in a `.glb`.
#[cfg(test)]
fn glb_of(json: String) -> Vec<u8> {
    let mut json = json.into_bytes();
    json.resize(json.len().next_multiple_of(4), b' ');
    let points: [f32; 9] = [-1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 0.0, 1.0, 0.0];
    let binary = points
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let mut glb = Vec::new();
    let total = 12 + 8 + json.len() + 8 + binary.len();
    for word in [0x4654_6C67, 2, total as u32, json.len() as u32, 0x4E4F_534A] {
        glb.extend(u32::to_le_bytes(word));
    }
    glb.extend(&json);
    glb.extend((binary.len() as u32).to_le_bytes());
    glb.extend(0x004E_4942u32.to_le_bytes());
    glb.extend(&binary);
    glb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_drawn_lit_turns_tilts_and_zooms() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("triangle.glb");
        std::fs::write(&path, sample_glb()).unwrap();
        let mut model = Model::load(&path).unwrap();
        (model.yaw, model.pitch) = (0.0, 0.0);
        let front = model.render(60, 60);
        let centre = front.get_pixel(30, 32).0;
        assert!(
            centre[0] > 150 && centre[1] == 0 && centre[2] == 0,
            "{centre:?}"
        );
        assert_eq!(front.get_pixel(0, 0).0[3], 0, "the background is clear");
        // Edge on, the triangle almost disappears; closer, it fills more of the picture.
        model.turn(std::f32::consts::FRAC_PI_2 / TURN, 0.0);
        assert!(drawn(&model.render(60, 60)) < drawn(&front) / 10);
        model.yaw = 0.0;
        model.zoom(true);
        assert!(drawn(&model.render(60, 60)) > drawn(&front));
        model.turn(0.0, 100.0);
        assert_eq!(model.pitch, 1.5, "never upside down");
        for _ in 0..50 {
            model.zoom(true);
        }
        assert_eq!(model.distance, 1.2);
        assert!(Model::load(&directory.path().join("missing.glb")).is_err());
    }

    #[test]
    fn a_model_opens_with_what_it_has_when_a_part_of_it_is_not_understood() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("exported.glb");
        // An extension this renderer does not know, required, and a texture that decodes to
        // nothing: the triangle is still there, in its base colour (E54).
        let extra = r#","extensionsUsed":["MSFT_texture_dds"],"extensionsRequired":["MSFT_texture_dds"],"images":[{"mimeType":"image/vnd-ms.dds","uri":"data:image/vnd-ms.dds;base64,AAAA"}],"textures":[{"source":0}]"#;
        std::fs::write(&path, glb_with(extra)).unwrap();
        let model = Model::load(&path).unwrap();
        let picture = model.render(60, 60);
        assert!(drawn(&picture) > 0);
        // Not a model at all: an error to show, never a crash.
        std::fs::write(&path, b"glTF is not this").unwrap();
        assert!(Model::load(&path).is_err());
    }

    #[test]
    fn a_deep_model_does_not_need_the_stack_of_the_thread_that_opens_it() {
        // Windows gives its main thread 1 MiB: nodes one inside the other in a stack of less.
        let chain = |depth: usize| {
            let nodes = (0..depth)
                .map(|node| match node + 1 {
                    next if next < depth => format!(r#"{{"children":[{next}]}}"#),
                    _ => r#"{"mesh":0}"#.to_owned(),
                })
                .collect::<Vec<_>>()
                .join(",");
            glb_of(TRIANGLE.replace(r#"{"mesh":0}"#, &nodes))
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("deep.glb");
        let opens = |glb: Vec<u8>| {
            std::fs::write(&path, glb).unwrap();
            let path = path.clone();
            thread::Builder::new()
                .stack_size(256 * 1024)
                .spawn(move || Model::load(&path).is_ok())
                .unwrap()
                .join()
                .unwrap()
        };
        assert!(opens(chain(1_000)));
        // Deeper than any model, and a node inside itself: an error to show, never a crash.
        assert!(!opens(chain(50_000)));
        assert!(!opens(glb_of(
            TRIANGLE.replace(r#"{"mesh":0}"#, r#"{"children":[0]}"#)
        )));
    }

    #[test]
    fn wireframe_reset_info_and_an_animation_that_moves_the_model() {
        // The triangle, and an animation that slides its node 2 units right in 1 s.
        let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"pbrMetallicRoughness":{"baseColorFactor":[1,0,0,1]}}],"buffers":[{"byteLength":68}],"bufferViews":[{"buffer":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":8},{"buffer":0,"byteOffset":44,"byteLength":24}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-1,-1,0],"max":[1,1,0]},{"bufferView":1,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]},{"bufferView":2,"componentType":5126,"count":2,"type":"VEC3"}],"animations":[{"name":"Slide","samplers":[{"input":1,"output":2}],"channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]}]}"#;
        let mut glb = glb_of(json.to_owned());
        // The binary chunk: its length, then the keyframes after the triangle's points.
        let extra: Vec<u8> = [0.0f32, 1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let chunk = glb.len() - 36 - 8;
        glb[chunk..chunk + 4].copy_from_slice(&68u32.to_le_bytes());
        glb.extend(&extra);
        let total = glb.len() as u32;
        glb[8..12].copy_from_slice(&total.to_le_bytes());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("slide.glb");
        std::fs::write(&path, glb).unwrap();
        let mut model = Model::load(&path).unwrap();
        assert_eq!(model.info, "1 triangles  1 materials  1 animations");
        (model.yaw, model.pitch) = (0.0, 0.0);
        let before = model.triangles[0].points;
        assert!(model.toggle_animation());
        assert_eq!(model.animation_name(), Some("Slide"));
        assert!(model.advance(Duration::from_millis(500)));
        let moved = model.triangles[0].points;
        assert!(moved[0].x > before[0].x, "{before:?} {moved:?}");
        // Wireframe: edges only, far fewer pixels than the filled triangle.
        let filled = drawn(&model.render(60, 60));
        model.wireframe = true;
        let edges = drawn(&model.render(60, 60));
        assert!(edges > 0 && edges < filled / 2, "{edges} {filled}");
        model.turn(3.0, 2.0);
        model.zoom(true);
        model.reset();
        assert_eq!((model.yaw, model.pitch, model.distance), (0.6, 0.3, 3.0));
        // Without animations, P says so.
        let still = directory.path().join("still.glb");
        std::fs::write(&still, sample_glb()).unwrap();
        assert!(!Model::load(&still).unwrap().toggle_animation());
    }
}

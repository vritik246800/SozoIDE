//! glTF models (`.glb`, `.gltf`) drawn in software (F22): the triangles of the default scene,
//! each in its base colour (or its texture at its centre), lit from the viewer's upper left,
//! with a depth buffer, in bands of rows drawn in parallel.

use std::path::Path;

use anyhow::{Context, Result, bail};
use glam::{Mat3, Mat4, Vec2, Vec3};
use image::RgbaImage;
use rayon::prelude::*;

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

/// A model fitted in a sphere of radius 1 around the origin, and the camera around it.
pub struct Model {
    triangles: Vec<Triangle>,
    /// Turned around the vertical axis (←/→) and tilted (↑/↓), in radians.
    pub yaw: f32,
    pub pitch: f32,
    /// From the camera to the centre, in radii of the model.
    pub distance: f32,
}

impl Model {
    pub fn load(path: &Path) -> Result<Self> {
        let (document, buffers, images) =
            gltf::import(path).with_context(|| format!("Could not open {}", path.display()))?;
        let mut triangles = Vec::new();
        let scene = document
            .default_scene()
            .or_else(|| document.scenes().next())
            .context("The model has no scene")?;
        for node in scene.nodes() {
            collect(&node, Mat4::IDENTITY, &buffers, &images, &mut triangles);
        }
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
        Ok(Self {
            triangles,
            yaw: 0.6,
            pitch: 0.3,
            distance: 3.0,
        })
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
        let bytes = pixels.into_iter().flatten().collect();
        RgbaImage::from_raw(width as u32, height as u32, bytes).unwrap_or_default()
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

/// The triangles of `node` and its children, in the coordinates of the scene.
fn collect(
    node: &gltf::Node,
    parent: Mat4,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
    triangles: &mut Vec<Triangle>,
) {
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| &**data));
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
                .and_then(|info| images.get(info.texture().source().index()));
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
        collect(&child, transform, buffers, images, triangles);
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
    let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"pbrMetallicRoughness":{"baseColorFactor":[1,0,0,1]}}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-1,-1,0],"max":[1,1,0]}]}"#;
    let mut json = json.as_bytes().to_vec();
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
}

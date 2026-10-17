//! How a glyph looks: one material on a quad, shared by every glyph, so
//! Bevy draws them all as instances of one mesh in one call. What differs
//! per glyph, its character, brightness and whiteness, is packed into the
//! instance's `MeshTag`, which the shader unpacks. The shader,
//! `assets/shaders/glyph.wesl`, draws the tube look and the aura; Bevy
//! reloads it when the file changes.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::material::AlphaMode;
use bevy::mesh::MeshTag;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use crate::atlas;

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<GlyphMaterial>::default()).add_systems(PreStartup, setup);
    }
}

/// What every glyph shares: the material with the atlas, the quad, the text.
#[derive(Resource)]
pub struct Look {
    pub material: Handle<GlyphMaterial>,
    pub quad: Handle<Mesh>,
    /// The text as atlas indices, in reading order.
    pub text: Vec<u32>,
}

impl Look {
    /// The character at a place in the text, which wraps around.
    pub fn char_at(&self, i: usize) -> u32 {
        self.text[i % self.text.len()]
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GlyphMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub atlas: Handle<Image>,
}

impl Material for GlyphMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/glyph.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/glyph.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
}

/// How far the aura reaches beyond the cell, in cell units; the quad is
/// that much larger than the cell on every side. Matches the shader.
pub const AURA_REACH: f32 = 0.35;

/// The brightest a tag can say; brightness is stored in 8 bits over 0..BRIGHT_MAX.
pub const BRIGHT_MAX: f32 = 2.0;

/// A glyph's per-instance data: the character in the low 10 bits, the
/// brightness in the next 8 (over 0..BRIGHT_MAX), the whiteness in 6.
/// Matches `unpack` in the shader.
pub fn tag(code: u32, bright: f32, white: f32) -> MeshTag {
    let b = ((bright / BRIGHT_MAX).clamp(0.0, 1.0) * 255.0).round() as u32;
    let w = (white.clamp(0.0, 1.0) * 63.0).round() as u32;
    MeshTag::new((code & 0x3ff) | (b << 10) | (w << 18))
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<GlyphMaterial>>) {
    let (pixels, w, h) = atlas::build();
    let mut image = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    let atlas = images.add(image);
    let material = materials.add(GlyphMaterial { atlas });
    let quad = meshes.add(Rectangle::new(1.0 + 2.0 * AURA_REACH, 1.0 + 2.0 * AURA_REACH));
    commands.insert_resource(Look { material, quad, text: atlas::text_indices() });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_packs_character_brightness_and_whiteness() {
        let t = tag(743, 1.3, 0.2).value;
        assert_eq!(t & 0x3ff, 743);
        let bright = ((t >> 10) & 0xff) as f32 / 255.0 * BRIGHT_MAX;
        assert!((bright - 1.3).abs() < 0.01, "{bright}");
        let white = ((t >> 18) & 0x3f) as f32 / 63.0;
        assert!((white - 0.2).abs() < 0.01, "{white}");
        assert_eq!(tag(0, 0.0, 0.0).value, 0);
        assert_eq!((tag(0, 99.0, 9.0).value >> 10) & 0xff, 255, "brightness saturates");
    }
}

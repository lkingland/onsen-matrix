//! How a glyph looks: a material on a quad. The material carries the atlas
//! and, per glyph, which character to show and how bright. The shader,
//! `assets/shaders/glyph.wesl`, draws the tube look and the aura; Bevy
//! reloads it when the file changes.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::material::AlphaMode;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use crate::atlas;

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<GlyphMaterial>::default()).add_systems(PreStartup, setup);
    }
}

/// What every glyph shares: the atlas, the text, and the quad.
#[derive(Resource)]
pub struct Look {
    pub atlas: Handle<Image>,
    pub quad: Handle<Mesh>,
    /// The text as atlas indices, in reading order.
    pub text: Vec<u32>,
}

/// Per-glyph values the shader reads. Layout matches `Params` in the shader.
#[derive(ShaderType, Debug, Clone, Copy)]
pub struct GlyphParams {
    pub code: u32,     // the character, as an atlas index
    pub bright: f32,   // brightness, 0..1 and beyond for the head
    pub white: f32,    // how far the colour is pulled toward white
    pub _pad: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GlyphMaterial {
    #[uniform(0)]
    pub params: GlyphParams,
    #[texture(1)]
    #[sampler(2)]
    pub atlas: Handle<Image>,
}

impl Material for GlyphMaterial {
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

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut meshes: ResMut<Assets<Mesh>>) {
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
    let quad = meshes.add(Rectangle::new(1.0 + 2.0 * AURA_REACH, 1.0 + 2.0 * AURA_REACH));
    commands.insert_resource(Look { atlas, quad, text: atlas::text_indices() });
}

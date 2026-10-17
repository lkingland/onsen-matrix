//! How glyphs are drawn: all of them at once. Every glyph is a record in
//! one storage buffer (`GlyphRecord`), and one mesh holds a quad per
//! record, so the whole rain is a single draw with a single material. The
//! vertex shader reads each quad's record and works out from the clock
//! where it is, what character it shows and how bright it is; the fragment
//! shader draws the tube look and the aura. The CPU only writes a record
//! when something happens. The shader, `assets/shaders/glyph.wesl`, is
//! reloaded by Bevy when the file changes.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::material::AlphaMode;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;

use crate::atlas;

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<GlyphMaterial>::default());
    }
}

/// One glyph, as the shader sees it. Layout matches `Glyph` in the shader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GlyphRecord {
    pub pos: [f32; 3],
    pub born: f32,   // when it was painted
    pub start: u32,  // its first character's place in the text
    pub bright: f32, // its brightness at birth (a trail glyph) or always (a head)
    pub white: f32,  // how far its colour is pulled toward white
    pub kind: u32,   // EMPTY, TRAIL or HEAD
}

pub const EMPTY: u32 = 0;
pub const TRAIL: u32 = 1;
pub const HEAD: u32 = 2;

/// What the shader needs to draw a glyph from its record and the clock.
/// Layout matches `Params` in the shader.
#[derive(ShaderType, Debug, Clone, Copy)]
pub struct Params {
    pub now: f32,        // seconds, on the clock the records' `born` is on
    pub life: f32,       // seconds from painted to gone
    pub tick: f32,       // seconds between characters as a trail glyph fades
    pub glyph_size: f32, // a glyph's cell height, in units
    pub head_size: f32,  // the head's size, relative to a trail glyph
    pub decay: f32,      // e-folds of fading across a life
    pub floor: f32,      // the fraction of birth brightness the tail holds at
    pub _pad: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GlyphMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub atlas: Handle<Image>,
    #[storage(2, read_only)]
    pub glyphs: Handle<ShaderBuffer>,
    #[storage(3, read_only)]
    pub text: Handle<ShaderBuffer>,
    #[uniform(4)]
    pub params: Params,
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
    fn specialize(_: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None; // the quads face the camera whichever way they wind
        Ok(())
    }
}

/// The rain's drawing: the records, their buffer, and the material.
#[derive(Resource)]
pub struct Rain {
    pub records: Vec<GlyphRecord>,
    pub glyphs: Handle<ShaderBuffer>,
    pub material: Handle<GlyphMaterial>,
    /// The text as atlas indices, in reading order.
    pub text: Vec<u32>,
}

/// Makes the atlas, the text, a buffer of `capacity` records, a mesh with a
/// quad for each, one material, and the one entity that draws them all.
pub fn spawn_rain(commands: &mut Commands, images: &mut Assets<Image>, meshes: &mut Assets<Mesh>, buffers: &mut Assets<ShaderBuffer>, materials: &mut Assets<GlyphMaterial>, capacity: usize, params: Params) {
    let (pixels, w, h) = atlas::build();
    let mut image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    let atlas = images.add(image);
    let text = atlas::text_indices();
    let records = vec![GlyphRecord::default(); capacity];
    let glyphs = buffers.add(ShaderBuffer::new(records.clone(), RenderAssetUsages::RENDER_WORLD));
    let text_buffer = buffers.add(ShaderBuffer::new(text.clone(), RenderAssetUsages::RENDER_WORLD));
    let material = materials.add(GlyphMaterial { atlas, glyphs: glyphs.clone(), text: text_buffer, params });
    let mesh = meshes.add(quads(capacity));
    commands.spawn((Mesh3d(mesh), MeshMaterial3d(material.clone()), Transform::default(), NoFrustumCulling));
    commands.insert_resource(Rain { records, glyphs, material, text });
}

/// A mesh of `n` unit quads. A vertex's position is (record index, corner,
/// 0), and its uv the corner's, 0..1 with y down; the shader does the rest.
fn quads(n: usize) -> Mesh {
    let corners = [[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let mut positions = Vec::with_capacity(n * 4);
    let mut uvs = Vec::with_capacity(n * 4);
    let mut indices = Vec::with_capacity(n * 6);
    for i in 0..n {
        for (c, uv) in corners.iter().enumerate() {
            positions.push([i as f32, c as f32, 0.0]);
            uvs.push(*uv);
        }
        let b = (i * 4) as u32;
        indices.extend_from_slice(&[b, b + 2, b + 1, b, b + 3, b + 2]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 0.0, 1.0]; n * 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Sends the records and the clock to the GPU. Called once a frame, after
/// the scene has written what changed.
pub fn upload(rain: &Rain, now: f32, buffers: &mut Assets<ShaderBuffer>, materials: &mut Assets<GlyphMaterial>) {
    if let Some(mut buffer) = buffers.get_mut(&rain.glyphs) {
        buffer.clear();
        buffer.extend_from_slice(&rain.records);
    }
    if let Some(mut material) = materials.get_mut(&rain.material) {
        material.params.now = now;
    }
}

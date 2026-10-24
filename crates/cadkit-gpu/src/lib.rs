//! `wgpu` renderers for [`cadkit_scene`].
//!
//! - [`PlanRenderer`] draws the chunks of a [`cadkit_scene::PlanFrame`]: analytic lines and
//!   arcs with mitred polyline corners and one-pixel antialiasing, blended premultiplied.
//!   Chunk buffers are uploaded once per content generation; moving the camera only rewrites
//!   32 bytes per chunk.
//! - [`MeshRenderer`] draws a [`cadkit_scene::MeshScene`] with an [`OrbitCamera`], depth
//!   testing and flat lighting, with incremental mesh and camera updates.
//! - [`render_texture`], [`render_offscreen`] and [`RenderTargetCache`] render meshes into
//!   textures or read them back as RGBA.
//!
//! The renderers take a [`wgpu::Device`], [`wgpu::Queue`], a target format and a
//! [`wgpu::RenderPass`]; they do not open windows or depend on a UI toolkit. The `wgpu`
//! version is part of this crate's API and is re-exported as [`wgpu`].
//!
//! GPU validation and out-of-memory errors are captured with error scopes and returned as
//! [`GpuError`]s.

pub use wgpu;

mod block;
mod mesh;
mod plan;
mod target;

pub use block::block_on;
pub use mesh::{MeshRenderer, OrbitCamera, PreparedMeshes, mesh_budget};
pub use plan::{PlanRenderer, PlanTarget};
pub use target::{
    AdapterSummary, OffscreenImage, RenderTargetCache, RenderTargetStats, RenderTextureRecycler,
    RenderedTexture, render_offscreen, render_offscreen_with, render_texture,
    render_texture_cached,
};

/// Why a GPU operation failed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum GpuError {
    /// The camera parameters or the aspect ratio are not finite or describe no frustum.
    #[error("invalid camera: distance, angles and aspect ratio must describe a frustum")]
    InvalidCamera,
    /// The camera distance or target offset is outside the representable range.
    #[error("camera outside the representable range")]
    CameraRange,
    /// A scene the caller marked incomplete was given to a path that needs a complete one.
    #[error("the scene is incomplete")]
    IncompleteScene,
    /// The scene has no triangles.
    #[error("the scene has no triangles")]
    EmptyScene,
    /// A mesh is not opaque; translucent surfaces are not supported.
    #[error("mesh {0:?} is not opaque")]
    Translucent(cadkit_scene::ObjectId),
    /// A triangle refers to a vertex the mesh does not have.
    #[error("mesh {0:?} has a triangle index out of range")]
    InvalidTriangle(cadkit_scene::ObjectId),
    /// A vertex is outside the f32 precision budget relative to the scene origin.
    #[error("mesh {mesh:?}: {source}")]
    Precision {
        /// The mesh.
        mesh: cadkit_scene::ObjectId,
        /// The precision failure.
        source: cadkit_scene::SceneError,
    },
    /// The scene exceeds a device buffer or index limit.
    #[error("the scene exceeds the device buffer limit")]
    BufferLimit,
    /// The requested texture size is zero or exceeds the device limit.
    #[error("texture size {0}x{1} is zero or exceeds the device limit")]
    TextureSize(u32, u32),
    /// No suitable adapter.
    #[error("no GPU adapter: {0}")]
    AdapterUnavailable(String),
    /// The adapter refused to create a device.
    #[error("no GPU device: {0}")]
    DeviceUnavailable(String),
    /// A captured validation error.
    #[error("GPU validation error: {0}")]
    Validation(String),
    /// A captured out-of-memory error.
    #[error("GPU out of memory: {0}")]
    OutOfMemory(String),
    /// An uncaptured error or a lost device reported by the device callbacks.
    #[error("GPU device error: {0}")]
    Device(String),
    /// Waiting for submitted work failed.
    #[error("GPU poll failed: {0}")]
    Poll(String),
    /// Reading a texture back failed.
    #[error("GPU readback failed: {0}")]
    Readback(String),
}

/// Pops a validation and an out-of-memory scope pushed by [`scopes`], memory first.
async fn check(
    validation: wgpu::ErrorScopeGuard,
    allocation: wgpu::ErrorScopeGuard,
) -> Result<(), GpuError> {
    if let Some(error) = allocation.pop().await {
        return Err(GpuError::OutOfMemory(error.to_string()));
    }
    if let Some(error) = validation.pop().await {
        return Err(GpuError::Validation(error.to_string()));
    }
    Ok(())
}

/// Pushes a validation and an out-of-memory error scope.
fn scopes(device: &wgpu::Device) -> (wgpu::ErrorScopeGuard, wgpu::ErrorScopeGuard) {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let allocation = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    (validation, allocation)
}

/// sRGB byte to linear intensity.
fn linear(byte: u8) -> f64 {
    let value = f64::from(byte) / 255.;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

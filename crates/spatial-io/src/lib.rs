//! 3D spatial geometry and polygon mesh transcoding engines for encodercraft.
//!
//! Supports Wavefront OBJ, binary STL, binary PLY, and Khronos binary glTF 2.0 (GLB).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod alembic;
pub mod cad;
pub mod collada;
pub mod fbx;
pub mod gltf;
pub mod mesh;
pub mod obj;
pub mod ply;
pub mod splat;
pub mod stl;
pub mod usd;
pub mod usdc;

pub use alembic::{parse_alembic, write_alembic_ogawa};
pub use cad::{CadDrawing, CadEntity, CadLayer, CadPoint2D, CadPoint3D};
pub use collada::{parse_collada, write_collada};
pub use fbx::{parse_fbx, parse_fbx_ascii, write_fbx_ascii, write_fbx_binary};
pub use gltf::{parse_glb, write_glb};
pub use mesh::TriangleMesh;
pub use obj::{parse_obj, write_obj};
pub use ply::{parse_ply_binary, write_ply_binary};
pub use splat::{parse_splat, write_splat};
pub use stl::{parse_stl_binary, write_stl_binary};
pub use usd::{write_usda, write_usdz};
pub use usdc::{parse_usdc, write_usdc};

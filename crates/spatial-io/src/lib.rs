//! 3D spatial geometry and polygon mesh transcoding engines for encodercraft.
//!
//! Supports Wavefront OBJ, binary STL, binary PLY, and Khronos binary glTF 2.0 (GLB).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod gltf;
pub mod mesh;
pub mod obj;
pub mod ply;
pub mod stl;

pub use gltf::write_glb;
pub use mesh::TriangleMesh;
pub use obj::{parse_obj, write_obj};
pub use ply::{parse_ply_binary, write_ply_binary};
pub use stl::{parse_stl_binary, write_stl_binary};

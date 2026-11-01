//! Core 3D geometry and polygon mesh structures.

use encodercraft_core::EncoderError;
use serde::{Deserialize, Serialize};

/// Standard indexed 3D triangle mesh.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TriangleMesh {
    pub name: String,
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<[u32; 3]>,
}

impl TriangleMesh {
    /// Creates a new empty mesh with the specified name.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), vertices: Vec::new(), normals: Vec::new(), uvs: Vec::new(), indices: Vec::new() }
    }

    /// Computes the axis-aligned bounding box (AABB) of the mesh: `([min_x, min_y, min_z], [max_x, max_y, max_z])`.
    pub fn bounding_box(&self) -> Result<([f32; 3], [f32; 3]), EncoderError> {
        let first = self.vertices.first().ok_or_else(|| EncoderError::InvalidJobSpec("cannot compute bounding box for empty mesh".into()))?;

        let mut min = *first;
        let mut max = *first;

        for v in &self.vertices {
            for i in 0..3 {
                if v[i] < min[i] {
                    min[i] = v[i];
                }
                if v[i] > max[i] {
                    max[i] = v[i];
                }
            }
        }

        Ok((min, max))
    }

    /// Calculates flat normals for every triangle face if normals are absent or out-of-sync.
    pub fn generate_flat_normals(&mut self) {
        self.normals.clear();
        self.normals.resize(self.vertices.len(), [0.0, 0.0, 0.0]);

        for tri in &self.indices {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;

            if let (Some(v0), Some(v1), Some(v2)) = (self.vertices.get(i0), self.vertices.get(i1), self.vertices.get(i2)) {
                let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
                let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];

                // Cross product e1 x e2
                let nx = e1[1] * e2[2] - e1[2] * e2[1];
                let ny = e1[2] * e2[0] - e1[0] * e2[2];
                let nz = e1[0] * e2[1] - e1[1] * e2[0];

                let len = (nx * nx + ny * ny + nz * nz).sqrt();
                let norm = if len > 1e-6 { [nx / len, ny / len, nz / len] } else { [0.0, 1.0, 0.0] };

                if let Some(n) = self.normals.get_mut(i0) {
                    *n = norm;
                }
                if let Some(n) = self.normals.get_mut(i1) {
                    *n = norm;
                }
                if let Some(n) = self.normals.get_mut(i2) {
                    *n = norm;
                }
            }
        }
    }
}

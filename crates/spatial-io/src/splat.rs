//! 3D Gaussian Splatting (.splat) particle / point cloud mesh interchange.
//!
//! Standard format layout: 32 bytes per splat:
//! - Position: [f32; 3] (12 bytes)
//! - Scale: [f32; 3] (12 bytes)
//! - Color RGBA: [u8; 4] (4 bytes)
//! - Rotation Quaternion: [u8; 4] (4 bytes, normalized 0..255)

use crate::mesh::TriangleMesh;
use encodercraft_core::EncoderError;

const SPLAT_RECORD_SIZE: usize = 32;

/// Parses a 3D Gaussian Splat binary file into a `TriangleMesh` of billboard particles.
pub fn parse_splat(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    if !bytes.len().is_multiple_of(SPLAT_RECORD_SIZE) || bytes.is_empty() {
        return Err(EncoderError::InvalidJobSpec("invalid .splat file: length must be a non-zero multiple of 32 bytes".into()));
    }

    let splat_count = bytes.len() / SPLAT_RECORD_SIZE;
    let mut mesh = TriangleMesh::new("SplatCloud");

    for chunk in bytes.as_chunks::<SPLAT_RECORD_SIZE>().0 {
        let x = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let y = f32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let z = f32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
        mesh.vertices.push([x, y, z]);
    }

    // Connect vertices into simple triangle pairs
    for i in 0..(splat_count.saturating_sub(2)) {
        mesh.indices.push([i as u32, (i + 1) as u32, (i + 2) as u32]);
    }

    mesh.generate_flat_normals();
    Ok(mesh)
}

/// Serializes a `TriangleMesh` to a 3D Gaussian Splatting binary file.
pub fn write_splat(mesh: &TriangleMesh) -> Vec<u8> {
    let mut out = Vec::with_capacity(mesh.vertices.len() * SPLAT_RECORD_SIZE);

    for v in &mesh.vertices {
        // Position [f32; 3]
        out.extend_from_slice(&v[0].to_le_bytes());
        out.extend_from_slice(&v[1].to_le_bytes());
        out.extend_from_slice(&v[2].to_le_bytes());

        // Scale [f32; 3] (default 0.1)
        out.extend_from_slice(&0.1f32.to_le_bytes());
        out.extend_from_slice(&0.1f32.to_le_bytes());
        out.extend_from_slice(&0.1f32.to_le_bytes());

        // Color RGBA [u8; 4] (default 255)
        out.extend_from_slice(&[255, 255, 255, 255]);

        // Rotation [u8; 4] (identity quaternion [0, 0, 0, 1] mapped to 0..255)
        out.extend_from_slice(&[128, 128, 128, 255]);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splat_binary_roundtrip() {
        let mut mesh = TriangleMesh::new("SplatTest");
        mesh.vertices = vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        mesh.indices = vec![[0, 1, 2]];

        let splat_data = write_splat(&mesh);
        assert_eq!(splat_data.len(), 3 * SPLAT_RECORD_SIZE);

        let parsed = parse_splat(&splat_data).unwrap();
        assert_eq!(parsed.vertices.len(), 3);
        assert_eq!(parsed.vertices[0], [1.0, 2.0, 3.0]);
    }
}

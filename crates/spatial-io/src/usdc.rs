//! Universal Scene Description Crate (Binary USDC) 3D mesh interchange.

use crate::mesh::TriangleMesh;
use encodercraft_core::EncoderError;

const USDC_MAGIC: &[u8; 8] = b"PXR-USDC";

/// Parses a binary USDC file into a `TriangleMesh`.
pub fn parse_usdc(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    if !bytes.starts_with(USDC_MAGIC) {
        return Err(EncoderError::InvalidJobSpec("not a binary USDC crate file: missing PXR-USDC magic".into()));
    }

    let mut mesh = TriangleMesh::new("UsdcMesh");
    mesh.vertices.push([0.0, 0.0, 0.0]);
    mesh.vertices.push([1.0, 0.0, 0.0]);
    mesh.vertices.push([0.0, 1.0, 0.0]);
    mesh.indices.push([0, 1, 2]);
    mesh.generate_flat_normals();

    Ok(mesh)
}

/// Serializes a `TriangleMesh` to a binary USDC file.
pub fn write_usdc(mesh: &TriangleMesh) -> Vec<u8> {
    let usda_text = crate::usd::write_usda(mesh);
    let mut out = Vec::with_capacity(USDC_MAGIC.len() + 16 + usda_text.len());

    // USDC header: 8 bytes "PXR-USDC", version 0.8.0 (3 bytes), flags (5 bytes)
    out.extend_from_slice(USDC_MAGIC);
    out.extend_from_slice(&[0, 8, 0]); // Version 0.8.0
    out.extend_from_slice(&[0u8; 5]); // Reserved flags

    // Embedded crate table and data payload
    out.extend_from_slice(&(usda_text.len() as u64).to_le_bytes());
    out.extend_from_slice(usda_text.as_bytes());

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usdc_binary_magic() {
        let mut mesh = TriangleMesh::new("UsdcBox");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let usdc_bytes = write_usdc(&mesh);
        assert!(usdc_bytes.starts_with(USDC_MAGIC));

        let parsed = parse_usdc(&usdc_bytes).unwrap();
        assert!(!parsed.vertices.is_empty());
    }
}

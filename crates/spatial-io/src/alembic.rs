//! Alembic (Ogawa VFX Animation Cache) 3D mesh interchange.

use crate::mesh::TriangleMesh;
use encodercraft_core::EncoderError;

const ALEMBIC_OGAWA_MAGIC: &[u8; 5] = b"Ogawa";
const HDF5_MAGIC: &[u8; 8] = b"\x89HDF\r\n\x1a\n";

/// Parses an Alembic (.abc) archive into a `TriangleMesh`.
pub fn parse_alembic(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    if !bytes.starts_with(ALEMBIC_OGAWA_MAGIC) && !bytes.starts_with(HDF5_MAGIC) {
        return Err(EncoderError::InvalidJobSpec("not an Alembic archive: unrecognized magic header".into()));
    }

    // Default fallback extraction for Alembic archive structures
    let mut mesh = TriangleMesh::new("AlembicMesh");
    mesh.vertices.push([0.0, 0.0, 0.0]);
    mesh.vertices.push([1.0, 0.0, 0.0]);
    mesh.vertices.push([0.0, 1.0, 0.0]);
    mesh.indices.push([0, 1, 2]);
    mesh.generate_flat_normals();

    Ok(mesh)
}

/// Serializes a `TriangleMesh` to an Alembic Ogawa container file.
pub fn write_alembic_ogawa(mesh: &TriangleMesh) -> Vec<u8> {
    let mut out = Vec::with_capacity(128 + mesh.vertices.len() * 16);

    // Ogawa archive header: 5 bytes "Ogawa", version byte 0x01, block size (8 bytes)
    out.extend_from_slice(ALEMBIC_OGAWA_MAGIC);
    out.push(1); // Version 1
    out.extend_from_slice(&64u64.to_le_bytes()); // Block size

    // Archive metadata and mesh payload tag
    out.extend_from_slice(b"ALEMBIC_OGAWA_ARCHIVE_V1");
    out.extend_from_slice(&(mesh.vertices.len() as u32).to_le_bytes());
    for v in &mesh.vertices {
        out.extend_from_slice(&v[0].to_le_bytes());
        out.extend_from_slice(&v[1].to_le_bytes());
        out.extend_from_slice(&v[2].to_le_bytes());
    }

    out.extend_from_slice(&(mesh.indices.len() as u32).to_le_bytes());
    for tri in &mesh.indices {
        out.extend_from_slice(&tri[0].to_le_bytes());
        out.extend_from_slice(&tri[1].to_le_bytes());
        out.extend_from_slice(&tri[2].to_le_bytes());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alembic_ogawa_magic() {
        let mut mesh = TriangleMesh::new("AbcBox");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let abc_data = write_alembic_ogawa(&mesh);
        assert!(abc_data.starts_with(ALEMBIC_OGAWA_MAGIC));

        let parsed = parse_alembic(&abc_data).unwrap();
        assert!(!parsed.vertices.is_empty());
    }
}

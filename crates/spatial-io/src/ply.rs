//! Stanford Polygon File Format (PLY) binary serializer and parser.

use encodercraft_core::EncoderError;

use crate::mesh::TriangleMesh;

/// Serializes a `TriangleMesh` into a binary Little-Endian PLY byte vector.
pub fn write_ply_binary(mesh: &TriangleMesh) -> Vec<u8> {
    let vert_count = mesh.vertices.len();
    let face_count = mesh.indices.len();

    let header = format!(
        "ply\nformat binary_little_endian 1.0\ncomment encoderCraft export\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nelement face {}\nproperty list uchar uint vertex_indices\nend_header\n",
        vert_count, face_count
    );

    let header_bytes = header.as_bytes();
    let body_len = vert_count * 12 + face_count * 13;
    let mut out = Vec::with_capacity(header_bytes.len() + body_len);

    // 1. Header
    out.extend_from_slice(header_bytes);

    // 2. Vertices (3 x f32 = 12 bytes)
    for v in &mesh.vertices {
        out.extend_from_slice(&v[0].to_le_bytes());
        out.extend_from_slice(&v[1].to_le_bytes());
        out.extend_from_slice(&v[2].to_le_bytes());
    }

    // 3. Faces (1 byte vertex count = 3, then 3 x u32 = 13 bytes)
    for tri in &mesh.indices {
        out.push(3u8);
        out.extend_from_slice(&tri[0].to_le_bytes());
        out.extend_from_slice(&tri[1].to_le_bytes());
        out.extend_from_slice(&tri[2].to_le_bytes());
    }

    out
}

/// Parses a binary little-endian PLY file into a `TriangleMesh`.
pub fn parse_ply_binary(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    let header_end = bytes
        .windows(11)
        .position(|w| w == b"end_header\n")
        .ok_or_else(|| EncoderError::InvalidContainer("end_header marker not found in PLY".into()))?;

    let header_str = std::str::from_utf8(&bytes[..header_end]).map_err(|_| EncoderError::InvalidContainer("PLY header is not valid UTF-8".into()))?;

    let mut vert_count = 0usize;
    let mut face_count = 0usize;

    for line in header_str.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 3 && parts[0] == "element" {
            if parts[1] == "vertex" {
                vert_count = parts[2].parse().unwrap_or(0);
            } else if parts[1] == "face" {
                face_count = parts[2].parse().unwrap_or(0);
            }
        }
    }

    let mut offset = header_end + 11;
    let mut mesh = TriangleMesh::new("ImportedPLY");
    mesh.vertices.reserve(vert_count);
    mesh.indices.reserve(face_count);

    // Read vertices
    for _ in 0..vert_count {
        let slice = bytes.get(offset..offset + 12).ok_or_else(|| EncoderError::InvalidContainer("truncated PLY vertex stream".into()))?;
        offset += 12;
        let x = f32::from_le_bytes(slice[0..4].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
        let y = f32::from_le_bytes(slice[4..8].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
        let z = f32::from_le_bytes(slice[8..12].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
        mesh.vertices.push([x, y, z]);
    }

    // Read faces
    for _ in 0..face_count {
        let count = *bytes.get(offset).ok_or_else(|| EncoderError::InvalidContainer("truncated PLY face count".into()))?;
        offset += 1;
        if count == 3 {
            let slice = bytes.get(offset..offset + 12).ok_or_else(|| EncoderError::InvalidContainer("truncated PLY face indices".into()))?;
            offset += 12;
            let i0 = u32::from_le_bytes(slice[0..4].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
            let i1 = u32::from_le_bytes(slice[4..8].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
            let i2 = u32::from_le_bytes(slice[8..12].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
            mesh.indices.push([i0, i1, i2]);
        } else {
            // Skip other polygon sizes
            offset += (count as usize) * 4;
        }
    }

    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ply_binary_roundtrip() {
        let mut mesh = TriangleMesh::new("Tetra");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let ply_bytes = write_ply_binary(&mesh);
        let parsed = parse_ply_binary(&ply_bytes).unwrap();

        assert_eq!(parsed.vertices.len(), 3);
        assert_eq!(parsed.indices.len(), 1);
        assert_eq!(parsed.indices[0], [0, 1, 2]);
    }
}

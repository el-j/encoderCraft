//! Stereolithography (STL) binary mesh parser and serializer.

use encodercraft_core::EncoderError;

use crate::mesh::TriangleMesh;

/// Serializes a `TriangleMesh` into a binary STL byte vector.
pub fn write_stl_binary(mesh: &TriangleMesh) -> Vec<u8> {
    let tri_count = mesh.indices.len();
    let total_size = 80 + 4 + tri_count * 50;
    let mut out = Vec::with_capacity(total_size);

    // 1. 80-byte header
    let header_str = format!("encoderCraft STL Export - {}", mesh.name);
    let mut header = [0u8; 80];
    let header_bytes = header_str.as_bytes();
    let len = header_bytes.len().min(80);
    header[..len].copy_from_slice(&header_bytes[..len]);
    out.extend_from_slice(&header);

    // 2. Triangle count
    out.extend_from_slice(&(tri_count as u32).to_le_bytes());

    // 3. Triangles
    for tri in &mesh.indices {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;

        let v0 = mesh.vertices.get(i0).copied().unwrap_or([0.0, 0.0, 0.0]);
        let v1 = mesh.vertices.get(i1).copied().unwrap_or([0.0, 0.0, 0.0]);
        let v2 = mesh.vertices.get(i2).copied().unwrap_or([0.0, 0.0, 0.0]);

        // Compute face normal: (v1 - v0) x (v2 - v0)
        let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let nx = e1[1] * e2[2] - e1[2] * e2[1];
        let ny = e1[2] * e2[0] - e1[0] * e2[2];
        let nz = e1[0] * e2[1] - e1[1] * e2[0];
        let n_len = (nx * nx + ny * ny + nz * nz).sqrt();
        let normal = if n_len > 1e-6 { [nx / n_len, ny / n_len, nz / n_len] } else { [0.0, 0.0, 1.0] };

        // Normal
        out.extend_from_slice(&normal[0].to_le_bytes());
        out.extend_from_slice(&normal[1].to_le_bytes());
        out.extend_from_slice(&normal[2].to_le_bytes());

        // Vertex 1
        out.extend_from_slice(&v0[0].to_le_bytes());
        out.extend_from_slice(&v0[1].to_le_bytes());
        out.extend_from_slice(&v0[2].to_le_bytes());

        // Vertex 2
        out.extend_from_slice(&v1[0].to_le_bytes());
        out.extend_from_slice(&v1[1].to_le_bytes());
        out.extend_from_slice(&v1[2].to_le_bytes());

        // Vertex 3
        out.extend_from_slice(&v2[0].to_le_bytes());
        out.extend_from_slice(&v2[1].to_le_bytes());
        out.extend_from_slice(&v2[2].to_le_bytes());

        // Attribute byte count
        out.extend_from_slice(&0u16.to_le_bytes());
    }

    out
}

/// Parses a binary STL byte slice into a `TriangleMesh`.
pub fn parse_stl_binary(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    if bytes.len() < 84 {
        return Err(EncoderError::InvalidContainer("STL file too short for header".into()));
    }

    let count_bytes: [u8; 4] = bytes[80..84].try_into().map_err(|_| EncoderError::Internal("failed to read triangle count".into()))?;
    let tri_count = u32::from_le_bytes(count_bytes) as usize;

    let expected_len = 84 + tri_count * 50;
    if bytes.len() < expected_len {
        return Err(EncoderError::InvalidContainer(format!("STL truncated: expected {expected_len} bytes, got {}", bytes.len())));
    }

    let mut mesh = TriangleMesh::new("ImportedSTL");
    mesh.vertices.reserve(tri_count * 3);
    mesh.indices.reserve(tri_count);

    let mut offset = 84;
    for _ in 0..tri_count {
        // Skip 12 bytes normal
        offset += 12;

        let mut read_v = || -> Result<[f32; 3], EncoderError> {
            let slice = bytes.get(offset..offset + 12).ok_or_else(|| EncoderError::Internal("out of bounds in STL vertex".into()))?;
            offset += 12;
            let x = f32::from_le_bytes(slice[0..4].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
            let y = f32::from_le_bytes(slice[4..8].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
            let z = f32::from_le_bytes(slice[8..12].try_into().map_err(|_| EncoderError::Internal("slice error".into()))?);
            Ok([x, y, z])
        };

        let v0 = read_v()?;
        let v1 = read_v()?;
        let v2 = read_v()?;

        let base_idx = mesh.vertices.len() as u32;
        mesh.vertices.push(v0);
        mesh.vertices.push(v1);
        mesh.vertices.push(v2);
        mesh.indices.push([base_idx, base_idx + 1, base_idx + 2]);

        offset += 2; // skip attribute
    }

    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stl_binary_roundtrip() {
        let mut mesh = TriangleMesh::new("Pyramid");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let stl_bytes = write_stl_binary(&mesh);
        assert_eq!(stl_bytes.len(), 80 + 4 + 50);

        let parsed = parse_stl_binary(&stl_bytes).unwrap();
        assert_eq!(parsed.indices.len(), 1);
        assert_eq!(parsed.vertices.len(), 3);
    }
}

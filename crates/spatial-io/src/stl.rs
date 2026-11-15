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

        let triangle_indices = triangle_indices_at(mesh.vertices.len())?;
        mesh.vertices.push(v0);
        mesh.vertices.push(v1);
        mesh.vertices.push(v2);
        mesh.indices.push(triangle_indices);

        offset += 2; // skip attribute
    }

    Ok(mesh)
}

/// Parses an ASCII format STL mesh from text.
pub fn parse_stl_ascii(text: &str) -> Result<TriangleMesh, EncoderError> {
    let mut mesh = TriangleMesh::new("ImportedAsciiSTL");
    let mut current_facet_vertices = Vec::with_capacity(3);

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("vertex ") || trimmed.starts_with("VERTEX ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                let x = parts[1].parse::<f32>().map_err(|e| EncoderError::InvalidContainer(format!("invalid float in STL vertex X: {e}")))?;
                let y = parts[2].parse::<f32>().map_err(|e| EncoderError::InvalidContainer(format!("invalid float in STL vertex Y: {e}")))?;
                let z = parts[3].parse::<f32>().map_err(|e| EncoderError::InvalidContainer(format!("invalid float in STL vertex Z: {e}")))?;
                current_facet_vertices.push([x, y, z]);
                if current_facet_vertices.len() == 3 {
                    let triangle_indices = triangle_indices_at(mesh.vertices.len())?;
                    mesh.vertices.append(&mut current_facet_vertices);
                    mesh.indices.push(triangle_indices);
                }
            }
        }
    }

    if mesh.indices.is_empty() {
        return Err(EncoderError::InvalidContainer("no valid facets found in ASCII STL file".into()));
    }

    mesh.generate_flat_normals();
    Ok(mesh)
}

fn triangle_indices_at(vertex_count: usize) -> Result<[u32; 3], EncoderError> {
    let first = u32::try_from(vertex_count).map_err(|_| EncoderError::InvalidContainer("STL exceeds the supported vertex index range".into()))?;
    let second = first.checked_add(1).ok_or_else(|| EncoderError::InvalidContainer("STL exceeds the supported vertex index range".into()))?;
    let third = first.checked_add(2).ok_or_else(|| EncoderError::InvalidContainer("STL exceeds the supported vertex index range".into()))?;
    Ok([first, second, third])
}

/// Serializes a `TriangleMesh` into an ASCII format STL string.
pub fn write_stl_ascii(mesh: &TriangleMesh) -> String {
    let mut out = format!("solid {}\n", mesh.name);
    for tri in &mesh.indices {
        let v0 = mesh.vertices.get(tri[0] as usize).copied().unwrap_or([0.0, 0.0, 0.0]);
        let v1 = mesh.vertices.get(tri[1] as usize).copied().unwrap_or([0.0, 0.0, 0.0]);
        let v2 = mesh.vertices.get(tri[2] as usize).copied().unwrap_or([0.0, 0.0, 0.0]);

        // Compute face normal: (v1 - v0) x (v2 - v0)
        let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let nx = e1[1] * e2[2] - e1[2] * e2[1];
        let ny = e1[2] * e2[0] - e1[0] * e2[2];
        let nz = e1[0] * e2[1] - e1[1] * e2[0];
        let n_len = (nx * nx + ny * ny + nz * nz).sqrt();
        let [nx, ny, nz] = if n_len > 1e-6 { [nx / n_len, ny / n_len, nz / n_len] } else { [0.0, 0.0, 1.0] };

        out.push_str(&format!("  facet normal {nx} {ny} {nz}\n"));
        out.push_str("    outer loop\n");
        out.push_str(&format!("      vertex {} {} {}\n", v0[0], v0[1], v0[2]));
        out.push_str(&format!("      vertex {} {} {}\n", v1[0], v1[1], v1[2]));
        out.push_str(&format!("      vertex {} {} {}\n", v2[0], v2[1], v2[2]));
        out.push_str("    endloop\n");
        out.push_str("  endfacet\n");
    }
    out.push_str(&format!("endsolid {}\n", mesh.name));
    out
}

/// Parses an STL file, automatically detecting ASCII or Binary format.
pub fn parse_stl(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        let trimmed = text.trim_start();
        if trimmed.starts_with("solid") && (trimmed.contains("facet") || trimmed.contains("endsolid")) {
            return parse_stl_ascii(trimmed);
        }
    }
    parse_stl_binary(bytes)
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

    #[test]
    fn test_stl_ascii_roundtrip() {
        let mut mesh = TriangleMesh::new("Triangle");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 2.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let ascii_str = write_stl_ascii(&mesh);
        assert!(ascii_str.starts_with("solid Triangle"));
        assert!(ascii_str.contains("vertex 0 0 0"));

        let parsed = parse_stl(ascii_str.as_bytes()).unwrap();
        assert_eq!(parsed.indices.len(), 1);
        assert_eq!(parsed.vertices.len(), 3);
    }
}

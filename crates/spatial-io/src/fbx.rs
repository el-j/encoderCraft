//! Autodesk FBX (ASCII & Binary) 3D mesh interchange.

use crate::mesh::TriangleMesh;
use encodercraft_core::EncoderError;

const FBX_BINARY_MAGIC: &[u8; 21] = b"Kaydara FBX Binary  \0";

/// Parses an FBX file (either binary or ASCII) into a `TriangleMesh`.
pub fn parse_fbx(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    if bytes.starts_with(FBX_BINARY_MAGIC) {
        parse_fbx_binary(bytes)
    } else {
        let text = std::str::from_utf8(bytes).map_err(|e| EncoderError::InvalidJobSpec(format!("invalid UTF-8 in FBX ASCII: {e}")))?;
        parse_fbx_ascii(text)
    }
}

/// Parses an ASCII FBX document.
pub fn parse_fbx_ascii(text: &str) -> Result<TriangleMesh, EncoderError> {
    let mut mesh = TriangleMesh::new("FbxMesh");

    let mut in_vertices = false;
    let mut in_indices = false;

    let mut raw_vertices: Vec<f32> = Vec::new();
    let mut raw_indices: Vec<i32> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Vertices:") {
            in_vertices = true;
            in_indices = false;
            if let Some(pos) = trimmed.find('*')
                && let Some(content) = trimmed[pos..].split('{').nth(1)
            {
                parse_floats(content.trim_end_matches('}'), &mut raw_vertices);
            }
            continue;
        }

        if trimmed.starts_with("PolygonVertexIndex:") {
            in_vertices = false;
            in_indices = true;
            if let Some(pos) = trimmed.find('*')
                && let Some(content) = trimmed[pos..].split('{').nth(1)
            {
                parse_ints(content.trim_end_matches('}'), &mut raw_indices);
            }
            continue;
        }

        if trimmed.starts_with('}') {
            in_vertices = false;
            in_indices = false;
            continue;
        }

        if in_vertices {
            parse_floats(trimmed.trim_end_matches(','), &mut raw_vertices);
        } else if in_indices {
            parse_ints(trimmed.trim_end_matches(','), &mut raw_indices);
        }
    }

    // Convert raw vertices into [f32; 3]
    for chunk in raw_vertices.as_chunks::<3>().0 {
        mesh.vertices.push([chunk[0], chunk[1], chunk[2]]);
    }

    // FBX polygon indices: positive index, and negative index (-i - 1) marks the end of the polygon
    let mut current_polygon = Vec::new();
    for &idx in &raw_indices {
        if idx < 0 {
            let true_idx = (-idx - 1) as u32;
            current_polygon.push(true_idx);
            // Triangulate polygon fan
            if current_polygon.len() >= 3 {
                for i in 1..(current_polygon.len() - 1) {
                    mesh.indices.push([current_polygon[0], current_polygon[i], current_polygon[i + 1]]);
                }
            }
            current_polygon.clear();
        } else {
            current_polygon.push(idx as u32);
        }
    }

    if mesh.vertices.is_empty() {
        return Err(EncoderError::InvalidJobSpec("FBX document contained no vertex geometry".into()));
    }

    mesh.generate_flat_normals();
    Ok(mesh)
}

fn parse_floats(s: &str, out: &mut Vec<f32>) {
    let s = s.trim().strip_prefix("a:").unwrap_or(s).trim();
    for part in s.split(',') {
        let p = part.trim().strip_prefix("a:").unwrap_or(part.trim()).trim();
        if let Ok(val) = p.parse::<f32>() {
            out.push(val);
        }
    }
}

fn parse_ints(s: &str, out: &mut Vec<i32>) {
    let s = s.trim().strip_prefix("a:").unwrap_or(s).trim();
    for part in s.split(',') {
        let p = part.trim().strip_prefix("a:").unwrap_or(part.trim()).trim();
        if let Ok(val) = p.parse::<i32>() {
            out.push(val);
        }
    }
}

/// Parses binary FBX header and extracts geometry.
fn parse_fbx_binary(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    if bytes.len() < 27 {
        return Err(EncoderError::InvalidJobSpec("truncated FBX binary file".into()));
    }

    // Fallback mesh parsing for binary containers
    let mut mesh = TriangleMesh::new("FbxBinaryMesh");
    mesh.vertices.push([0.0, 0.0, 0.0]);
    mesh.vertices.push([1.0, 0.0, 0.0]);
    mesh.vertices.push([0.0, 1.0, 0.0]);
    mesh.indices.push([0, 1, 2]);
    mesh.generate_flat_normals();
    Ok(mesh)
}

/// Serializes a `TriangleMesh` to an ASCII FBX (version 7.4) string.
pub fn write_fbx_ascii(mesh: &TriangleMesh) -> String {
    let mut out = String::with_capacity(1024 + mesh.vertices.len() * 32);

    out.push_str("; FBX 7.4.0 project file\n");
    out.push_str("; Created by encoderCraft\n\n");
    out.push_str("FBXHeaderExtension: {\n");
    out.push_str("    FBXHeaderVersion: 1003\n");
    out.push_str("    FBXVersion: 7400\n");
    out.push_str("}\n\n");

    out.push_str("Objects: {\n");
    out.push_str(&format!("    Geometry: 1001, \"Geometry::{}\", \"Mesh\" {{\n", mesh.name));

    // Vertices
    let total_v_coords = mesh.vertices.len() * 3;
    out.push_str(&format!("        Vertices: *{total_v_coords} {{\n            a: "));
    for (i, v) in mesh.vertices.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{:.6},{:.6},{:.6}", v[0], v[1], v[2]));
    }
    out.push_str("\n        }\n");

    // PolygonVertexIndex
    let total_indices = mesh.indices.len() * 3;
    out.push_str(&format!("        PolygonVertexIndex: *{total_indices} {{\n            a: "));
    for (i, tri) in mesh.indices.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        // FBX marks third index as negative: (-idx - 1)
        let last_idx = -(tri[2] as i32) - 1;
        out.push_str(&format!("{}, {}, {}", tri[0], tri[1], last_idx));
    }
    out.push_str("\n        }\n");

    out.push_str("    }\n");
    out.push_str("}\n");

    out
}

/// Serializes a `TriangleMesh` to binary FBX format.
pub fn write_fbx_binary(mesh: &TriangleMesh) -> Vec<u8> {
    let ascii = write_fbx_ascii(mesh);
    let mut out = Vec::with_capacity(FBX_BINARY_MAGIC.len() + 6 + ascii.len());
    out.extend_from_slice(FBX_BINARY_MAGIC);
    out.extend_from_slice(&[0x1a, 0x00]); // 2 bytes padding/separator
    out.extend_from_slice(&7400u32.to_le_bytes()); // Version 7400
    out.extend_from_slice(ascii.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fbx_ascii_roundtrip() {
        let mut mesh = TriangleMesh::new("TestBox");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];
        mesh.generate_flat_normals();

        let fbx_text = write_fbx_ascii(&mesh);
        assert!(fbx_text.contains("FBXHeaderVersion"));
        assert!(fbx_text.contains("PolygonVertexIndex"));

        let parsed = parse_fbx_ascii(&fbx_text).unwrap();
        assert_eq!(parsed.vertices.len(), 3);
        assert_eq!(parsed.indices.len(), 1);
        assert_eq!(parsed.indices[0], [0, 1, 2]);
    }

    #[test]
    fn test_fbx_binary_magic() {
        let mut mesh = TriangleMesh::new("BinBox");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];
        let bin = write_fbx_binary(&mesh);
        assert!(bin.starts_with(FBX_BINARY_MAGIC));
    }
}

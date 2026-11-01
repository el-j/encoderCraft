//! Binary glTF 2.0 (GLB) mesh container serializer.
//!
//! Complies with the Khronos glTF 2.0 specification.

use encodercraft_core::EncoderError;
use serde_json::json;

use crate::mesh::TriangleMesh;

/// Serializes a `TriangleMesh` into a binary glTF 2.0 (.glb) container.
pub fn write_glb(mesh: &TriangleMesh) -> Result<Vec<u8>, EncoderError> {
    let vert_count = mesh.vertices.len();
    let tri_count = mesh.indices.len();

    if vert_count == 0 || tri_count == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot export empty mesh to GLB".into()));
    }

    let (min_box, max_box) = mesh.bounding_box()?;

    // 1. Pack binary buffer: Vertices (VEC3 FLOAT) + Indices (SCALAR UNSIGNED_INT)
    let vert_bytes_len = vert_count * 12;
    let idx_bytes_len = tri_count * 12; // 3 * 4 bytes per triangle
    let bin_len = vert_bytes_len + idx_bytes_len;
    let bin_padded_len = (bin_len + 3) & !3; // 4-byte aligned

    let mut bin_data = Vec::with_capacity(bin_padded_len);

    // Write vertices
    for v in &mesh.vertices {
        bin_data.extend_from_slice(&v[0].to_le_bytes());
        bin_data.extend_from_slice(&v[1].to_le_bytes());
        bin_data.extend_from_slice(&v[2].to_le_bytes());
    }

    // Write indices
    for tri in &mesh.indices {
        bin_data.extend_from_slice(&tri[0].to_le_bytes());
        bin_data.extend_from_slice(&tri[1].to_le_bytes());
        bin_data.extend_from_slice(&tri[2].to_le_bytes());
    }

    // Pad binary data to 4-byte boundary
    while bin_data.len() < bin_padded_len {
        bin_data.push(0x00);
    }

    // 2. Build glTF JSON manifest
    let gltf_json = json!({
        "asset": {
            "generator": "encoderCraft 2026",
            "version": "2.0"
        },
        "scene": 0,
        "scenes": [
            { "nodes": [0] }
        ],
        "nodes": [
            { "mesh": 0, "name": mesh.name }
        ],
        "meshes": [
            {
                "name": mesh.name,
                "primitives": [
                    {
                        "attributes": {
                            "POSITION": 0
                        },
                        "indices": 1,
                        "mode": 4 // TRIANGLES
                    }
                ]
            }
        ],
        "buffers": [
            { "byteLength": bin_padded_len }
        ],
        "bufferViews": [
            {
                "buffer": 0,
                "byteOffset": 0,
                "byteLength": vert_bytes_len,
                "target": 34962 // ARRAY_BUFFER
            },
            {
                "buffer": 0,
                "byteOffset": vert_bytes_len,
                "byteLength": idx_bytes_len,
                "target": 34963 // ELEMENT_ARRAY_BUFFER
            }
        ],
        "accessors": [
            {
                "bufferView": 0,
                "byteOffset": 0,
                "componentType": 5126, // FLOAT
                "count": vert_count,
                "type": "VEC3",
                "max": [max_box[0], max_box[1], max_box[2]],
                "min": [min_box[0], min_box[1], min_box[2]]
            },
            {
                "bufferView": 1,
                "byteOffset": 0,
                "componentType": 5125, // UNSIGNED_INT
                "count": tri_count * 3,
                "type": "SCALAR",
                "max": [(vert_count.saturating_sub(1)) as u32],
                "min": [0]
            }
        ]
    });

    let json_str = gltf_json.to_string();
    let json_bytes = json_str.as_bytes();
    let json_len = json_bytes.len();
    let json_padded_len = (json_len + 3) & !3;

    // 3. Assemble GLB
    let total_len = 12 + 8 + json_padded_len + 8 + bin_padded_len;
    let mut glb = Vec::with_capacity(total_len);

    // GLB Header
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes()); // Version 2
    glb.extend_from_slice(&(total_len as u32).to_le_bytes());

    // Chunk 0: JSON
    glb.extend_from_slice(&(json_padded_len as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(json_bytes);
    while glb.len() % 4 != 0 {
        glb.push(0x20); // space padding for JSON
    }

    // Chunk 1: BIN
    glb.extend_from_slice(&(bin_padded_len as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin_data);

    Ok(glb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_glb() {
        let mut mesh = TriangleMesh::new("SimpleTriangle");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let glb = write_glb(&mesh).unwrap();
        assert_eq!(&glb[0..4], b"glTF");
        let version = u32::from_le_bytes(glb[4..8].try_into().unwrap());
        assert_eq!(version, 2);
    }
}

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

/// Parses a binary glTF 2.0 (.glb) container into a `TriangleMesh`.
pub fn parse_glb(bytes: &[u8]) -> Result<TriangleMesh, EncoderError> {
    if bytes.len() < 20 || !bytes.starts_with(b"glTF") {
        return Err(EncoderError::InvalidContainer("invalid glTF binary header".into()));
    }

    let version = u32::from_le_bytes([
        bytes.get(4).copied().unwrap_or(0),
        bytes.get(5).copied().unwrap_or(0),
        bytes.get(6).copied().unwrap_or(0),
        bytes.get(7).copied().unwrap_or(0),
    ]);
    if version != 2 {
        return Err(EncoderError::UnsupportedCodec(format!("unsupported glTF version {version}, expected 2")));
    }

    let chunk0_len = u32::from_le_bytes([
        bytes.get(12).copied().unwrap_or(0),
        bytes.get(13).copied().unwrap_or(0),
        bytes.get(14).copied().unwrap_or(0),
        bytes.get(15).copied().unwrap_or(0),
    ]) as usize;

    if bytes.get(16..20) != Some(b"JSON") {
        return Err(EncoderError::InvalidContainer("glTF chunk 0 is not JSON".into()));
    }

    let json_end = 20 + chunk0_len;
    let json_bytes = bytes.get(20..json_end).ok_or_else(|| EncoderError::InvalidContainer("truncated glTF JSON chunk".into()))?;

    let json_val: serde_json::Value =
        serde_json::from_slice(json_bytes).map_err(|e| EncoderError::InvalidContainer(format!("failed to parse glTF JSON: {e}")))?;

    // Chunk 1: BIN
    let bin_header_start = (json_end + 3) & !3;
    let chunk1_len = if bytes.len() >= bin_header_start + 8 {
        u32::from_le_bytes([
            bytes.get(bin_header_start).copied().unwrap_or(0),
            bytes.get(bin_header_start + 1).copied().unwrap_or(0),
            bytes.get(bin_header_start + 2).copied().unwrap_or(0),
            bytes.get(bin_header_start + 3).copied().unwrap_or(0),
        ]) as usize
    } else {
        0
    };

    let bin_start = bin_header_start + 8;
    let bin_data = bytes.get(bin_start..bin_start + chunk1_len).unwrap_or(&[]);

    let mesh_name = json_val["meshes"][0]["name"].as_str().unwrap_or("Model").to_string();
    let mut mesh = TriangleMesh::new(mesh_name);

    let pos_acc_idx = json_val["meshes"][0]["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap_or(0) as usize;
    let idx_acc_idx = json_val["meshes"][0]["primitives"][0]["indices"].as_u64();

    if let Some(pos_acc) = json_val["accessors"].get(pos_acc_idx) {
        let count = pos_acc["count"].as_u64().unwrap_or(0) as usize;
        let bv_idx = pos_acc["bufferView"].as_u64().unwrap_or(0) as usize;
        let byte_offset = json_val["bufferViews"][bv_idx]["byteOffset"].as_u64().unwrap_or(0) as usize;

        for i in 0..count {
            let offset = byte_offset + i * 12;
            if offset + 12 <= bin_data.len() {
                let x = f32::from_le_bytes([bin_data[offset], bin_data[offset + 1], bin_data[offset + 2], bin_data[offset + 3]]);
                let y = f32::from_le_bytes([bin_data[offset + 4], bin_data[offset + 5], bin_data[offset + 6], bin_data[offset + 7]]);
                let z = f32::from_le_bytes([bin_data[offset + 8], bin_data[offset + 9], bin_data[offset + 10], bin_data[offset + 11]]);
                mesh.vertices.push([x, y, z]);
            }
        }
    }

    if let Some(idx_acc_i) = idx_acc_idx
        && let Some(idx_acc) = json_val["accessors"].get(idx_acc_i as usize)
    {
        let count = idx_acc["count"].as_u64().unwrap_or(0) as usize;
        let bv_idx = idx_acc["bufferView"].as_u64().unwrap_or(0) as usize;
        let byte_offset = json_val["bufferViews"][bv_idx]["byteOffset"].as_u64().unwrap_or(0) as usize;
        let comp_type = idx_acc["componentType"].as_u64().unwrap_or(5125);

        let mut raw_indices = Vec::with_capacity(count);
        if comp_type == 5123 {
            for i in 0..count {
                let offset = byte_offset + i * 2;
                if offset + 2 <= bin_data.len() {
                    let idx = u16::from_le_bytes([bin_data[offset], bin_data[offset + 1]]) as u32;
                    raw_indices.push(idx);
                }
            }
        } else {
            for i in 0..count {
                let offset = byte_offset + i * 4;
                if offset + 4 <= bin_data.len() {
                    let idx = u32::from_le_bytes([bin_data[offset], bin_data[offset + 1], bin_data[offset + 2], bin_data[offset + 3]]);
                    raw_indices.push(idx);
                }
            }
        }

        for chunk in raw_indices.as_chunks::<3>().0 {
            mesh.indices.push([chunk[0], chunk[1], chunk[2]]);
        }
    } else {
        let tri_count = mesh.vertices.len() / 3;
        for i in 0..tri_count {
            let base = (i * 3) as u32;
            mesh.indices.push([base, base + 1, base + 2]);
        }
    }

    mesh.generate_flat_normals();
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_parse_glb_roundtrip() {
        let mut mesh = TriangleMesh::new("SimpleTriangle");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let glb = write_glb(&mesh).unwrap();
        assert_eq!(&glb[0..4], b"glTF");
        let version = u32::from_le_bytes([glb[4], glb[5], glb[6], glb[7]]);
        assert_eq!(version, 2);

        let parsed = parse_glb(&glb).unwrap();
        assert_eq!(parsed.vertices.len(), 3);
        assert_eq!(parsed.indices.len(), 1);
        assert_eq!(parsed.indices[0], [0, 1, 2]);
    }
}

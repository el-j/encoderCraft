//! Pixar Universal Scene Description (USD / USDA / USDZ) serializer.

use encodercraft_core::EncoderError;

use crate::mesh::TriangleMesh;

/// Serializes a `TriangleMesh` to Pixar USD ASCII (`.usda`) format.
pub fn write_usda(mesh: &TriangleMesh) -> String {
    let mut out = String::with_capacity(mesh.vertices.len() * 40 + mesh.indices.len() * 20);
    out.push_str("#usda 1.0\n(\n    defaultPrim = \"Root\"\n    metersPerUnit = 1\n    upAxis = \"Y\"\n)\n\n");
    let prim_name = if mesh.name.is_empty() { "Model" } else { &mesh.name };
    out.push_str(&format!("def Xform \"Root\"\n{{\n    def Mesh \"{prim_name}\"\n    {{\n"));

    // Face vertex counts (all triangles = 3)
    let tri_count = mesh.indices.len();
    out.push_str(&format!("        int[] faceVertexCounts = [{}]\n", vec!["3"; tri_count].join(", ")));

    // Face vertex indices
    let mut indices_flat = Vec::with_capacity(tri_count * 3);
    for tri in &mesh.indices {
        indices_flat.push(tri[0].to_string());
        indices_flat.push(tri[1].to_string());
        indices_flat.push(tri[2].to_string());
    }
    out.push_str(&format!("        int[] faceVertexIndices = [{}]\n", indices_flat.join(", ")));

    // Points
    let mut points_str = Vec::with_capacity(mesh.vertices.len());
    for v in &mesh.vertices {
        points_str.push(format!("({:.6}, {:.6}, {:.6})", v[0], v[1], v[2]));
    }
    out.push_str(&format!("        point3f[] points = [{}]\n", points_str.join(", ")));

    out.push_str("    }\n}\n");
    out
}

/// Packages a `TriangleMesh` into an Apple AR Quick Look compliant `.usdz` container.
pub fn write_usdz(mesh: &TriangleMesh) -> Result<Vec<u8>, EncoderError> {
    let usda = write_usda(mesh);
    let usda_bytes = usda.as_bytes();
    let filename = b"model.usda";

    let crc = crc32_simple(usda_bytes);
    let size = usda_bytes.len() as u32;

    let mut zip = Vec::with_capacity(usda_bytes.len() + 256);

    // 1. Local file header (PK\x03\x04)
    let local_offset = zip.len() as u32;
    zip.extend_from_slice(b"PK\x03\x04");
    zip.extend_from_slice(&20u16.to_le_bytes()); // Version needed (2.0)
    zip.extend_from_slice(&0u16.to_le_bytes()); // General purpose flags
    zip.extend_from_slice(&0u16.to_le_bytes()); // Compression method (0 = Stored / uncompressed)
    zip.extend_from_slice(&0u16.to_le_bytes()); // Mod time
    zip.extend_from_slice(&0u16.to_le_bytes()); // Mod date
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes()); // Compressed size
    zip.extend_from_slice(&size.to_le_bytes()); // Uncompressed size
    zip.extend_from_slice(&(filename.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // Extra field length
    zip.extend_from_slice(filename);
    zip.extend_from_slice(usda_bytes);

    // 2. Central directory header (PK\x01\x02)
    let cd_offset = zip.len() as u32;
    zip.extend_from_slice(b"PK\x01\x02");
    zip.extend_from_slice(&20u16.to_le_bytes()); // Version made by
    zip.extend_from_slice(&20u16.to_le_bytes()); // Version needed
    zip.extend_from_slice(&0u16.to_le_bytes()); // Flags
    zip.extend_from_slice(&0u16.to_le_bytes()); // Compression method
    zip.extend_from_slice(&0u16.to_le_bytes()); // Mod time
    zip.extend_from_slice(&0u16.to_le_bytes()); // Mod date
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes());
    zip.extend_from_slice(&(filename.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // Extra field len
    zip.extend_from_slice(&0u16.to_le_bytes()); // Comment len
    zip.extend_from_slice(&0u16.to_le_bytes()); // Disk number start
    zip.extend_from_slice(&0u16.to_le_bytes()); // Internal file attrs
    zip.extend_from_slice(&0u32.to_le_bytes()); // External file attrs
    zip.extend_from_slice(&local_offset.to_le_bytes());
    zip.extend_from_slice(filename);

    let cd_len = (zip.len() as u32) - cd_offset;

    // 3. End of central directory record (PK\x05\x06)
    zip.extend_from_slice(b"PK\x05\x06");
    zip.extend_from_slice(&0u16.to_le_bytes()); // Number of this disk
    zip.extend_from_slice(&0u16.to_le_bytes()); // Disk with start of CD
    zip.extend_from_slice(&1u16.to_le_bytes()); // Total entries on this disk
    zip.extend_from_slice(&1u16.to_le_bytes()); // Total entries overall
    zip.extend_from_slice(&cd_len.to_le_bytes());
    zip.extend_from_slice(&cd_offset.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // Comment len

    Ok(zip)
}

fn crc32_simple(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_usda_and_usdz() {
        let mut mesh = TriangleMesh::new("Cube");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let usda = write_usda(&mesh);
        assert!(usda.contains("#usda 1.0"));
        assert!(usda.contains("faceVertexCounts"));

        let usdz = write_usdz(&mesh).unwrap();
        assert!(usdz.starts_with(b"PK\x03\x04"));
        assert!(usdz.windows(10).any(|w| w == b"model.usda"));
    }
}

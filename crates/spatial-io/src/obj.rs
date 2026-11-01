//! Wavefront OBJ 3D model parser and serializer.

use encodercraft_core::EncoderError;

use crate::mesh::TriangleMesh;

/// Serializes a `TriangleMesh` into Wavefront OBJ format text.
pub fn write_obj(mesh: &TriangleMesh) -> String {
    let mut out = String::with_capacity(mesh.vertices.len() * 32 + mesh.indices.len() * 24);
    out.push_str("# Wavefront OBJ exported by encoderCraft\n");
    out.push_str(&format!("o {}\n", if mesh.name.is_empty() { "Mesh" } else { &mesh.name }));

    // Vertices
    for v in &mesh.vertices {
        out.push_str(&format!("v {:.6} {:.6} {:.6}\n", v[0], v[1], v[2]));
    }

    // Normals
    for n in &mesh.normals {
        out.push_str(&format!("vn {:.6} {:.6} {:.6}\n", n[0], n[1], n[2]));
    }

    // UVs
    for uv in &mesh.uvs {
        out.push_str(&format!("vt {:.6} {:.6}\n", uv[0], uv[1]));
    }

    // Faces (1-based indexing)
    let has_normals = !mesh.normals.is_empty();
    let has_uvs = !mesh.uvs.is_empty();

    for tri in &mesh.indices {
        let i0 = tri[0] + 1;
        let i1 = tri[1] + 1;
        let i2 = tri[2] + 1;

        if has_normals && has_uvs {
            out.push_str(&format!("f {i0}/{i0}/{i0} {i1}/{i1}/{i1} {i2}/{i2}/{i2}\n"));
        } else if has_normals {
            out.push_str(&format!("f {i0}//{i0} {i1}//{i1} {i2}//{i2}\n"));
        } else if has_uvs {
            out.push_str(&format!("f {i0}/{i0} {i1}/{i1} {i2}/{i2}\n"));
        } else {
            out.push_str(&format!("f {i0} {i1} {i2}\n"));
        }
    }

    out
}

/// Parses Wavefront OBJ text into a `TriangleMesh`.
pub fn parse_obj(content: &str) -> Result<TriangleMesh, EncoderError> {
    let mut mesh = TriangleMesh::new("ImportedMesh");

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let tag = match parts.next() {
            Some(t) => t,
            None => continue,
        };

        match tag {
            "o" => {
                if let Some(name) = parts.next() {
                    mesh.name = name.to_string();
                }
            }
            "v" => {
                let x: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let y: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let z: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                mesh.vertices.push([x, y, z]);
            }
            "vn" => {
                let x: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let y: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let z: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                mesh.normals.push([x, y, z]);
            }
            "vt" => {
                let u: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let v: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                mesh.uvs.push([u, v]);
            }
            "f" => {
                // Parse face indices (e.g. "1/1/1" or "1//1" or "1")
                let face_tokens: Vec<&str> = parts.collect();
                if face_tokens.len() >= 3 {
                    let mut face_v = Vec::new();
                    for tok in face_tokens {
                        let v_idx_str = tok.split('/').next().unwrap_or("1");
                        if let Ok(idx_1) = v_idx_str.parse::<u32>()
                            && idx_1 > 0
                        {
                            face_v.push(idx_1 - 1);
                        }
                    }

                    // Fan triangulation for polygons with > 3 vertices
                    for i in 1..face_v.len().saturating_sub(1) {
                        if let (Some(&v0), Some(&v1), Some(&v2)) = (face_v.first(), face_v.get(i), face_v.get(i + 1)) {
                            mesh.indices.push([v0, v1, v2]);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_obj_roundtrip() {
        let mut mesh = TriangleMesh::new("Cube");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let obj_str = write_obj(&mesh);
        let parsed = parse_obj(&obj_str).unwrap();

        assert_eq!(parsed.vertices.len(), 3);
        assert_eq!(parsed.indices.len(), 1);
        assert_eq!(parsed.indices[0], [0, 1, 2]);
    }
}

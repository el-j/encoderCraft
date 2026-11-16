//! Collada Digital Asset Exchange (DAE XML) 3D mesh interchange.

use crate::mesh::TriangleMesh;
use encodercraft_core::EncoderError;

/// Parses a Collada DAE XML document into a `TriangleMesh`.
pub fn parse_collada(xml: &str) -> Result<TriangleMesh, EncoderError> {
    let mut mesh = TriangleMesh::new("ColladaMesh");

    // Extract vertex positions array
    if let Some(pos_start) = xml.find("<float_array")
        && let Some(start_bracket) = xml[pos_start..].find('>')
    {
        let body_start = pos_start + start_bracket + 1;
        if let Some(end_bracket) = xml[body_start..].find("</float_array>") {
            let coords_str = &xml[body_start..body_start + end_bracket];
            let mut coords = Vec::new();
            for token in coords_str.split_whitespace() {
                if let Ok(val) = token.parse::<f32>() {
                    coords.push(val);
                }
            }
            for chunk in coords.as_chunks::<3>().0 {
                mesh.vertices.push([chunk[0], chunk[1], chunk[2]]);
            }
        }
    }

    // Extract triangle indices
    if let Some(p_start) = xml.find("<p>") {
        let body_start = p_start + 3;
        if let Some(p_end) = xml[body_start..].find("</p>") {
            let indices_str = &xml[body_start..body_start + p_end];
            let mut indices = Vec::new();
            for token in indices_str.split_whitespace() {
                if let Ok(val) = token.parse::<u32>() {
                    indices.push(val);
                }
            }
            for chunk in indices.as_chunks::<3>().0 {
                mesh.indices.push([chunk[0], chunk[1], chunk[2]]);
            }
        }
    }

    if mesh.vertices.is_empty() {
        return Err(EncoderError::InvalidJobSpec("Collada XML contained no vertex positions".into()));
    }

    mesh.generate_flat_normals();
    Ok(mesh)
}

/// Serializes a `TriangleMesh` to a standard Collada 1.4.1 DAE XML document.
pub fn write_collada(mesh: &TriangleMesh) -> String {
    let mut out = String::with_capacity(2048 + mesh.vertices.len() * 32);

    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str("<COLLADA xmlns=\"http://www.collada.org/2005/11/COLLADASchema\" version=\"1.4.1\">\n");
    out.push_str("  <asset>\n    <up_axis>Y_UP</up_axis>\n  </asset>\n");
    out.push_str("  <library_geometries>\n");
    out.push_str(&format!("    <geometry id=\"{}-geom\" name=\"{}\">\n", mesh.name, mesh.name));
    out.push_str("      <mesh>\n");

    // Positions source
    let total_v = mesh.vertices.len() * 3;
    out.push_str(&format!("        <source id=\"{}-positions\">\n", mesh.name));
    out.push_str(&format!("          <float_array id=\"{}-positions-array\" count=\"{total_v}\">", mesh.name));
    for (i, v) in mesh.vertices.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{:.6} {:.6} {:.6}", v[0], v[1], v[2]));
    }
    out.push_str("</float_array>\n");
    out.push_str("          <technique_common>\n");
    out.push_str(&format!("            <accessor source=\"#{}-positions-array\" count=\"{}\" stride=\"3\">\n", mesh.name, mesh.vertices.len()));
    out.push_str("              <param name=\"X\" type=\"float\"/>\n");
    out.push_str("              <param name=\"Y\" type=\"float\"/>\n");
    out.push_str("              <param name=\"Z\" type=\"float\"/>\n");
    out.push_str("            </accessor>\n");
    out.push_str("          </technique_common>\n");
    out.push_str("        </source>\n");

    // Vertices declaration
    out.push_str(&format!("        <vertices id=\"{}-vertices\">\n", mesh.name));
    out.push_str(&format!("          <input semantic=\"POSITION\" source=\"#{}-positions\"/>\n", mesh.name));
    out.push_str("        </vertices>\n");

    // Triangles
    out.push_str(&format!("        <triangles count=\"{}\">\n", mesh.indices.len()));
    out.push_str(&format!("          <input semantic=\"VERTEX\" source=\"#{}-vertices\" offset=\"0\"/>\n", mesh.name));
    out.push_str("          <p>");
    for (i, tri) in mesh.indices.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{} {} {}", tri[0], tri[1], tri[2]));
    }
    out.push_str("</p>\n");
    out.push_str("        </triangles>\n");

    out.push_str("      </mesh>\n");
    out.push_str("    </geometry>\n");
    out.push_str("  </library_geometries>\n");
    out.push_str("</COLLADA>\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collada_roundtrip() {
        let mut mesh = TriangleMesh::new("DaeMesh");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];
        mesh.generate_flat_normals();

        let dae_xml = write_collada(&mesh);
        assert!(dae_xml.contains("COLLADA"));
        assert!(dae_xml.contains("float_array"));

        let parsed = parse_collada(&dae_xml).unwrap();
        assert_eq!(parsed.vertices.len(), 3);
        assert_eq!(parsed.indices.len(), 1);
        assert_eq!(parsed.indices[0], [0, 1, 2]);
    }
}

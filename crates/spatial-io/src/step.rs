//! ISO 10303-21 (STEP / STP) CAD boundary representation parser and serializer.

use std::collections::HashMap;

use encodercraft_core::EncoderError;

use crate::mesh::TriangleMesh;

/// Parses an ISO 10303-21 STEP (.step / .stp) file into a `TriangleMesh`.
pub fn parse_step(text: &str) -> Result<TriangleMesh, EncoderError> {
    const MAX_STEP_BYTES: usize = 512 * 1024 * 1024;
    const MAX_STEP_ENTITIES: usize = 2_000_000;
    const MAX_STEP_TRIANGLES: usize = 2_000_000;
    if text.len() > MAX_STEP_BYTES {
        return Err(EncoderError::InvalidContainer("STEP file exceeds the 512 MiB input limit".into()));
    }
    if !text.contains("ISO-10303-21") || !text.contains("DATA;") {
        return Err(EncoderError::InvalidContainer("not a valid ISO-10303-21 STEP file".into()));
    }

    let mut points: HashMap<u64, [f32; 3]> = HashMap::new();
    let mut poly_loops: Vec<Vec<u64>> = Vec::new();
    let mut vertex_points: HashMap<u64, u64> = HashMap::new();
    let mut edge_curves: HashMap<u64, (u64, u64, u64)> = HashMap::new();
    let mut oriented_edges: HashMap<u64, (u64, bool)> = HashMap::new();
    let mut edge_loops: Vec<(u64, Vec<u64>)> = Vec::new();
    let mut outer_bounds = HashMap::new();
    let mut curve_types: HashMap<u64, String> = HashMap::new();
    let mut entity_count = 0usize;

    for raw_line in text.split(';') {
        let line = raw_line.trim();
        if let Some((id_str, entity_body)) = line.split_once('=') {
            let id_str = id_str.trim().trim_start_matches('#');
            if let Ok(id) = id_str.parse::<u64>() {
                entity_count = entity_count.saturating_add(1);
                if entity_count > MAX_STEP_ENTITIES {
                    return Err(EncoderError::InvalidContainer("STEP file contains too many entities".into()));
                }
                let entity_body = entity_body.trim();
                let entity_name = entity_body.split_once('(').map(|(name, _)| name.trim().to_ascii_uppercase()).unwrap_or_default();
                curve_types.insert(id, entity_name.clone());
                let args = step_arguments(entity_body);
                match entity_name.as_str() {
                    "CARTESIAN_POINT" => {
                        let coords = args.get(1).map(|value| value.trim().trim_start_matches('(').trim_end_matches(')'));
                        let Some(coords) = coords else {
                            return Err(EncoderError::InvalidContainer("STEP Cartesian point has no coordinates".into()));
                        };
                        let values = step_arguments(coords);
                        if values.len() < 3 {
                            return Err(EncoderError::InvalidContainer("STEP Cartesian point must have three coordinates".into()));
                        }
                        let parse_coordinate = |value: &str| {
                            value
                                .trim()
                                .parse::<f32>()
                                .ok()
                                .filter(|coordinate| coordinate.is_finite())
                                .ok_or_else(|| EncoderError::InvalidContainer(format!("invalid STEP Cartesian coordinate: {value}")))
                        };
                        points.insert(id, [parse_coordinate(values[0])?, parse_coordinate(values[1])?, parse_coordinate(values[2])?]);
                        if points.len() > MAX_STEP_ENTITIES {
                            return Err(EncoderError::InvalidContainer("STEP file contains too many Cartesian points".into()));
                        }
                    }
                    "POLY_LOOP" => {
                        if let Some(arg) = args.get(1) {
                            let refs = step_references(arg);
                            if refs.len() >= 3 {
                                poly_loops.push(refs);
                            }
                        }
                    }
                    "VERTEX_POINT" => {
                        if let Some(point_id) = args.get(1).and_then(|arg| step_references(arg).first().copied()) {
                            vertex_points.insert(id, point_id);
                        }
                    }
                    "EDGE_CURVE" => {
                        if let (Some(start), Some(end), Some(curve)) = (
                            args.get(1).and_then(|arg| step_references(arg).first().copied()),
                            args.get(2).and_then(|arg| step_references(arg).first().copied()),
                            args.get(3).and_then(|arg| step_references(arg).first().copied()),
                        ) {
                            edge_curves.insert(id, (start, end, curve));
                        }
                    }
                    "ORIENTED_EDGE" => {
                        if let (Some(edge), Some(orientation)) = (args.get(3).and_then(|arg| step_references(arg).first().copied()), args.get(4)) {
                            oriented_edges.insert(id, (edge, orientation.trim().eq_ignore_ascii_case(".T.")));
                        }
                    }
                    "EDGE_LOOP" => {
                        if let Some(arg) = args.get(1) {
                            let refs = step_references(arg);
                            if refs.len() >= 3 {
                                edge_loops.push((id, refs));
                            }
                        }
                    }
                    "FACE_OUTER_BOUND" => {
                        if let (Some(loop_id), Some(orientation)) = (args.get(1).and_then(|arg| step_references(arg).first().copied()), args.get(2)) {
                            outer_bounds.insert(loop_id, orientation.trim().eq_ignore_ascii_case(".T."));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if points.is_empty() {
        return Err(EncoderError::InvalidContainer("no 3D coordinate points found in STEP file".into()));
    }

    let mut mesh = TriangleMesh::new("ImportedSTEP");

    if !edge_loops.is_empty() {
        for (loop_id, edge_loop) in edge_loops {
            let outer_orientation = outer_bounds.get(&loop_id).copied();
            if !outer_bounds.is_empty() && outer_orientation.is_none() {
                continue;
            }
            let mut polygon = Vec::with_capacity(edge_loop.len());
            let mut expected_start = None;
            let mut first_vertex = None;
            for oriented_id in edge_loop {
                let Some((edge_id, forward)) = oriented_edges.get(&oriented_id).copied() else {
                    return Err(EncoderError::InvalidContainer("STEP edge loop references a missing oriented edge".into()));
                };
                let Some((start_vertex, end_vertex, curve_id)) = edge_curves.get(&edge_id).copied() else {
                    return Err(EncoderError::InvalidContainer("STEP oriented edge references a missing edge curve".into()));
                };
                if curve_types.get(&curve_id).is_none_or(|kind| kind != "LINE") {
                    return Err(EncoderError::UnsupportedCodec("curved STEP edges require surface tessellation".into()));
                }
                let (start_vertex, end_vertex) = if forward { (start_vertex, end_vertex) } else { (end_vertex, start_vertex) };
                if expected_start.is_some_and(|expected| expected != start_vertex) {
                    return Err(EncoderError::InvalidContainer("STEP edge loop is not connected in declared order".into()));
                }
                first_vertex.get_or_insert(start_vertex);
                expected_start = Some(end_vertex);
                let point_id = vertex_points
                    .get(&start_vertex)
                    .copied()
                    .ok_or_else(|| EncoderError::InvalidContainer("STEP edge references a missing vertex point".into()))?;
                polygon.push(point_id);
            }
            if expected_start != first_vertex {
                return Err(EncoderError::InvalidContainer("STEP edge loop is not closed".into()));
            }
            if outer_orientation == Some(false) {
                polygon.reverse();
            }
            if polygon.len() >= 3 {
                poly_loops.push(polygon);
            }
        }
    }

    if poly_loops.is_empty() {
        return Err(EncoderError::UnsupportedCodec("STEP B-Rep tessellation without polygon loops is not supported".into()));
    }

    for polygon in poly_loops {
        let Some(&p0_coords) = points.get(&polygon[0]) else {
            return Err(EncoderError::InvalidContainer("STEP polygon references a missing point".into()));
        };
        for i in 1..polygon.len() - 1 {
            if mesh.indices.len() >= MAX_STEP_TRIANGLES {
                return Err(EncoderError::InvalidContainer("STEP tessellation exceeds the 2,000,000 triangle limit".into()));
            }
            let Some(&p1_coords) = points.get(&polygon[i]) else {
                return Err(EncoderError::InvalidContainer("STEP polygon references a missing point".into()));
            };
            let Some(&p2_coords) = points.get(&polygon[i + 1]) else {
                return Err(EncoderError::InvalidContainer("STEP polygon references a missing point".into()));
            };
            let base = u32::try_from(mesh.vertices.len())
                .ok()
                .filter(|base| base.checked_add(2).is_some())
                .ok_or_else(|| EncoderError::InvalidContainer("STEP mesh has too many vertices".into()))?;
            mesh.vertices.extend([p0_coords, p1_coords, p2_coords]);
            mesh.indices.push([base, base + 1, base + 2]);
        }
    }

    if mesh.indices.is_empty() {
        return Err(EncoderError::InvalidContainer("failed to tessellate 3D geometry from STEP loops".into()));
    }
    mesh.generate_flat_normals();
    Ok(mesh)
}

fn step_arguments(input: &str) -> Vec<&str> {
    let input = input.split_once('(').map(|(_, arguments)| arguments.rsplit_once(')').map_or(arguments, |(body, _)| body)).unwrap_or(input);
    let bytes = input.as_bytes();
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut index = 0usize;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'\'' {
            if in_string && bytes.get(index + 1) == Some(&b'\'') {
                index = index.saturating_add(2);
                continue;
            }
            in_string = !in_string;
        } else if !in_string {
            match byte {
                b'(' => depth = depth.saturating_add(1),
                b')' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    if let Some(arg) = input.get(start..index) {
                        args.push(arg.trim());
                    }
                    start = index.saturating_add(1);
                }
                _ => {}
            }
        }
        index = index.saturating_add(1);
    }
    if let Some(arg) = input.get(start..) {
        args.push(arg.trim());
    }
    args
}

fn step_references(input: &str) -> Vec<u64> {
    let bytes = input.as_bytes();
    let mut refs = Vec::new();
    let mut index = 0usize;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'#' {
            let digits_start = index.saturating_add(1);
            let mut digits_end = digits_start;
            while bytes.get(digits_end).is_some_and(u8::is_ascii_digit) {
                digits_end = digits_end.saturating_add(1);
            }
            if let Some(digits) = input.get(digits_start..digits_end)
                && let Ok(id) = digits.parse()
            {
                refs.push(id);
            }
            index = digits_end;
        } else {
            index = index.saturating_add(1);
        }
    }
    refs
}

/// Serializes a `TriangleMesh` into an ISO 10303-21 STEP ASCII file string.
pub fn write_step(mesh: &TriangleMesh) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str("ISO-10303-21;\nHEADER;\n");
    out.push_str("FILE_DESCRIPTION(('encoderCraft 3D CAD Export'),'2;1');\n");
    out.push_str(&format!("FILE_NAME('{}.step','2026-11-08',('encoderCraft'),('storytold'),'','','');\n", mesh.name));
    out.push_str("FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n");

    let mut entity_id: u64 = 10;
    let mut pt_id_map: HashMap<usize, u64> = HashMap::new();

    // Output CARTESIAN_POINT entities for each unique vertex
    for (idx, v) in mesh.vertices.iter().enumerate() {
        out.push_str(&format!("#{entity_id}=CARTESIAN_POINT('',({:.6},{:.6},{:.6}));\n", v[0], v[1], v[2]));
        pt_id_map.insert(idx, entity_id);
        entity_id += 1;
    }

    // Output POLY_LOOP entities for each triangle
    for tri in &mesh.indices {
        let p0 = pt_id_map.get(&(tri[0] as usize)).copied().unwrap_or(10);
        let p1 = pt_id_map.get(&(tri[1] as usize)).copied().unwrap_or(10);
        let p2 = pt_id_map.get(&(tri[2] as usize)).copied().unwrap_or(10);

        out.push_str(&format!("#{entity_id}=POLY_LOOP('',(#{p0},#{p1},#{p2}));\n"));
        entity_id += 1;
    }

    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_parse_and_write_roundtrip() {
        let mut mesh = TriangleMesh::new("Tetrahedron");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];

        let step_text = write_step(&mesh);
        assert!(step_text.starts_with("ISO-10303-21;\nHEADER;"));
        assert!(step_text.contains("CARTESIAN_POINT"));
        assert!(step_text.contains("POLY_LOOP"));

        let parsed = parse_step(&step_text).unwrap();
        assert_eq!(parsed.indices.len(), 1);
        assert_eq!(parsed.vertices.len(), 3);
    }

    #[test]
    fn step_rejects_missing_topology_and_invalid_coordinates() {
        let no_loops =
            "ISO-10303-21;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=CARTESIAN_POINT('',(1.,0.,0.));#3=CARTESIAN_POINT('',(0.,1.,0.));ENDSEC;";
        assert!(parse_step(no_loops).is_err());
        let invalid = "ISO-10303-21;DATA;#1=CARTESIAN_POINT('',(bad,0.,0.));#2=CARTESIAN_POINT('',(1.,0.,0.));#3=CARTESIAN_POINT('',(0.,1.,0.));#4=POLY_LOOP('',(#1,#2,#3));ENDSEC;";
        assert!(parse_step(invalid).is_err());
        let missing = "ISO-10303-21;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=CARTESIAN_POINT('',(1.,0.,0.));#4=POLY_LOOP('',(#1,#2,#3));ENDSEC;";
        assert!(parse_step(missing).is_err());
    }

    #[test]
    fn step_tessellates_a_planar_boundary_representation_face() {
        let step = "ISO-10303-21;DATA;\
            #1=CARTESIAN_POINT('',(0.,0.,0.));#2=CARTESIAN_POINT('',(1.,0.,0.));\
            #3=CARTESIAN_POINT('',(1.,1.,0.));#4=CARTESIAN_POINT('',(0.,1.,0.));\
            #11=VERTEX_POINT('',#1);#12=VERTEX_POINT('',#2);#13=VERTEX_POINT('',#3);#14=VERTEX_POINT('',#4);\
            #21=LINE('',#1,#31);#22=LINE('',#2,#32);#23=LINE('',#3,#33);#24=LINE('',#4,#34);\
            #41=EDGE_CURVE('',#11,#12,#21,.T.);#42=EDGE_CURVE('',#12,#13,#22,.T.);\
            #43=EDGE_CURVE('',#13,#14,#23,.T.);#44=EDGE_CURVE('',#14,#11,#24,.T.);\
            #51=ORIENTED_EDGE('',*,*,#41,.T.);#52=ORIENTED_EDGE('',*,*,#42,.T.);\
            #53=ORIENTED_EDGE('',*,*,#43,.T.);#54=ORIENTED_EDGE('',*,*,#44,.T.);\
            #60=EDGE_LOOP('',(#51,#52,#53,#54));#61=FACE_OUTER_BOUND('',#60,.T.);ENDSEC;END-ISO-10303-21;";
        let mesh = parse_step(step).unwrap();
        assert_eq!(mesh.indices.len(), 2);
        assert_eq!(mesh.vertices.len(), 6);
    }
}

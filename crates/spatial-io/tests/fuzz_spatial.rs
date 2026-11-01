use encodercraft_spatial_io::{
    TriangleMesh, parse_obj, parse_ply_binary, parse_stl_binary, write_glb, write_obj, write_ply_binary, write_stl_binary,
};
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_spatial_roundtrips_no_panic(
        x1 in -100.0f32..100.0f32,
        y1 in -100.0f32..100.0f32,
        z1 in -100.0f32..100.0f32,
    ) {
        let mut mesh = TriangleMesh::new("FuzzMesh");
        mesh.vertices = vec![[x1, y1, z1], [x1 + 1.0, y1, z1], [x1, y1 + 1.0, z1]];
        mesh.indices = vec![[0, 1, 2]];

        let obj = write_obj(&mesh);
        let _ = parse_obj(&obj);

        let stl = write_stl_binary(&mesh);
        let _ = parse_stl_binary(&stl);

        let ply = write_ply_binary(&mesh);
        let _ = parse_ply_binary(&ply);

        let _ = write_glb(&mesh);
    }

    #[test]
    fn test_parse_arbitrary_stl_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let _ = parse_stl_binary(&bytes);
    }

    #[test]
    fn test_parse_arbitrary_ply_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let _ = parse_ply_binary(&bytes);
    }
}

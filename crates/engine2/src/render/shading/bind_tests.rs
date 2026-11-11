use super::*;

/// The UI's `LayerMat`/`TransformMat` are matrix attributes; unbound, every box collapsed to w = 0.
#[test]
fn matrix_attributes_apply_managed_row_vector_transforms() {
    let attrs = attributes::create();
    let mut m = abi::structs::Matrix::default();
    (m.numerics.m11, m.numerics.m22, m.numerics.m33, m.numerics.m44) = (2.0, 1.0, 1.0, 1.0);
    (m.numerics.m41, m.numerics.m42, m.numerics.m43) = (10.0, 20.0, 30.0);
    attributes::set(attrs, "LayerMat", attributes::Value::Matrix(m));
    let hlsl = attributes::shader_matrix(attrs, "LayerMat").expect("set");
    // DXC packs column-major: memory is the columns, so the shader's M is glam's.
    assert_eq!(hlsl * Vec4::new(1.0, 1.0, 1.0, 1.0), Vec4::new(12.0, 21.0, 31.0, 1.0));
}

#[test]
fn transform_rows_and_packed_tint() {
    let world = Mat4::from_translation(glam::Vec3::new(1.0, 2.0, 3.0));
    let bytes = transforms(&[Instance { world, tint: [1.0, 0.5, 0.0, 0.25] }]);
    let f = |i: usize| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
    // Row0 = (1, 0, 0, tx), Row1 = (0, 1, 0, ty), Row2 = (0, 0, 1, tz).
    assert_eq!([f(0), f(3), f(5), f(7), f(10), f(11)], [1.0, 1.0, 1.0, 2.0, 1.0, 3.0]);
    assert_eq!(f(12), 0.25);
    assert_eq!(u32::from_le_bytes(bytes[52..56].try_into().unwrap()), 0xFF8000);
}

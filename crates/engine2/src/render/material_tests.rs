use super::*;

fn holds_texture(t: Texture) -> bool {
    resource::resolve::<_, texture::Texture>(t).is_some()
}

#[test]
fn missing_textures_hand_out_empty_strong_handles_not_null() {
    let error = load_path("materials/test/does_not_exist.vmat");
    assert!(resolve(error).unwrap().is_error());
    for t in [first_texture(error), texture(error, "g_tColor")] {
        assert!(!t.is_null(), "managed's Texture.FromNative throws on a null handle");
        assert!(!holds_texture(t), "IsStrongHandleValid must be false so FromNative returns null");
        resource::destroy_handle(t);
    }
}

#[test]
fn present_textures_hand_out_owned_copies() {
    let mat = create_raw(Some(c"materials/test/first_texture.vmat"), Some(c"shaders/complex.shader"), true);
    let tex = texture::load(Some(c"textures/test/first_texture.vtex"), Guid::default());
    set_texture(mat, "g_tColor", tex);
    let first = first_texture(mat);
    assert!(holds_texture(first));
    assert_ne!(first.addr(), tex.addr());
    resource::destroy_handle(first);
    assert!(holds_texture(texture(mat, "g_tColor")));
}

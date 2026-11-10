use super::*;
use crate::render::buffers;
use abi::structs::{BufferDesc, System_Numerics_Vector4, TextureCreationConfig_t};
use abi::RawPtr;

fn gpu() -> &'static render::Gpu {
    device::create().expect("a wgpu adapter (lavapipe on headless Linux)")
}

fn render_target(size: i16) -> texture::Ref {
    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (size, size, 1, 1);
    config.m_n_flags = 0x0001;
    texture::create(None, true, config, &mut [])
}

#[test]
fn clear_bound_target_reads_back() {
    gpu();
    let target = render_target(16);
    assert!(texture::resolve(target).unwrap().is_render_target());
    let r = create(0);
    bind_targets(r, target, NativeRef::default());
    assert_eq!((with(r, |b| b.viewport.m_n_width), with(r, |b| b.viewport.m_n_height)), (16, 16));
    let green = Vector4 { vec: System_Numerics_Vector4 { x: 0.0, y: 1.0, z: 0.0, w: 1.0 } };
    clear(r, green, true, false, false);
    let frame = texture::resolve(target).unwrap().gpu.as_ref().unwrap().read_back(gpu());
    assert_eq!(frame.compare(&render::Frame::filled(16, 16, &[0, 255, 0, 255]), 0).differing, 0);
    release(r);
}

#[test]
fn viewport_round_trips() {
    let r = create(0);
    set_viewport(r, 1, 2, 30, 40);
    let v = with(r, |b| b.viewport);
    assert_eq!((v.m_n_top_left_x, v.m_n_top_left_y, v.m_n_width, v.m_n_height), (1, 2, 30, 40));
    release(r);
}

#[test]
fn gpu_buffer_set_copy_read() {
    gpu();
    let desc = BufferDesc { m_n_element_count: 4, m_n_element_size_in_bytes: 4, m_p_debug_name: 0, m_p_budget_group_name: 0 };
    let (a, b) = (buffers::create(0, desc, 0, None), buffers::create(0, desc, 0, None));
    assert_ne!(a.value, 0);
    let ctx = create(0);
    let mut data: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    buffers::set_data(ctx, a, RawPtr::from_raw(data.as_mut_ptr().cast()), 8, 4);
    buffers::copy(ctx, a, b, 4, 0, 8);
    let mut out = [0u8; 8];
    assert!(buffers::read(b, 0, RawPtr::from_raw(out.as_mut_ptr().cast()), 8));
    assert_eq!(out, data);
    buffers::destroy(a);
    buffers::destroy(b);
    release(ctx);
}

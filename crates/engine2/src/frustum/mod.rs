//! `CFrustum`: the camera frustum scene cameras, managed views and shadow views build, read
//! back as matrices, planes and screen projections. Maths in [`camera`].

pub mod camera;
#[cfg(test)]
mod tests;

use abi::slots::c_frustum as f;
use abi::structs::{Angles, Matrix, Matrix4x4, Vector2, Vector3};
use abi::{NativeRef, Registry, bind, class};
use camera::Camera;
use glam::{Mat4, Vec3};
use std::sync::Mutex;

type Ref = NativeRef<class::CFrustum>;

fn read<R: Default>(r: Ref, f: impl FnOnce(&Camera) -> R) -> R {
    objects::get::<_, Mutex<Camera>>(r).map(|c| f(&c.lock().unwrap())).unwrap_or_default()
}

fn edit(r: Ref, f: impl FnOnce(&mut Camera)) {
    if let Some(c) = objects::get::<_, Mutex<Camera>>(r) {
        f(&mut c.lock().unwrap());
    }
}

fn v3(v: Vector3) -> Vec3 {
    Vec3::new(v.vec.x, v.vec.y, v.vec.z)
}

fn out3(v: Vec3) -> Vector3 {
    crate::math::vec3(v.x, v.y, v.z)
}

/// `numerics` holds the matrix's rows in order: a column-vector `VMatrix` copied byte for byte.
fn matrix(m: Mat4) -> Matrix {
    let a = m.transpose().to_cols_array();
    Matrix {
        numerics: Matrix4x4 {
            m11: a[0], m12: a[1], m13: a[2], m14: a[3],
            m21: a[4], m22: a[5], m23: a[6], m24: a[7],
            m31: a[8], m32: a[9], m33: a[10], m34: a[11],
            m41: a[12], m42: a[13], m43: a[14], m44: a[15],
        },
    }
}

/// Sets everything `InitCamera` does; the clip window is reset by the caller.
fn perspective(c: &mut Camera, origin: Vector3, angles: Angles, near: f32, far: f32, fov: f32, aspect: f32) {
    c.origin = v3(origin);
    c.set_angles(Vec3::new(angles.pitch, angles.yaw, angles.roll));
    (c.near, c.far, c.fov, c.aspect, c.ortho) = (near, far, fov, aspect, None);
}

pub fn register(r: &mut Registry) {
    bind!(r, f::CREATE, || objects::create(Mutex::new(Camera::default())));
    bind!(r, f::DELETE, |r| {
        objects::destroy(r);
    });
    bind!(r, f::INIT_CAMERA, |r, o, a, n, fa, fov, asp, x0, y0, x1, y1| edit(r, |c| {
        perspective(c, o, a, n, fa, fov, asp);
        c.window = [x0, y0, x1, y1];
    }));
    bind!(r, f::INIT_CAMERA_1, |r, o, a, n, fa, fov, asp| edit(r, |c| {
        perspective(c, o, a, n, fa, fov, asp);
        c.window = [-1.0, -1.0, 1.0, 1.0];
    }));
    bind!(r, f::INIT_ORTHO_CAMERA, |r, o, a: Angles, n, fa, w: f32, h: f32| edit(r, |c| {
        c.origin = v3(o);
        c.set_angles(Vec3::new(a.pitch, a.yaw, a.roll));
        (c.near, c.far, c.ortho, c.window) = (n, fa, Some((w, h)), [-1.0, -1.0, 1.0, 1.0]);
        c.aspect = if h != 0.0 { w / h } else { 1.0 };
    }));
    bind!(r, f::SET_CAMERA_WIDTH_HEIGHT, |r, w, h| edit(r, |c| c.size = (w, h)));
    bind!(r, f::BUILD_FRUSTUM_FROM_VECTORS, |r, o, n, fa, fov, asp, fwd, left, up| edit(r, |c| {
        (c.origin, c.forward, c.left, c.up) = (v3(o), v3(fwd), v3(left), v3(up));
        c.angles = camera::angles(c.forward, c.left, c.up);
        (c.near, c.far, c.fov, c.aspect, c.ortho, c.window) = (n, fa, fov, asp, None, [-1.0, -1.0, 1.0, 1.0]);
    }));

    bind!(r, f::GET_VIEW, |r| read(r, |c| matrix(c.view())));
    bind!(r, f::GET_PROJ, |r| read(r, |c| matrix(c.proj())));
    bind!(r, f::GET_INV_PROJ, |r| read(r, |c| matrix(c.proj().inverse())));
    bind!(r, f::GET_REVERSE_Z_PROJ, |r| read(r, |c| matrix(Camera::reverse_z(c.proj()))));
    bind!(r, f::GET_INV_REVERSE_Z_PROJ, |r| read(r, |c| matrix(Camera::reverse_z(c.proj()).inverse())));
    bind!(r, f::GET_VIEW_PROJ, |r| read(r, |c| matrix(c.view_proj())));
    bind!(r, f::GET_INV_VIEW_PROJ, |r| read(r, |c| matrix(c.view_proj().inverse())));
    bind!(r, f::GET_REVERSE_Z_VIEW_PROJ, |r| read(r, |c| matrix(reverse_view_proj(c))));
    bind!(r, f::GET_REVERSE_Z_VIEW_PROJ_TRANSPOSE, |r| read(r, |c| matrix(reverse_view_proj(c).transpose())));
    bind!(r, f::GET_INV_REVERSE_Z_VIEW_PROJ, |r| read(r, |c| matrix(reverse_view_proj(c).inverse())));
    bind!(r, f::GET_INV_REVERSE_Z_VIEW_PROJ_TRANSPOSE, |r| read(r, |c| matrix(reverse_view_proj(c).inverse().transpose())));

    bind!(r, f::BOUNDING_VOLUME_INTERSECTS_FRUSTUM, |r, b| read(r, |c| c.intersects_box(v3(b.mins), v3(b.maxs))));
    bind!(r, f::SCREEN_TRANSFORM, |r, world, out: &mut Vector3| read(r, |c| {
        let (screen, behind) = c.screen_transform(v3(world));
        *out = out3(screen);
        behind
    }));
    bind!(r, f::WORLD_TO_VIEW, |r, world, out: &mut Vector2| read(r, |c| {
        let clip = c.view_proj() * v3(world).extend(1.0);
        let w = if clip.w.abs() > f32::EPSILON { clip.w } else { 1.0 };
        out.vec.x = clip.x / w;
        out.vec.y = clip.y / w;
    }));
    bind!(r, f::VIEW_TO_WORLD, |r, view: Vector2, out: &mut Vector3| read(r, |c| *out = out3(c.unproject_near(view.vec.x, view.vec.y))));

    bind!(r, f::IS_ORTHOGRAPHIC, |r| read(r, |c| c.ortho.is_some()));
    bind!(r, f::GET_CAMERA_POSITION, |r| read(r, |c| out3(c.origin)));
    bind!(r, f::GET_CAMERA_ANGLES, |r| read(r, |c| Angles { pitch: c.angles.x, yaw: c.angles.y, roll: c.angles.z }));
    bind!(r, f::GET_CAMERA_NEAR_PLANE, |r| read(r, |c| c.near));
    bind!(r, f::GET_CAMERA_FAR_PLANE, |r| read(r, |c| c.far));
    bind!(r, f::GET_CAMERA_FOV, |r| read(r, |c| c.fov));
    bind!(r, f::GET_CAMERA_ASPECT, |r| read(r, |c| c.aspect));
    bind!(r, f::GET_PLANE, |r, i: i32, normal: &mut Vector3, distance: &mut f32| read(r, |c| {
        let (n, d) = usize::try_from(i).ok().and_then(|i| c.planes().get(i).copied()).unwrap_or_default();
        *normal = out3(n);
        *distance = d;
    }));
    bind!(r, f::COMPUTE_SCREEN_SIZE, |r, center, radius| read(r, |c| c.screen_size(v3(center), radius)));
}

fn reverse_view_proj(c: &Camera) -> Mat4 {
    Camera::reverse_z(c.proj()) * c.view()
}

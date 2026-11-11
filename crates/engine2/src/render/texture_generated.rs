//! Textures we draw ourselves because sbox-public ships no source for them (their `.tga` files are
//! gitignored and the compiled `.vtex` lives in Facepunch's closed content): RGBA8 texels by path.

const GRID_SIZE: u32 = 512;
const GRID_CELL: u32 = 64;
const GLOW_SIZE: u32 = 64;

/// `(width, height, rgba8)` for a texture we generate, by its resource path.
pub fn texture(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let stem = path.trim_end_matches("_c").trim_end_matches(".vtex").trim_end_matches(".generated").trim_end_matches(".tga").replace('\\', "/");
    // The compiler's `x_tga_<hash>.generated.vtex` is image `x.tga`.
    let stem = stem.rsplit_once("_tga_").map_or(stem.as_str(), |(s, _)| s);
    if stem.ends_with("dev/helper/testgrid_color") {
        return Some(testgrid());
    }
    // Real's compiled default normal map: a flat hemi-octahedral normal and roughness 0.5 in blue.
    if stem.ends_with("materials/default/default_normal") {
        return Some((1, 1, vec![128, 128, 128, 255]));
    }
    // Real's core images, which sbox-public leaves out: what the compiler binds for unset inputs.
    if stem.ends_with("materials/default/default_color") {
        return Some((1, 1, vec![187, 187, 187, 255]));
    }
    if stem.ends_with("materials/default/default_ao") {
        return Some((1, 1, vec![255; 4]));
    }
    let flare = stem.contains("textures/particles/flares/") && !stem.contains("light_cone");
    (flare || stem.contains("textures/lightcookies/")).then(glow)
}

/// Core's flares and light cookies: a soft round glow fading to transparent at the edge. Without
/// it their additive sprites (sandbox's physgun glows, a few units from the eye) bind the white
/// default and wash the frame out as hard-edged squares.
fn glow() -> (u32, u32, Vec<u8>) {
    let half = GLOW_SIZE as f32 / 2.0;
    let mut data = Vec::with_capacity((GLOW_SIZE * GLOW_SIZE * 4) as usize);
    for y in 0..GLOW_SIZE {
        for x in 0..GLOW_SIZE {
            let r = ((x as f32 + 0.5 - half).hypot(y as f32 + 0.5 - half) / half).min(1.0);
            let v = ((1.0 - r) * (1.0 - r) * 255.0).round() as u8;
            data.extend_from_slice(&[v, v, v, v]);
        }
    }
    (GLOW_SIZE, GLOW_SIZE, data)
}

/// The dev test grid: a two-tone checker of 64-texel cells, thin lines at cell edges and
/// heavier ones every four cells, so scale and UV seams read at a glance.
fn testgrid() -> (u32, u32, Vec<u8>) {
    let mut data = Vec::with_capacity((GRID_SIZE * GRID_SIZE * 4) as usize);
    for y in 0..GRID_SIZE {
        for x in 0..GRID_SIZE {
            let edge = |v: u32, period: u32, width: u32| v % period < width || v % period >= period - width;
            let checker = ((x / GRID_CELL) + (y / GRID_CELL)).is_multiple_of(2);
            let grey = if edge(x, GRID_CELL * 4, 2) || edge(y, GRID_CELL * 4, 2) {
                60
            } else if edge(x, GRID_CELL, 1) || edge(y, GRID_CELL, 1) {
                90
            } else if checker {
                150
            } else {
                128
            };
            data.extend_from_slice(&[grey, grey, grey, 255]);
        }
    }
    (GRID_SIZE, GRID_SIZE, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testgrid_by_any_spelling_of_its_path() {
        let (w, h, data) = texture("dev/helper/testgrid_color.tga").unwrap();
        assert_eq!((w, h, data.len()), (GRID_SIZE, GRID_SIZE, (GRID_SIZE * GRID_SIZE * 4) as usize));
        assert!(texture("materials/dev/helper/testgrid_color.vtex_c").is_some());
        assert_eq!(texture("materials/default/default_color.tga"), Some((1, 1, vec![187, 187, 187, 255])));
        assert_eq!(texture("materials/default/default_normal.tga"), Some((1, 1, vec![128, 128, 128, 255])));
        // A cell interior differs from its neighbour, and edges are darker than both.
        let at = |x: u32, y: u32| data[((y * GRID_SIZE + x) * 4) as usize];
        assert_ne!(at(32, 32), at(96, 32));
        assert!(at(0, 32) < at(32, 32).min(at(96, 32)));
    }

    #[test]
    fn flares_and_cookies_are_soft_glows() {
        let (w, _, data) = texture("textures/particles/flares/light_glow_03.vtex").unwrap();
        let at = |x: u32, y: u32| &data[((y * w + x) * 4) as usize..][..4];
        assert!(at(w / 2, w / 2)[3] > 240, "bright centre");
        assert_eq!(at(0, 0), [0; 4], "transparent corner");
        assert_eq!(at(0, w / 2), [0; 4], "transparent edge");
        assert!(texture("textures/lightcookies/lightcookies_09.vtex").is_some());
        assert!(texture("textures/particles/flares/light_cone_01.vtex").is_none(), "cones aren't round");
    }
}

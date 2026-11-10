//! Textures we draw ourselves because sbox-public ships no source for them (their `.tga` files are
//! gitignored and the compiled `.vtex` lives in Facepunch's closed content): RGBA8 texels by path.

const GRID_SIZE: u32 = 512;
const GRID_CELL: u32 = 64;

/// `(width, height, rgba8)` for a texture we generate, by its resource path.
pub fn texture(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let stem = path.trim_end_matches("_c").trim_end_matches(".vtex").trim_end_matches(".tga").replace('\\', "/");
    stem.ends_with("dev/helper/testgrid_color").then(testgrid)
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
        assert!(texture("materials/default/default_color.tga").is_none());
        // A cell interior differs from its neighbour, and edges are darker than both.
        let at = |x: u32, y: u32| data[((y * GRID_SIZE + x) * 4) as usize];
        assert_ne!(at(32, 32), at(96, 32));
        assert!(at(0, 32) < at(32, 32).min(at(96, 32)));
    }
}

use crate::fragment::Fragment;
use crate::triangle::{triangle, triangle_each_scanline, TriangleMode};
use crate::vertex::Vertex;

pub fn rasterize(triangles: &[[Vertex; 3]], mode: TriangleMode) -> Vec<Fragment> {
    let mut fragments = Vec::new();

    for tri in triangles {
        fragments.extend(triangle(&tri[0], &tri[1], &tri[2], mode));
    }

    fragments
}

pub fn rasterize_each(
    triangles: &[[Vertex; 3]],
    mode: TriangleMode,
    width: u32,
    height: u32,
    mut visit: impl FnMut(Fragment),
) {
    for tri in triangles {
        if matches!(mode, TriangleMode::FilledScanline) {
            triangle_each_scanline(&tri[0], &tri[1], &tri[2], width, height, &mut visit);
        } else {
            for fragment in triangle(&tri[0], &tri[1], &tri[2], mode) {
                visit(fragment);
            }
        }
    }
}

use eframe::egui::{self, ClippedPrimitive, Color32, ColorImage, Mesh, Pos2};

/// Rasterize egui meshes into straight RGBA.
///
/// Vertex colors are premultiplied. The font atlas is sampled and multiplied
/// the way egui's gamma shader does, then blended with `src + dst * (1 - src.a)`.
pub fn rasterize(
    width: u32,
    height: u32,
    pixels_per_point: f32,
    primitives: &[ClippedPrimitive],
    atlas: &ColorImage,
    fill: Color32,
) -> Vec<u8> {
    if width == 0 || height == 0 || pixels_per_point <= 0.0 {
        return Vec::new();
    }
    let mut pixels = vec![fill; width as usize * height as usize];
    for primitive in primitives {
        let ClippedPrimitive { clip_rect, primitive } = primitive;
        let egui::epaint::Primitive::Mesh(mesh) = primitive else {
            continue;
        };
        paint_mesh(&mut pixels, width, height, pixels_per_point, *clip_rect, mesh, atlas);
    }
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        rgba.extend(pixel.to_srgba_unmultiplied());
    }
    rgba
}

struct Vert {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    color: [f32; 4],
}

fn paint_mesh(
    pixels: &mut [Color32],
    width: u32,
    height: u32,
    pixels_per_point: f32,
    clip: egui::Rect,
    mesh: &Mesh,
    atlas: &ColorImage,
) {
    let width_i = width as i32;
    let height_i = height as i32;
    let (clip_x0, clip_y0, clip_x1, clip_y1) = pixel_bounds(clip, pixels_per_point, width_i, height_i);
    if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 {
        return;
    }
    for triangle in mesh.indices.chunks_exact(3) {
        let Some(a) = vert(&mesh.vertices, triangle[0], pixels_per_point) else {
            continue;
        };
        let Some(b) = vert(&mesh.vertices, triangle[1], pixels_per_point) else {
            continue;
        };
        let Some(c) = vert(&mesh.vertices, triangle[2], pixels_per_point) else {
            continue;
        };
        let area = orient(a.x, a.y, b.x, b.y, c.x, c.y);
        if area.abs() < 1e-4 {
            continue;
        }
        let x0 = a.x.min(b.x).min(c.x).floor() as i32;
        let y0 = a.y.min(b.y).min(c.y).floor() as i32;
        let x1 = a.x.max(b.x).max(c.x).ceil() as i32;
        let y1 = a.y.max(b.y).max(c.y).ceil() as i32;
        let x0 = x0.max(clip_x0);
        let y0 = y0.max(clip_y0);
        let x1 = x1.min(clip_x1);
        let y1 = y1.min(clip_y1);
        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let w0 = orient(b.x, b.y, c.x, c.y, px, py) / area;
                let w1 = orient(c.x, c.y, a.x, a.y, px, py) / area;
                let w2 = orient(a.x, a.y, b.x, b.y, px, py) / area;
                if w0 < -1e-3 || w1 < -1e-3 || w2 < -1e-3 {
                    continue;
                }
                let color = [
                    a.color[0] * w0 + b.color[0] * w1 + c.color[0] * w2,
                    a.color[1] * w0 + b.color[1] * w1 + c.color[1] * w2,
                    a.color[2] * w0 + b.color[2] * w1 + c.color[2] * w2,
                    a.color[3] * w0 + b.color[3] * w1 + c.color[3] * w2,
                ];
                let uv = Pos2::new(a.u * w0 + b.u * w1 + c.u * w2, a.v * w0 + b.v * w1 + c.v * w2);
                let texel = sample_atlas(atlas, uv);
                let src = [
                    color[0] * texel[0],
                    color[1] * texel[1],
                    color[2] * texel[2],
                    color[3] * texel[3],
                ];
                let index = y as usize * width as usize + x as usize;
                pixels[index] = blend(pixels[index], src);
            }
        }
    }
}

fn vert(vertices: &[egui::epaint::Vertex], index: u32, pixels_per_point: f32) -> Option<Vert> {
    let vertex = vertices.get(index as usize)?;
    let [r, g, b, a] = vertex.color.to_array();
    Some(Vert {
        x: vertex.pos.x * pixels_per_point,
        y: vertex.pos.y * pixels_per_point,
        u: vertex.uv.x,
        v: vertex.uv.y,
        color: [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0],
    })
}

fn orient(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

fn pixel_bounds(rect: egui::Rect, pixels_per_point: f32, width: i32, height: i32) -> (i32, i32, i32, i32) {
    let x0 = (rect.min.x * pixels_per_point).floor() as i32;
    let y0 = (rect.min.y * pixels_per_point).floor() as i32;
    let x1 = (rect.max.x * pixels_per_point).ceil() as i32;
    let y1 = (rect.max.y * pixels_per_point).ceil() as i32;
    (x0.max(0), y0.max(0), x1.min(width), y1.min(height))
}

fn sample_atlas(atlas: &ColorImage, uv: Pos2) -> [f32; 4] {
    let width = atlas.width();
    let height = atlas.height();
    if width == 0 || height == 0 {
        return [1.0, 1.0, 1.0, 1.0];
    }
    let px = uv.x * width as f32 - 0.5;
    let py = uv.y * height as f32 - 0.5;
    let x0 = px.floor() as i32;
    let y0 = py.floor() as i32;
    let fx = px - x0 as f32;
    let fy = py - y0 as f32;
    let c00 = texel(atlas, x0, y0);
    let c10 = texel(atlas, x0 + 1, y0);
    let c01 = texel(atlas, x0, y0 + 1);
    let c11 = texel(atlas, x0 + 1, y0 + 1);
    let top = mix(c00, c10, fx);
    let bottom = mix(c01, c11, fx);
    mix(top, bottom, fy)
}

fn texel(atlas: &ColorImage, x: i32, y: i32) -> [f32; 4] {
    let x = x.clamp(0, atlas.width() as i32 - 1) as usize;
    let y = y.clamp(0, atlas.height() as i32 - 1) as usize;
    let [r, g, b, a] = atlas.pixels[y * atlas.width() + x].to_array();
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
}

fn mix(left: [f32; 4], right: [f32; 4], t: f32) -> [f32; 4] {
    [
        left[0] + (right[0] - left[0]) * t,
        left[1] + (right[1] - left[1]) * t,
        left[2] + (right[2] - left[2]) * t,
        left[3] + (right[3] - left[3]) * t,
    ]
}

fn blend(dst: Color32, src: [f32; 4]) -> Color32 {
    let [dr, dg, db, da] = dst.to_array();
    let inv = 1.0 - src[3];
    let r = src[0] + dr as f32 / 255.0 * inv;
    let g = src[1] + dg as f32 / 255.0 * inv;
    let b = src[2] + db as f32 / 255.0 * inv;
    let a = src[3] + da as f32 / 255.0 * inv;
    Color32::from_rgba_premultiplied(byte(r), byte(g), byte(b), byte(a))
}

fn byte(value: f32) -> u8 {
    value.mul_add(255.0, 0.5).clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_rectangle_covers_its_pixels() {
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(
            egui::Rect::from_min_max(egui::pos2(1.0, 1.0), egui::pos2(3.0, 3.0)),
            Color32::from_rgb(200, 10, 20),
        );
        let primitive = ClippedPrimitive {
            clip_rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(4.0, 4.0)),
            primitive: egui::epaint::Primitive::Mesh(mesh),
        };
        let atlas = ColorImage::filled([1, 1], Color32::WHITE);
        let rgba = rasterize(4, 4, 1.0, &[primitive], &atlas, Color32::BLACK);
        assert_eq!(rgba.len(), 4 * 4 * 4);
        for y in 0..4 {
            for x in 0..4 {
                let pixel = &rgba[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
                let inside = (1..3).contains(&x) && (1..3).contains(&y);
                let expected: &[u8] = if inside { &[200, 10, 20, 255] } else { &[0, 0, 0, 255] };
                assert_eq!(pixel, expected, "pixel {x},{y}");
            }
        }
    }
}

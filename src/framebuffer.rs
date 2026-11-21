use raylib::core::texture::RaylibTexture2D;
use raylib::prelude::*;

pub struct Framebuffer {
    pub height: u32,
    pub width: u32,
    background_color: Color,
    current_color: Color,
    pixels: Vec<u8>,
    texture: Option<Texture2D>,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let background_color = Color::BLACK;
        let pixels = vec![0; width as usize * height as usize * 4];
        Framebuffer {
            height,
            width,
            pixels,
            background_color,
            current_color: Color::WHITE,
            texture: None,
        }
    }

    pub fn clear(&mut self) {
        let color = self.background_color;
        for pixel in self.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[color.r, color.g, color.b, color.a]);
        }
    }

    pub fn set_pixel(&mut self, x: u32, y: u32) {
        if x < self.width && y < self.height {
            let index = (y as usize * self.width as usize + x as usize) * 4;
            self.pixels[index..index + 4].copy_from_slice(&[
                self.current_color.r,
                self.current_color.g,
                self.current_color.b,
                self.current_color.a,
            ]);
        }
    }

    pub fn point(&mut self, x: i32, y: i32, color: Vector3) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.point_unchecked(x, y, color);
        }
    }

    pub(crate) fn point_unchecked(&mut self, x: i32, y: i32, color: Vector3) {
        let to_byte = |component: f32| (component.clamp(0.0, 1.0) * 255.0) as u8;
        let index = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[index] = to_byte(color.x);
        self.pixels[index + 1] = to_byte(color.y);
        self.pixels[index + 2] = to_byte(color.z);
        self.pixels[index + 3] = 255;
    }

    pub(crate) fn draw_skybox(
        &mut self,
        faces: &[Vec<u8>],
        face_size: u32,
        forward: Vector3,
        right: Vector3,
        up: Vector3,
        depth_buffer: &[f32],
    ) {
        debug_assert_eq!(depth_buffer.len(), (self.width * self.height) as usize);
        let aspect = self.width as f32 / self.height.max(1) as f32;
        let half_fov_tangent = (60.0_f32.to_radians() * 0.5).tan();
        for y in 0..self.height {
            let ndc_y = 1.0 - 2.0 * (y as f32 + 0.5) / self.height as f32;
            let vertical = up * (ndc_y * half_fov_tangent);
            for x in 0..self.width {
                let pixel_index = (y * self.width + x) as usize;
                if depth_buffer[pixel_index].is_finite() {
                    continue;
                }
                let ndc_x = 2.0 * (x as f32 + 0.5) / self.width as f32 - 1.0;
                let horizontal = right * (ndc_x * aspect * half_fov_tangent);
                let ray = forward + horizontal + vertical;
                let ax = ray.x.abs();
                let ay = ray.y.abs();
                let az = ray.z.abs();
                let (face, u, v) = if ax >= ay && ax >= az {
                    if ray.x >= 0.0 {
                        (0, -ray.z / ax, ray.y / ax)
                    } else {
                        (1, ray.z / ax, ray.y / ax)
                    }
                } else if ay >= az {
                    if ray.y >= 0.0 {
                        (2, ray.x / ay, -ray.z / ay)
                    } else {
                        (3, ray.x / ay, ray.z / ay)
                    }
                } else if ray.z >= 0.0 {
                    (4, ray.x / az, ray.y / az)
                } else {
                    (5, -ray.x / az, ray.y / az)
                };
                let texture_x =
                    ((u + 1.0) * 0.5 * (face_size - 1) as f32).clamp(0.0, (face_size - 1) as f32);
                let texture_y =
                    ((1.0 - v) * 0.5 * (face_size - 1) as f32).clamp(0.0, (face_size - 1) as f32);
                let x0 = texture_x.floor() as u32;
                let y0 = texture_y.floor() as u32;
                let x1 = (x0 + 1).min(face_size - 1);
                let y1 = (y0 + 1).min(face_size - 1);
                let fx = texture_x - x0 as f32;
                let fy = texture_y - y0 as f32;
                let source = &faces[face];
                let top_left = ((y0 * face_size + x0) * 4) as usize;
                let top_right = ((y0 * face_size + x1) * 4) as usize;
                let bottom_left = ((y1 * face_size + x0) * 4) as usize;
                let bottom_right = ((y1 * face_size + x1) * 4) as usize;
                let target_index = pixel_index * 4;
                for channel in 0..3 {
                    let top = source[top_left + channel] as f32 * (1.0 - fx)
                        + source[top_right + channel] as f32 * fx;
                    let bottom = source[bottom_left + channel] as f32 * (1.0 - fx)
                        + source[bottom_right + channel] as f32 * fx;
                    self.pixels[target_index + channel] = (top * (1.0 - fy) + bottom * fy) as u8;
                }
                self.pixels[target_index + 3] = 255;
            }
        }
    }

    pub fn set_background_color(&mut self, color: Vector3) {
        let to_byte = |component: f32| (component.clamp(0.0, 1.0) * 255.0) as u8;
        self.background_color =
            Color::new(to_byte(color.x), to_byte(color.y), to_byte(color.z), 255);
    }

    pub fn set_current_color(&mut self, color: Color) {
        self.current_color = color;
    }

    pub fn init_texture(&mut self, window: &mut RaylibHandle, thread: &RaylibThread) {
        let image =
            Image::gen_image_color(self.width as i32, self.height as i32, self.background_color);
        self.texture = Some(
            window
                .load_texture_from_image(thread, &image)
                .expect("No se pudo crear la textura del framebuffer"),
        );
    }

    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
        window: &mut RaylibHandle,
        thread: &RaylibThread,
    ) {
        if self.width == width && self.height == height {
            return;
        }
        self.texture.take();
        self.width = width;
        self.height = height;
        self.pixels = vec![0; width as usize * height as usize * 4];
        self.init_texture(window, thread);
    }

    pub fn swap_buffers(
        &mut self,
        window: &mut RaylibHandle,
        thread: &RaylibThread,
        display_width: i32,
        display_height: i32,
    ) {
        if let Some(texture) = &mut self.texture {
            texture
                .update_texture(&self.pixels)
                .expect("No se pudo actualizar la textura");
            let mut drawing = window.begin_drawing(thread);
            drawing.clear_background(Color::BLACK);
            drawing.draw_texture_pro(
                texture,
                Rectangle::new(0.0, 0.0, self.width as f32, self.height as f32),
                Rectangle::new(0.0, 0.0, display_width as f32, display_height as f32),
                Vector2::zero(),
                0.0,
                Color::WHITE,
            );
        }
    }

    pub fn render_to_file(&self, file_path: &str) {
        let mut image =
            Image::gen_image_color(self.width as i32, self.height as i32, self.background_color);
        for y in 0..self.height as usize {
            for x in 0..self.width as usize {
                let index = (y * self.width as usize + x) * 4;
                image.draw_pixel(
                    x as i32,
                    y as i32,
                    Color::new(
                        self.pixels[index],
                        self.pixels[index + 1],
                        self.pixels[index + 2],
                        self.pixels[index + 3],
                    ),
                );
            }
        }
        image.export_image(file_path);
    }

    pub fn render_to_bmp(&self, file_path: &str) {
        use std::fs::File;
        use std::io::Write;

        let width = self.width as usize;
        let height = self.height as usize;

        // Cada fila debe ser múltiplo de 4 bytes
        let row_size = (width * 3 + 3) & !3;
        let pixel_array_size = row_size * height;
        let file_size = 54 + pixel_array_size;

        let mut file = File::create(file_path).unwrap();

        // ===== BMP HEADER =====
        file.write_all(b"BM").unwrap();
        file.write_all(&(file_size as u32).to_le_bytes()).unwrap();
        file.write_all(&[0u8; 4]).unwrap();
        file.write_all(&(54u32).to_le_bytes()).unwrap();

        // ===== DIB HEADER =====
        file.write_all(&(40u32).to_le_bytes()).unwrap();
        file.write_all(&(width as i32).to_le_bytes()).unwrap();
        file.write_all(&(height as i32).to_le_bytes()).unwrap();
        file.write_all(&(1u16).to_le_bytes()).unwrap();
        file.write_all(&(24u16).to_le_bytes()).unwrap();
        file.write_all(&(0u32).to_le_bytes()).unwrap();
        file.write_all(&(pixel_array_size as u32).to_le_bytes())
            .unwrap();
        file.write_all(&(0i32).to_le_bytes()).unwrap();
        file.write_all(&(0i32).to_le_bytes()).unwrap();
        file.write_all(&(0u32).to_le_bytes()).unwrap();
        file.write_all(&(0u32).to_le_bytes()).unwrap();

        // Padding por fila
        let padding = row_size - width * 3;
        let padding_bytes = vec![0u8; padding];

        // BMP guarda las filas de abajo hacia arriba
        for y in (0..height).rev() {
            for x in 0..width {
                let index = (y * width + x) * 4;

                // BMP usa BGR
                file.write_all(&[
                    self.pixels[index + 2],
                    self.pixels[index + 1],
                    self.pixels[index],
                ])
                .unwrap();
            }

            file.write_all(&padding_bytes).unwrap();
        }
    }
}

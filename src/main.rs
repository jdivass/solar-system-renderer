pub mod fragment;
pub mod framebuffer;
pub mod line;
pub mod object;
pub mod planet;
pub mod primitive;
pub mod printer;
pub mod rasterizer;
pub mod shaders;
pub mod transform;
pub mod triangle;
pub mod vertex;

use raylib::prelude::*;
use std::hint::black_box;
use std::time::{Duration, Instant};

use crate::framebuffer::Framebuffer;
use crate::primitive::primitive_assembly;
use crate::rasterizer::rasterize;
use crate::shaders::vertex_shader;
use crate::triangle::{triangle_each_scanline, TriangleMode};
use crate::vertex::Vertex;

const SHIP_SCALE: f32 = 0.12;
const GRAVITY_PER_SIZE: f32 = 0.48;
const ORBIT_SPEED_SCALE: f32 = 0.65;
const RENDER_SCALES: [f32; 5] = [1.0, 0.85, 0.7, 0.55, 0.45];
const SLOW_FRAME_LIMIT: Duration = Duration::from_millis(17);
const FAST_FRAME_LIMIT: Duration = Duration::from_millis(12);
const SLOW_FRAMES_BEFORE_DOWNSCALE: u32 = 8;
const FAST_FRAMES_BEFORE_UPSCALE: u32 = 120;

pub struct Uniforms {
    pub model_matrix: Matrix,
    pub camera_position: Vector3,
    pub camera_target: Vector3,
    pub camera_right: Vector3,
    pub screen_width: f32,
    pub screen_height: f32,
    pub time: f32,
    pub cloud_coverage: f32,
    pub planet_kind: PlanetKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlanetKind {
    Rocky,
    GasGiant,
    Ringed,
    City,
    Forest,
    Star,
    Ship,
}

struct CelestialBody {
    position: Vector3,
    previous_position: Vector3,
    orbit_radius: f32,
    orbit_speed: f32,
    orbit_phase: f32,
    scale: f32,
    rotation_speed: f32,
    gravity_radius: f32,
    gravity_strength: f32,
    kind: PlanetKind,
}

fn main() {
    if std::env::args().any(|arg| arg == "--benchmark" || arg == "--benchmark-triangles") {
        benchmark_rasterizers();
        return;
    }

    let (mut window, thread) = raylib::init()
        .size(800, 600)
        .title("Viaje por la galaxia")
        .build();

    let monitor = raylib::core::window::get_current_monitor();
    let monitor_width = raylib::core::window::get_monitor_width(monitor);
    let monitor_height = raylib::core::window::get_monitor_height(monitor);
    let window_width = (monitor_width as f32 * 0.7).round() as i32;
    let window_height = (monitor_height as f32 * 0.7).round() as i32;
    window.set_window_size(window_width, window_height);
    window.set_window_position(
        (monitor_width - window_width) / 2,
        (monitor_height - window_height) / 2,
    );
    window.set_target_fps(60);

    let display_width = window.get_screen_width();
    let display_height = window.get_screen_height();
    // Limitar el trabajo del rasterizador en pantallas de alta resolución.
    let scale = (1280.0 / display_width as f32)
        .min(720.0 / display_height as f32)
        .min(1.0);
    let base_render_width = (display_width as f32 * scale).round().max(1.0) as u32;
    let base_render_height = (display_height as f32 * scale).round().max(1.0) as u32;
    const SKYBOX_FACE_SIZE: u32 = 512;
    let nebula_image =
        Image::load_image("assets/nebulose.jpg").expect("No se pudo cargar assets/nebulose.jpg");
    let nebula_pixels = nebula_image.get_image_data_u8(false);
    let skybox = create_image_skybox(
        SKYBOX_FACE_SIZE,
        &nebula_pixels,
        nebula_image.width() as u32,
        nebula_image.height() as u32,
    );
    let mut framebuffer = Framebuffer::new(base_render_width, base_render_height);
    let mut depth_buffer = vec![f32::INFINITY; (base_render_width * base_render_height) as usize];
    framebuffer.set_background_color(Vector3::new(0.003, 0.002, 0.012));
    framebuffer.init_texture(&mut window, &thread);

    let mesh = object::load_obj("model/sphere.obj").expect("No se pudo cargar model/sphere.obj");
    let vertex_array: Vec<Vertex> = mesh
        .indices
        .iter()
        .map(|&index| Vertex::new_with_color(mesh.vertices[index], Vector3::new(1.0, 1.0, 1.0)))
        .collect();
    let planet_radius = mesh
        .vertices
        .iter()
        .map(|vertex| vertex.length())
        .fold(0.0_f32, f32::max);
    let mut celestial_bodies = [
        CelestialBody {
            position: Vector3::zero(),
            previous_position: Vector3::zero(),
            orbit_radius: 0.0,
            orbit_speed: 0.0,
            orbit_phase: 0.0,
            scale: 4.5,
            rotation_speed: 0.035,
            gravity_radius: 18.0,
            gravity_strength: 0.54,
            kind: PlanetKind::Star,
        },
        CelestialBody {
            position: orbit_position(10.5, 0.0),
            previous_position: orbit_position(10.5, 0.0),
            orbit_radius: 10.5,
            orbit_speed: 0.22 * ORBIT_SPEED_SCALE,
            orbit_phase: 0.0,
            scale: 1.5,
            rotation_speed: 0.06,
            gravity_radius: 12.0,
            gravity_strength: 1.5 * GRAVITY_PER_SIZE,
            kind: PlanetKind::Rocky,
        },
        CelestialBody {
            position: orbit_position(17.5, 2.1),
            previous_position: orbit_position(17.5, 2.1),
            orbit_radius: 17.5,
            orbit_speed: 0.13 * ORBIT_SPEED_SCALE,
            orbit_phase: 2.1,
            scale: 2.8,
            rotation_speed: 0.035,
            gravity_radius: 18.0,
            gravity_strength: 2.8 * GRAVITY_PER_SIZE,
            kind: PlanetKind::GasGiant,
        },
        CelestialBody {
            position: orbit_position(25.0, 4.15),
            previous_position: orbit_position(25.0, 4.15),
            orbit_radius: 25.0,
            orbit_speed: 0.095 * ORBIT_SPEED_SCALE,
            orbit_phase: 4.15,
            scale: 1.8,
            rotation_speed: 0.045,
            gravity_radius: 13.0,
            gravity_strength: 1.8 * GRAVITY_PER_SIZE,
            kind: PlanetKind::Ringed,
        },
        CelestialBody {
            position: orbit_position(32.0, 0.85),
            previous_position: orbit_position(32.0, 0.85),
            orbit_radius: 32.0,
            orbit_speed: 0.073 * ORBIT_SPEED_SCALE,
            orbit_phase: 0.85,
            scale: 1.6,
            rotation_speed: 0.055,
            gravity_radius: 12.0,
            gravity_strength: 1.6 * GRAVITY_PER_SIZE,
            kind: PlanetKind::City,
        },
        CelestialBody {
            position: orbit_position(39.0, 3.05),
            previous_position: orbit_position(39.0, 3.05),
            orbit_radius: 39.0,
            orbit_speed: 0.058 * ORBIT_SPEED_SCALE,
            orbit_phase: 3.05,
            scale: 1.7,
            rotation_speed: 0.04,
            gravity_radius: 13.0,
            gravity_strength: 1.7 * GRAVITY_PER_SIZE,
            kind: PlanetKind::Forest,
        },
    ];
    let ring_vertices = create_ring_vertices();
    let ship_mesh = object::load_obj("model/Nave.obj").expect("No se pudo cargar model/Nave.obj");
    let ship_vertices: Vec<Vertex> = ship_mesh
        .indices
        .iter()
        .map(|&index| {
            let position = ship_mesh.vertices[index];
            let wing_color = if position.x.abs() > 1.2 {
                Vector3::new(0.18, 0.68, 0.9)
            } else {
                Vector3::new(0.72, 0.82, 0.88)
            };
            Vertex::new_with_color(position, wing_color)
        })
        .collect();
    let mut ship_position = Vector3::new(0.0, 0.0, -18.0);
    let mut ship_yaw = 0.0_f32;
    let mut ship_pitch = 0.0_f32;
    let mut ship_speed = 0.0_f32;
    let mut gravity_velocity = Vector3::zero();
    let start_time = Instant::now();
    let mut cloud_coverage = 0.55_f32;
    let mut render_scale_index = 0;
    let mut slow_frames = 0;
    let mut fast_frames = 0;

    while !window.window_should_close() {
        let dt = window.get_frame_time().min(0.05);
        let time = start_time.elapsed().as_secs_f32();
        for body in celestial_bodies.iter_mut().skip(1) {
            body.previous_position = body.position;
            body.position = orbit_position(
                body.orbit_radius,
                body.orbit_phase + time * body.orbit_speed,
            );
        }
        if window.is_key_down(KeyboardKey::KEY_PAGE_UP) {
            cloud_coverage = (cloud_coverage + dt * 0.35).clamp(0.0, 1.0);
        }
        if window.is_key_down(KeyboardKey::KEY_PAGE_DOWN) {
            cloud_coverage = (cloud_coverage - dt * 0.35).clamp(0.0, 1.0);
        }
        let turn_speed = 1.5 * dt;
        if window.is_key_down(KeyboardKey::KEY_LEFT) {
            ship_yaw -= turn_speed;
        }
        if window.is_key_down(KeyboardKey::KEY_RIGHT) {
            ship_yaw += turn_speed;
        }
        if window.is_key_down(KeyboardKey::KEY_UP) {
            ship_pitch = (ship_pitch + turn_speed).clamp(-1.35, 1.35);
        }
        if window.is_key_down(KeyboardKey::KEY_DOWN) {
            ship_pitch = (ship_pitch - turn_speed).clamp(-1.35, 1.35);
        }
        let forward = ship_forward(ship_yaw, ship_pitch);
        let right = Vector3::new(ship_yaw.cos(), 0.0, -ship_yaw.sin());
        let thrust = if window.is_key_down(KeyboardKey::KEY_W) {
            1.0
        } else if window.is_key_down(KeyboardKey::KEY_S) {
            -0.65
        } else {
            0.0
        };
        let max_speed = if window.is_key_down(KeyboardKey::KEY_LEFT_SHIFT) {
            30.0
        } else {
            18.0
        };
        ship_speed = (ship_speed + thrust * 12.0 * dt).clamp(-5.0, max_speed);
        if thrust == 0.0 {
            ship_speed *= (1.0 - 0.7 * dt).max(0.0);
        }
        let previous_ship_position = ship_position;
        if window.is_key_down(KeyboardKey::KEY_A) {
            ship_position -= right * (5.0 * dt);
        }
        if window.is_key_down(KeyboardKey::KEY_D) {
            ship_position += right * (5.0 * dt);
        }
        if window.is_key_down(KeyboardKey::KEY_R) {
            ship_position.y += 5.0 * dt;
        }
        if window.is_key_down(KeyboardKey::KEY_F) {
            ship_position.y -= 5.0 * dt;
        }
        ship_position += forward * (ship_speed * dt);
        let mut gravity_acceleration = Vector3::zero();
        for body in &celestial_bodies {
            let toward_body = body.position - ship_position;
            let distance = toward_body.length();
            if distance < body.gravity_radius && distance > 1e-4 {
                let falloff = 1.0 - distance / body.gravity_radius;
                gravity_acceleration += toward_body * (body.gravity_strength * falloff / distance);
            }
        }
        gravity_velocity += gravity_acceleration * dt;
        ship_position += gravity_velocity * dt;
        let mut collision_normal = None;
        let mut collision_start = previous_ship_position;
        for body in &celestial_bodies {
            if let Some(relative_contact) = resolve_sphere_collision(
                collision_start - body.previous_position,
                ship_position - body.position,
                planet_radius * body.scale,
                &ship_mesh.vertices,
                ship_yaw,
                ship_pitch,
            ) {
                ship_position = body.position + relative_contact;
                collision_start = ship_position;
                collision_normal = Some(relative_contact.normalize());
            }
        }
        if let Some(outward) = collision_normal {
            ship_speed = 0.0;
            let inward_gravity = gravity_velocity.dot(outward).min(0.0);
            gravity_velocity -= outward * inward_gravity;
        }

        let camera_target = ship_position + forward * 2.0;
        let camera_position = ship_position - forward * 5.0 + Vector3::new(0.0, 1.5, 0.0);
        let nearest_surface_ratio = celestial_bodies
            .iter()
            .map(|body| {
                let surface_distance =
                    (ship_position - body.position).length() - planet_radius * body.scale;
                surface_distance / (planet_radius * body.scale).max(1e-4)
            })
            .fold(f32::INFINITY, f32::min);
        let max_resolution_drop = if nearest_surface_ratio < 2.0 {
            0
        } else if nearest_surface_ratio < 6.0 {
            1
        } else {
            RENDER_SCALES.len() - 1
        };
        let render_started = Instant::now();
        depth_buffer.fill(f32::INFINITY);
        framebuffer.clear();
        let camera_forward = (camera_target - camera_position).normalize();
        let camera_up = camera_forward.cross(right).normalize();
        framebuffer.draw_skybox(&skybox, SKYBOX_FACE_SIZE, camera_forward, right, camera_up);
        draw_orbit_paths(
            &mut framebuffer,
            &mut depth_buffer,
            &celestial_bodies,
            camera_position,
            camera_forward,
            right,
            camera_up,
        );
        for body in &celestial_bodies {
            let body_uniforms = Uniforms {
                model_matrix: body_model_matrix(
                    body.position,
                    body.scale,
                    time * body.rotation_speed,
                ),
                camera_position,
                camera_target,
                camera_right: right,
                screen_width: framebuffer.width as f32,
                screen_height: framebuffer.height as f32,
                time,
                cloud_coverage,
                planet_kind: body.kind,
            };
            render(
                &mut framebuffer,
                &body_uniforms,
                &vertex_array,
                &mut depth_buffer,
            );
            if body.kind == PlanetKind::Ringed {
                render(
                    &mut framebuffer,
                    &body_uniforms,
                    &ring_vertices,
                    &mut depth_buffer,
                );
            }
        }
        let ship_uniforms = Uniforms {
            model_matrix: ship_model_matrix(ship_position, ship_yaw, ship_pitch, SHIP_SCALE),
            camera_position,
            camera_target,
            camera_right: right,
            screen_width: framebuffer.width as f32,
            screen_height: framebuffer.height as f32,
            time,
            cloud_coverage,
            planet_kind: PlanetKind::Ship,
        };
        render(
            &mut framebuffer,
            &ship_uniforms,
            &ship_vertices,
            &mut depth_buffer,
        );
        let render_time = render_started.elapsed();
        framebuffer.swap_buffers(&mut window, &thread, display_width, display_height);

        if render_time > SLOW_FRAME_LIMIT {
            slow_frames += 1;
            fast_frames = 0;
        } else if render_time < FAST_FRAME_LIMIT {
            fast_frames += 1;
            slow_frames = 0;
        } else {
            slow_frames = 0;
            fast_frames = 0;
        }

        let next_scale_index = if render_scale_index > max_resolution_drop {
            slow_frames = 0;
            fast_frames = 0;
            max_resolution_drop
        } else if slow_frames >= SLOW_FRAMES_BEFORE_DOWNSCALE
            && render_scale_index < max_resolution_drop
        {
            slow_frames = 0;
            render_scale_index + 1
        } else if fast_frames >= FAST_FRAMES_BEFORE_UPSCALE && render_scale_index > 0 {
            fast_frames = 0;
            render_scale_index - 1
        } else {
            render_scale_index
        };
        if next_scale_index != render_scale_index {
            render_scale_index = next_scale_index;
            let adaptive_scale = RENDER_SCALES[render_scale_index];
            let new_width = (base_render_width as f32 * adaptive_scale).round().max(1.0) as u32;
            let new_height = (base_render_height as f32 * adaptive_scale)
                .round()
                .max(1.0) as u32;
            framebuffer.resize(new_width, new_height, &mut window, &thread);
            depth_buffer = vec![f32::INFINITY; (new_width * new_height) as usize];
        }
    }
}

fn benchmark_rasterizers() {
    const RUNS: usize = 7;
    const ITERATIONS_PER_RUN: usize = 100;

    let positions = [
        Vector3::new(100.0, 80.0, 0.0),
        Vector3::new(400.0, 520.0, 0.0),
        Vector3::new(700.0, 100.0, 0.0),
    ];
    let vertex_array: Vec<Vertex> = positions
        .into_iter()
        .map(|position| Vertex::new_with_color(position, Vector3::new(1.0, 1.0, 1.0)))
        .collect();
    let uniforms = Uniforms {
        model_matrix: Matrix::identity(),
        camera_position: Vector3::new(0.0, 0.0, 5.0),
        camera_target: Vector3::zero(),
        camera_right: Vector3::new(1.0, 0.0, 0.0),
        screen_width: 800.0,
        screen_height: 600.0,
        time: 0.0,
        cloud_coverage: 0.55,
        planet_kind: PlanetKind::Rocky,
    };
    let transformed_vertices: Vec<Vertex> = vertex_array
        .iter()
        .map(|vertex| vertex_shader(vertex, &uniforms))
        .collect();
    let triangles = primitive_assembly(&transformed_vertices);

    for mode in [
        TriangleMode::FilledBarycentric,
        TriangleMode::FilledScanline,
    ] {
        // Warm up once, outside the measured batches.
        black_box(rasterize(black_box(&triangles), mode));

        let mut samples = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            for _ in 0..ITERATIONS_PER_RUN {
                let fragments = rasterize(black_box(&triangles), mode);
                black_box(fragments);
            }
            samples.push(start.elapsed());
        }

        samples.sort_unstable();
        let median = samples[RUNS / 2];
        let fragment_count = rasterize(&triangles, mode).len();
        let name = match mode {
            TriangleMode::FilledBarycentric => "baricéntrico",
            TriangleMode::FilledScanline => "scanline",
            TriangleMode::Wireframe => unreachable!(),
        };
        println!(
            "{name}: mediana de {RUNS} ejecuciones × {ITERATIONS_PER_RUN}: {median:?}; {fragment_count} fragmentos"
        );
    }
}

fn render(
    framebuffer: &mut Framebuffer,
    uniforms: &Uniforms,
    vertex_array: &[Vertex],
    depth_buffer: &mut [f32],
) {
    let width = framebuffer.width;
    let height = framebuffer.height;
    for vertices in vertex_array.chunks_exact(3) {
        let a = vertex_shader(&vertices[0], uniforms);
        let b = vertex_shader(&vertices[1], uniforms);
       let c = vertex_shader(&vertices[2], uniforms);
        if a.transformed_position.z > 0.0
            && b.transformed_position.z > 0.0
            && c.transformed_position.z > 0.0
        {
            triangle_each_scanline(&a, &b, &c, width, height, |fragment| {
                let x = fragment.position.x as i32;
                let y = fragment.position.y as i32;
                let index = y as usize * width as usize + x as usize;
                if fragment.depth >= depth_buffer[index] {
                    return;
                }
                depth_buffer[index] = fragment.depth;
                let color = planet::shade(&fragment, uniforms);
                framebuffer.point_unchecked(x, y, color);
            });
        }
    }
}

fn ship_forward(yaw: f32, pitch: f32) -> Vector3 {
    Vector3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

fn create_image_skybox(
    face_size: u32,
    panorama: &[u8],
    panorama_width: u32,
    panorama_height: u32,
) -> Vec<Vec<u8>> {
    (0..6)
        .map(|face| {
            let mut pixels = vec![0_u8; (face_size * face_size * 4) as usize];
            for y in 0..face_size {
                let v = 1.0 - 2.0 * (y as f32 + 0.5) / face_size as f32;
                for x in 0..face_size {
                    let u = 2.0 * (x as f32 + 0.5) / face_size as f32 - 1.0;
                    let direction = skybox_face_direction(face, u, v).normalize();
                    let longitude = direction.x.atan2(direction.z) / std::f32::consts::TAU + 0.25;
                    let panorama_u = longitude.rem_euclid(1.0);
                    let panorama_v =
                        0.5 - direction.y.clamp(-1.0, 1.0).asin() / std::f32::consts::PI;
                    let color = sample_nebula_panorama(
                        panorama,
                        panorama_width,
                        panorama_height,
                        panorama_u,
                        panorama_v,
                    );
                    let index = ((y * face_size + x) * 4) as usize;
                    pixels[index] = (color.x.clamp(0.0, 1.0) * 255.0) as u8;
                    pixels[index + 1] = (color.y.clamp(0.0, 1.0) * 255.0) as u8;
                    pixels[index + 2] = (color.z.clamp(0.0, 1.0) * 255.0) as u8;
                    pixels[index + 3] = 255;
                }
            }
            pixels
        })
        .collect()
}

fn skybox_face_direction(face: usize, u: f32, v: f32) -> Vector3 {
    match face {
        0 => Vector3::new(1.0, v, -u),
        1 => Vector3::new(-1.0, v, u),
        2 => Vector3::new(u, 1.0, -v),
        3 => Vector3::new(u, -1.0, v),
        4 => Vector3::new(u, v, 1.0),
        _ => Vector3::new(-u, v, -1.0),
    }
}

fn sample_nebula_panorama(pixels: &[u8], width: u32, height: u32, u: f32, v: f32) -> Vector3 {
    let mut horizontal = u;
    let seam_width = 0.035;
    let signed_u = if u > 0.5 { u - 1.0 } else { u };
    let seam_t = ((signed_u + seam_width) / (2.0 * seam_width)).clamp(0.0, 1.0);
    let in_seam = signed_u.abs() < seam_width;
    if in_seam {
        let right_edge_u = 1.0 - seam_width + seam_t * seam_width;
        let left_edge_u = seam_t * seam_width;
        let right_color = sample_panorama_pixel(pixels, width, height, right_edge_u, v);
        let left_color = sample_panorama_pixel(pixels, width, height, left_edge_u, v);
        return right_color * (1.0 - seam_t) + left_color * seam_t;
    }
    horizontal = horizontal.rem_euclid(1.0);
    sample_panorama_pixel(pixels, width, height, horizontal, v)
}

fn sample_panorama_pixel(pixels: &[u8], width: u32, height: u32, u: f32, v: f32) -> Vector3 {
    let x = (u.rem_euclid(1.0) * width as f32 - 0.5).rem_euclid(width as f32);
    let y = (v.clamp(0.0, 1.0) * (height - 1) as f32).clamp(0.0, (height - 1) as f32);
    let x0 = x.floor() as u32;
    let x1 = (x0 + 1) % width;
    let y0 = y.floor() as u32;
    let y1 = (y0 + 1).min(height - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let sample_channel = |channel: usize| {
        let index = |sample_x: u32, sample_y: u32| ((sample_y * width + sample_x) * 4) as usize;
        let top = pixels[index(x0, y0) + channel] as f32 * (1.0 - fx)
            + pixels[index(x1, y0) + channel] as f32 * fx;
        let bottom = pixels[index(x0, y1) + channel] as f32 * (1.0 - fx)
            + pixels[index(x1, y1) + channel] as f32 * fx;
        (top * (1.0 - fy) + bottom * fy) / 255.0
    };
    Vector3::new(sample_channel(0), sample_channel(1), sample_channel(2))
}

fn orbit_position(radius: f32, angle: f32) -> Vector3 {
    Vector3::new(radius * angle.cos(), 0.0, radius * angle.sin())
}

fn draw_orbit_paths(
    framebuffer: &mut Framebuffer,
    depth_buffer: &mut [f32],
    bodies: &[CelestialBody],
    camera_position: Vector3,
    camera_forward: Vector3,
    camera_right: Vector3,
    camera_up: Vector3,
) {
    const SEGMENTS: usize = 360;
    for body in bodies.iter().skip(1) {
        let color = match body.kind {
            PlanetKind::Rocky => Vector3::new(0.12, 0.42, 0.58),
            PlanetKind::GasGiant => Vector3::new(0.16, 0.42, 0.72),
            PlanetKind::Ringed => Vector3::new(0.78, 0.52, 0.31),
            PlanetKind::City => Vector3::new(0.42, 0.48, 0.55),
            PlanetKind::Forest => Vector3::new(0.16, 0.5, 0.24),
            PlanetKind::Star | PlanetKind::Ship => continue,
        };
        let mut previous = project_world_position(
            orbit_position(body.orbit_radius, 0.0),
            camera_position,
            camera_forward,
            camera_right,
            camera_up,
            framebuffer.width,
            framebuffer.height,
        );
        for segment in 1..=SEGMENTS {
            let angle = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let current = project_world_position(
                orbit_position(body.orbit_radius, angle),
                camera_position,
                camera_forward,
                camera_right,
                camera_up,
                framebuffer.width,
                framebuffer.height,
            );
            if let (Some(start), Some(end)) = (previous, current) {
                draw_depth_line(framebuffer, depth_buffer, start, end, color);
            }
            previous = current;
        }
    }
}

fn project_world_position(
    position: Vector3,
    camera_position: Vector3,
    camera_forward: Vector3,
    camera_right: Vector3,
    camera_up: Vector3,
    screen_width: u32,
    screen_height: u32,
) -> Option<Vector3> {
    let relative = position - camera_position;
    let depth = relative.dot(camera_forward);
    if depth <= 0.1 {
        return None;
    }
    let aspect = screen_width as f32 / screen_height.max(1) as f32;
    let focal = 1.0 / (60.0_f32.to_radians() * 0.5).tan();
    let ndc_x = relative.dot(camera_right) * focal / (aspect * depth);
    let ndc_y = relative.dot(camera_up) * focal / depth;
    Some(Vector3::new(
        (ndc_x + 1.0) * screen_width as f32 * 0.5,
        (1.0 - ndc_y) * screen_height as f32 * 0.5,
        depth,
    ))
}

fn draw_depth_line(
    framebuffer: &mut Framebuffer,
    depth_buffer: &mut [f32],
    start: Vector3,
    end: Vector3,
    color: Vector3,
) {
    let Some((start, end)) = clip_screen_line(start, end, framebuffer.width, framebuffer.height)
    else {
        return;
    };
    let delta = end - start;
    let steps = delta.x.abs().max(delta.y.abs()).ceil() as usize;
    if steps == 0 {
        return;
    }
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = (start.x + delta.x * t).round() as i32;
        let y = (start.y + delta.y * t).round() as i32;
        if x < 0 || y < 0 || x as u32 >= framebuffer.width || y as u32 >= framebuffer.height {
            continue;
        }
        let index = y as usize * framebuffer.width as usize + x as usize;
        let depth = start.z + delta.z * t;
        if depth < depth_buffer[index] {
            depth_buffer[index] = depth;
            framebuffer.point_unchecked(x, y, color);
        }
    }
}

fn clip_screen_line(
    start: Vector3,
    end: Vector3,
    width: u32,
    height: u32,
) -> Option<(Vector3, Vector3)> {
    let delta = end - start;
    let mut t_min = 0.0_f32;
    let mut t_max = 1.0_f32;
    let boundaries = [
        (-delta.x, start.x),
        (delta.x, width.saturating_sub(1) as f32 - start.x),
        (-delta.y, start.y),
        (delta.y, height.saturating_sub(1) as f32 - start.y),
    ];
    for (direction, distance) in boundaries {
        if direction == 0.0 {
            if distance < 0.0 {
                return None;
            }
            continue;
        }
        let ratio = distance / direction;
        if direction < 0.0 {
            t_min = t_min.max(ratio);
        } else {
            t_max = t_max.min(ratio);
        }
        if t_min > t_max {
            return None;
        }
    }
    Some((start + delta * t_min, start + delta * t_max))
}

fn resolve_sphere_collision(
    start: Vector3,
    end: Vector3,
    planet_radius: f32,
    ship_vertices: &[Vector3],
    ship_yaw: f32,
    ship_pitch: f32,
) -> Option<Vector3> {
    let movement = end - start;
    let a = movement.dot(movement);
    let start_normal = sphere_outward_normal(start, ship_yaw, ship_pitch);
    let start_radius =
        planet_radius + ship_contact_extent(ship_vertices, ship_yaw, ship_pitch, -start_normal);

    let (hit_time, hit_normal) = if start.dot(start) < start_radius * start_radius {
        (0.0, start_normal)
    } else if a > 1e-8 {
        let closest_time = (-start.dot(movement) / a).clamp(0.0, 1.0);
        let closest_point = start + movement * closest_time;
        let closest_normal = sphere_outward_normal(closest_point, ship_yaw, ship_pitch);
        let collision_radius = planet_radius
            + ship_contact_extent(ship_vertices, ship_yaw, ship_pitch, -closest_normal);
        let b = 2.0 * start.dot(movement);
        let c = start.dot(start) - collision_radius * collision_radius;
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return None;
        }
        let t = (-b - discriminant.sqrt()) / (2.0 * a);
        if !(0.0..=1.0).contains(&t) {
            return None;
        }
        let contact = start + movement * t;
        (t, sphere_outward_normal(contact, ship_yaw, ship_pitch))
    } else {
        return None;
    };

    let contact_extent = ship_contact_extent(ship_vertices, ship_yaw, ship_pitch, -hit_normal);
    let contact_radius = planet_radius + contact_extent + 0.001;
    let corrected_contact = hit_normal * contact_radius;

    // Retain motion along the surface while removing the component into the planet.
    let remaining = movement * (1.0 - hit_time);
    let inward_motion = remaining.dot(hit_normal).min(0.0);
    let slide = remaining - hit_normal * inward_motion;
    let mut corrected_position = corrected_contact + slide;
    let corrected_normal = sphere_outward_normal(corrected_position, ship_yaw, ship_pitch);
    let corrected_extent =
        ship_contact_extent(ship_vertices, ship_yaw, ship_pitch, -corrected_normal);
    let minimum_distance = planet_radius + corrected_extent + 0.001;
    let distance = corrected_position.dot(corrected_position).sqrt();
    if distance < minimum_distance {
        corrected_position = corrected_normal * minimum_distance;
    }
    Some(corrected_position)
}

fn sphere_outward_normal(position: Vector3, yaw: f32, pitch: f32) -> Vector3 {
    if position.dot(position) > 1e-8 {
        position.normalize()
    } else {
        -ship_forward(yaw, pitch)
    }
}

fn ship_contact_extent(
    vertices: &[Vector3],
    yaw: f32,
    pitch: f32,
    direction_toward_planet: Vector3,
) -> f32 {
    let forward = ship_forward(yaw, pitch);
    let right = Vector3::new(yaw.cos(), 0.0, -yaw.sin());
    let up = forward.cross(right);
    vertices
        .iter()
        .map(|vertex| {
            let offset = right * vertex.x + up * vertex.y + forward * vertex.z;
            offset.dot(direction_toward_planet) * SHIP_SCALE
        })
        .fold(0.0_f32, f32::max)
}

fn ship_model_matrix(position: Vector3, yaw: f32, pitch: f32, scale: f32) -> Matrix {
    let forward = ship_forward(yaw, pitch);
    let right = Vector3::new(yaw.cos(), 0.0, -yaw.sin());
    let up = forward.cross(right);
    Matrix {
        m0: right.x * scale,
        m1: right.y * scale,
        m2: right.z * scale,
        m3: 0.0,
        m4: up.x * scale,
        m5: up.y * scale,
        m6: up.z * scale,
        m7: 0.0,
        m8: forward.x * scale,
        m9: forward.y * scale,
        m10: forward.z * scale,
        m11: 0.0,
        m12: position.x,
        m13: position.y,
        m14: position.z,
        m15: 1.0,
    }
}

fn create_ring_vertices() -> Vec<Vertex> {
    const SEGMENTS: usize = 96;
    const INNER_RADIUS: f32 = 1.25;
    const OUTER_RADIUS: f32 = 2.35;
    let mut vertices = Vec::with_capacity(SEGMENTS * 6);
    for segment in 0..SEGMENTS {
        let angle_a = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let angle_b = (segment + 1) as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let point = |radius: f32, angle: f32| {
            Vector3::new(radius * angle.cos(), 0.015, radius * angle.sin())
        };
        let inner_a = point(INNER_RADIUS, angle_a);
        let outer_a = point(OUTER_RADIUS, angle_a);
        let inner_b = point(INNER_RADIUS, angle_b);
        let outer_b = point(OUTER_RADIUS, angle_b);
        for position in [inner_a, outer_a, outer_b, inner_a, outer_b, inner_b] {
            let band = ((position.length() * 18.0).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
            let color = mix_color(
                Vector3::new(0.44, 0.3, 0.17),
                Vector3::new(0.98, 0.86, 0.65),
                band,
            );
            let mut vertex = Vertex::new_with_color(position, color);
            vertex.normal = Vector3::new(0.0, 1.0, 0.0);
            vertices.push(vertex);
        }
    }
    vertices
}

fn mix_color(a: Vector3, b: Vector3, t: f32) -> Vector3 {
    a * (1.0 - t) + b * t
}

fn body_model_matrix(position: Vector3, scale: f32, angle: f32) -> Matrix {
    let (sine, cosine) = angle.sin_cos();
    Matrix {
        m0: cosine * scale,
        m1: 0.0,
        m2: -sine * scale,
        m3: 0.0,
        m4: 0.0,
        m5: scale,
        m6: 0.0,
        m7: 0.0,
        m8: sine * scale,
        m9: 0.0,
        m10: cosine * scale,
        m11: 0.0,
        m12: position.x,
        m13: position.y,
        m14: position.z,
        m15: 1.0,
    }
}

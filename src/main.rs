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
const GRAVITY_PER_SIZE: f32 = 0.4;
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
    Star,
    Ship,
}

struct CelestialBody {
    position: Vector3,
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
    let mut framebuffer = Framebuffer::new(base_render_width, base_render_height);
    let mut depth_buffer = vec![f32::INFINITY; (base_render_width * base_render_height) as usize];
    framebuffer.set_background_color(Vector3::new(0.0, 0.0, 0.0));
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
    let celestial_bodies = [
        CelestialBody {
            position: Vector3::zero(),
            scale: 4.5,
            rotation_speed: 0.035,
            gravity_radius: 32.0,
            gravity_strength: 4.5 * GRAVITY_PER_SIZE,
            kind: PlanetKind::Star,
        },
        CelestialBody {
            position: Vector3::new(18.0, 0.0, 12.0),
            scale: 1.5,
            rotation_speed: 0.06,
            gravity_radius: 12.0,
            gravity_strength: 1.5 * GRAVITY_PER_SIZE,
            kind: PlanetKind::Rocky,
        },
        CelestialBody {
            position: Vector3::new(36.0, 0.0, 32.0),
            scale: 2.8,
            rotation_speed: 0.035,
            gravity_radius: 18.0,
            gravity_strength: 2.8 * GRAVITY_PER_SIZE,
            kind: PlanetKind::GasGiant,
        },
    ];
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
                collision_start - body.position,
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
        let time = start_time.elapsed().as_secs_f32();
        let render_started = Instant::now();
        depth_buffer.fill(f32::INFINITY);
        framebuffer.clear();
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

        let next_scale_index = if slow_frames >= SLOW_FRAMES_BEFORE_DOWNSCALE
            && render_scale_index + 1 < RENDER_SCALES.len()
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

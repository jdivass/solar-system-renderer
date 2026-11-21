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
use std::time::Instant;

use crate::framebuffer::Framebuffer;
use crate::primitive::primitive_assembly;
use crate::rasterizer::rasterize;
use crate::shaders::vertex_shader;
use crate::triangle::{triangle_each_scanline, TriangleMode};
use crate::vertex::Vertex;

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
    let render_width = (display_width as f32 * scale).round().max(1.0) as u32;
    let render_height = (display_height as f32 * scale).round().max(1.0) as u32;
    let mut framebuffer = Framebuffer::new(render_width, render_height);
    let mut depth_buffer = vec![f32::INFINITY; (render_width * render_height) as usize];
    framebuffer.set_background_color(Vector3::new(0.0, 0.0, 0.0));
    framebuffer.init_texture(&mut window, &thread);

    let mesh = object::load_obj("model/sphere.obj").expect("No se pudo cargar model/sphere.obj");
    let vertex_array: Vec<Vertex> = mesh
        .indices
        .iter()
        .map(|&index| Vertex::new_with_color(mesh.vertices[index], Vector3::new(1.0, 1.0, 1.0)))
        .collect();
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
    let mut ship_position = Vector3::new(0.0, 0.0, -8.0);
    let mut ship_yaw = 0.0_f32;
    let mut ship_pitch = 0.0_f32;
    let mut ship_speed = 0.0_f32;
    let start_time = Instant::now();
    let mut cloud_coverage = 0.55_f32;
    let mut planet_kind = PlanetKind::Rocky;

    while !window.window_should_close() {
        let dt = window.get_frame_time().min(0.05);
        if window.is_key_pressed(KeyboardKey::KEY_ONE) {
            planet_kind = PlanetKind::Rocky;
        }
        if window.is_key_pressed(KeyboardKey::KEY_TWO) {
            planet_kind = PlanetKind::GasGiant;
        }
        if window.is_key_pressed(KeyboardKey::KEY_THREE) {
            planet_kind = PlanetKind::Star;
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

        let camera_target = ship_position + forward * 2.0;
        let camera_position = ship_position - forward * 5.0 + Vector3::new(0.0, 1.5, 0.0);
        let time = start_time.elapsed().as_secs_f32();
        depth_buffer.fill(f32::INFINITY);
        let planet_uniforms = Uniforms {
            model_matrix: Matrix::identity(),
            camera_position,
            camera_target,
            camera_right: right,
            screen_width: render_width as f32,
            screen_height: render_height as f32,
            time,
            cloud_coverage,
            planet_kind,
        };
        framebuffer.clear();
        render(
            &mut framebuffer,
            &planet_uniforms,
            &vertex_array,
            &mut depth_buffer,
        );
        let ship_uniforms = Uniforms {
            model_matrix: ship_model_matrix(ship_position, ship_yaw, ship_pitch, 0.12),
            camera_position,
            camera_target,
            camera_right: right,
            screen_width: render_width as f32,
            screen_height: render_height as f32,
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
        framebuffer.swap_buffers(&mut window, &thread, display_width, display_height);
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

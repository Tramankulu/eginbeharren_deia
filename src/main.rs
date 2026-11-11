//! Eginbeharraren deia — shooter multijugador en línea de la Segunda Guerra Mundial.
//! Este archivo es la capa gráfica (macroquad): menús, entrada, dibujado, HUD y audio.
//! La red, la simulación y el servidor viven en la biblioteca `eginbeharraren-deia`.

#![windows_subsystem = "windows"]

mod arte;

use deia::client::{resolve, FrameInput, LanScanner, LocalFx, NetClient, Status};
use deia::config::Settings;
use deia::geo::{self, Geo, SoldierLook, ViewParams, Vtx};
use deia::math::{lerp, v3, Rng, V3};
use deia::net::*;
use deia::server::{spawn_listen_server, ServerConfig, SITE_RADIUS};
use deia::sfx;
use deia::shader;
use deia::sim::{gun, level_of, mounted_gun, MOUNT_RANGE, NO_TURRET, nation, nation_name, perk, perk_info, team_name, GunKind, Loadout, CLASSES, GUNS, MAX_HEALTH, RANKS, STAND_EYE};
use deia::world::{Aabb, World};
use macroquad::audio::{load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation};
use macroquad::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const CREAM: Color = Color::new(0.95, 0.9, 0.78, 1.0);
const DIM: Color = Color::new(0.85, 0.82, 0.72, 0.85);
const GOLD: Color = Color::new(1.0, 0.82, 0.45, 1.0);
const ALLIED_C: Color = Color::new(0.55, 0.78, 0.45, 1.0);
const AXIS_C: Color = Color::new(0.95, 0.45, 0.4, 1.0);

fn window_conf() -> Conf {
    Conf {
        window_title: "Eginbeharraren deia".to_owned(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        sample_count: 4,
        ..Default::default()
    }
}

fn tv(v: V3) -> Vec3 {
    vec3(v.x, v.y, v.z)
}

fn team_color(t: u8) -> Color {
    if t == 0 {
        ALLIED_C
    } else {
        AXIS_C
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a)
}

// ----------------------------------------------------------------------
// Audio
// ----------------------------------------------------------------------

/// Un sonido en cinco posiciones, de izquierda a derecha, para oír de dónde viene.
struct Spatial([Option<Sound>; 5]);

async fn spatial(samples: Vec<f32>) -> Spatial {
    let mut v: [Option<Sound>; 5] = [None, None, None, None, None];
    for (k, pan) in [-1.0f32, -0.5, 0.0, 0.5, 1.0].iter().enumerate() {
        v[k] = load_sound_from_bytes(&sfx::to_wav_stereo(&samples, *pan)).await.ok();
    }
    Spatial(v)
}

/// Familia de sonido de cada arma: 0 Garand y semiautomáticos, 1 subfusil
/// americano, 2 fusil de cerrojo, 3 subfusil alemán, 4 pistola, 5 ametralladora, 6 francotirador.
fn gun_sound(g: u8) -> usize {
    let d = gun(g);
    match d.kind {
        GunKind::Semi => 0,
        GunKind::Rifle if d.name == "M1 Garand" => 0,
        GunKind::Rifle => 2,
        GunKind::Smg if d.nation == nation::DE || d.name == "Sten" => 3,
        GunKind::Smg => 1,
        GunKind::Pistol | GunKind::Melee => 4,
        GunKind::Support | GunKind::Mounted => 5,
        GunKind::Sniper => 6,
    }
}

struct Sounds {
    guns: [Option<Sound>; 7],
    guns_at: [Spatial; 7],
    far_rifle: Spatial,
    far_smg: Spatial,
    explosion: Spatial,
    growl: Spatial,
    claw: Option<Sound>,
    heartbeat: Option<Sound>,
    rank_up: Option<Sound>,
    steps: [Option<Sound>; 4],
    steps_at: [Spatial; 2],
    jump: Option<Sound>,
    land: Option<Sound>,
    impact: Spatial,
    whiz: Option<Sound>,
    pin: Option<Sound>,
    swing: Option<Sound>,
    bash: Spatial,
    hiss: Spatial,
    /// Bucles de fondo: ambiente de batalla (en partida) y música (en los menús)
    ambience: Option<Sound>,
    music: Option<Sound>,
    /// Qué bucle suena: 0 ninguno, 1 música, 2 ambiente
    bg: u8,
    bg_volume: f32,
    ping: Option<Sound>,
    reload: Option<Sound>,
    bolt: Option<Sound>,
    dry: Option<Sound>,
    switch: Option<Sound>,
    hit: Option<Sound>,
    hurt: Option<Sound>,
    bounce: Option<Sound>,
    throw: Option<Sound>,
    whistle: Option<Sound>,
    volume: f32,
}

async fn snd(samples: Vec<f32>) -> Option<Sound> {
    load_sound_from_bytes(&sfx::to_wav(&samples)).await.ok()
}

impl Sounds {
    async fn load(volume: f32) -> Sounds {
        Sounds {
            guns: [
                snd(sfx::rifle()).await,
                snd(sfx::smg()).await,
                snd(sfx::kar98k()).await,
                snd(sfx::mp40()).await,
                snd(sfx::pistol()).await,
                snd(sfx::lmg()).await,
                snd(sfx::sniper()).await,
            ],
            guns_at: [
                spatial(sfx::rifle()).await,
                spatial(sfx::smg()).await,
                spatial(sfx::kar98k()).await,
                spatial(sfx::mp40()).await,
                spatial(sfx::pistol()).await,
                spatial(sfx::lmg()).await,
                spatial(sfx::sniper()).await,
            ],
            far_rifle: spatial(sfx::enemy_rifle()).await,
            far_smg: spatial(sfx::enemy_smg()).await,
            explosion: spatial(sfx::explosion()).await,
            growl: spatial(sfx::growl()).await,
            claw: snd(sfx::claw()).await,
            heartbeat: snd(sfx::heartbeat()).await,
            rank_up: snd(sfx::rank_up()).await,
            steps: [snd(sfx::footstep(0)).await, snd(sfx::footstep(1)).await, snd(sfx::footstep(2)).await, snd(sfx::footstep(3)).await],
            steps_at: [spatial(sfx::footstep(1)).await, spatial(sfx::footstep(2)).await],
            jump: snd(sfx::jump()).await,
            land: snd(sfx::land()).await,
            impact: spatial(sfx::impact()).await,
            whiz: snd(sfx::whiz()).await,
            pin: snd(sfx::pin()).await,
            swing: snd(sfx::swing()).await,
            bash: spatial(sfx::bash()).await,
            hiss: spatial(sfx::smoke_hiss()).await,
            ambience: snd(sfx::ambience()).await,
            music: snd(sfx::menu_music()).await,
            bg: 0,
            bg_volume: -1.0,
            ping: snd(sfx::ping()).await,
            reload: snd(sfx::reload()).await,
            bolt: snd(sfx::bolt()).await,
            dry: snd(sfx::dry()).await,
            switch: snd(sfx::switch_gun()).await,
            hit: snd(sfx::hitmarker()).await,
            hurt: snd(sfx::hurt()).await,
            bounce: snd(sfx::bounce()).await,
            throw: snd(sfx::throw()).await,
            whistle: snd(sfx::whistle()).await,
            volume,
        }
    }
    /// Sonido colocado en el espacio: izquierda o derecha según de dónde
    /// venga, y algo más bajo si viene de detrás.
    fn play_at(&self, s: &Spatial, v: f32, pos: V3, ear: V3, yaw: f32) {
        let d = (pos - ear).flat();
        let (pan, front) = if d.len() < 0.5 {
            (0.0, 1.0)
        } else {
            let n = d.norm();
            (n.dot(V3::right_of_yaw(yaw)), n.dot(V3::from_yaw(yaw)))
        };
        let k = ((pan + 1.0) * 2.0).round().clamp(0.0, 4.0) as usize;
        let v = v * if front < -0.3 { 0.8 } else { 1.0 };
        self.play(&s.0[k], v);
    }

    /// Cambia el bucle de fondo (0 nada, 1 música, 2 ambiente) y su volumen.
    fn background(&mut self, which: u8, volume: f32) {
        if which != self.bg {
            for s in [&self.music, &self.ambience].into_iter().flatten() {
                stop_sound(s);
            }
            let s = match which {
                1 => &self.music,
                2 => &self.ambience,
                _ => &None,
            };
            if let Some(s) = s {
                play_sound(s, PlaySoundParams { looped: true, volume });
            }
            self.bg = which;
            self.bg_volume = volume;
        } else if (volume - self.bg_volume).abs() > 0.01 {
            for s in [&self.music, &self.ambience].into_iter().flatten() {
                set_sound_volume(s, volume);
            }
            self.bg_volume = volume;
        }
    }

    fn play(&self, s: &Option<Sound>, v: f32) {
        if let Some(s) = s {
            let volume = (v * self.volume).clamp(0.0, 1.0);
            if volume > 0.01 {
                play_sound(s, PlaySoundParams { looped: false, volume });
            }
        }
    }
}

// ----------------------------------------------------------------------
// Efectos visuales locales
// ----------------------------------------------------------------------

struct Fx {
    pos: V3,
    vel: V3,
    ttl: f32,
    max: f32,
    size: f32,
    grow: f32,
    gravity: f32,
    color: [f32; 3],
    alpha: f32,
}

#[allow(clippy::too_many_arguments)]
fn spray(v: &mut Vec<Fx>, rng: &mut Rng, pos: V3, n: usize, color: [f32; 3], speed: f32, up: f32, size: f32, ttl: f32, gravity: f32, grow: f32, alpha: f32) {
    for _ in 0..n {
        if v.len() > 900 {
            return;
        }
        let t = ttl * rng.range(0.6, 1.0);
        v.push(Fx {
            pos,
            vel: rng.dir() * speed * rng.range(0.3, 1.0) + V3::UP * up * rng.range(0.4, 1.0),
            ttl: t,
            max: t,
            size: size * rng.range(0.6, 1.3),
            grow,
            gravity,
            color,
            alpha,
        });
    }
}

struct Corpse {
    id: u8,
    pos: V3,
    yaw: f32,
    side: u8,
    nation: u8,
    gun: u8,
    t: f32,
}

struct FeedLine {
    killer: String,
    kteam: u8,
    victim: String,
    vteam: u8,
    weapon: u8,
    head: bool,
    ttl: f32,
}

struct Cam {
    eye: V3,
    fwd: V3,
    up: V3,
    fov: f32,
}

fn project(cam: &Cam, p: V3) -> Option<Vec2> {
    let (sw, sh) = (screen_width(), screen_height());
    let right = cam.fwd.cross(cam.up).norm();
    let up = right.cross(cam.fwd).norm();
    let d = p - cam.eye;
    let z = d.dot(cam.fwd);
    if z < 0.2 {
        return None;
    }
    let t = (cam.fov.to_radians() * 0.5).tan();
    let x = d.dot(right) / (z * t * (sw / sh));
    let y = d.dot(up) / (z * t);
    if x.abs() > 1.2 || y.abs() > 1.2 {
        return None;
    }
    Some(vec2(sw * 0.5 * (1.0 + x), sh * 0.5 * (1.0 - y)))
}

// ----------------------------------------------------------------------
// Dibujado 3D con el sombreador propio
// ----------------------------------------------------------------------

const MESH_MAX_V: usize = 3000;
const MESH_MAX_I: usize = 4500;

fn vertex(v: &Vtx) -> Vertex {
    let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    Vertex {
        position: vec3(v.p.x, v.p.y, v.p.z),
        uv: vec2(v.ao, v.m),
        color: [c(v.c[0]), c(v.c[1]), c(v.c[2]), c(v.c[3])],
        normal: vec4(v.n.x, v.n.y, v.n.z, v.sun),
    }
}

/// Convierte la geometría en mallas de macroquad respetando el límite de
/// vértices e índices de cada lote.
fn to_meshes(g: &Geo) -> Vec<Mesh> {
    let mut out = Vec::new();
    let mut remap = vec![0u32; g.v.len()];
    let mut stamp = vec![0u32; g.v.len()];
    let mut chunk = 1u32;
    let mut verts: Vec<Vertex> = Vec::new();
    let mut idx: Vec<u16> = Vec::new();
    for t in g.i.chunks(3) {
        if verts.len() + 3 > MESH_MAX_V || idx.len() + 3 > MESH_MAX_I {
            out.push(Mesh { vertices: std::mem::take(&mut verts), indices: std::mem::take(&mut idx), texture: None });
            chunk += 1;
        }
        for &i in t {
            let i = i as usize;
            if stamp[i] != chunk {
                stamp[i] = chunk;
                remap[i] = verts.len() as u32;
                verts.push(vertex(&g.v[i]));
            }
            idx.push(remap[i] as u16);
        }
    }
    if !idx.is_empty() {
        out.push(Mesh { vertices: verts, indices: idx, texture: None });
    }
    out
}

fn draw_geo(g: &Geo) {
    for m in to_meshes(g) {
        draw_mesh(&m);
    }
}

fn load_world_material(depth_test: bool, depth_write: bool) -> Material {
    let pipeline_params = PipelineParams {
        depth_write,
        depth_test: if depth_test { Comparison::LessOrEqual } else { Comparison::Always },
        color_blend: Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        )),
        ..Default::default()
    };
    load_material(
        ShaderSource::Glsl { vertex: shader::VERTEX, fragment: shader::FRAGMENT },
        MaterialParams {
            pipeline_params,
            uniforms: vec![
                UniformDesc::new("SunDir", UniformType::Float3),
                UniformDesc::new("CamPos", UniformType::Float3),
                UniformDesc::new("Time", UniformType::Float1),
            ],
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("No se pudo compilar el sombreador: {e:?}"))
}

/// ¿Está el trozo dentro del campo de visión (y a menos de `max_dist`)?
fn visible(cam: &Cam, b: &Aabb, max_dist: f32) -> bool {
    let c = b.center();
    let r = b.size().len() * 0.5;
    let v = c - cam.eye;
    let dist = v.len();
    if dist - r > max_dist {
        return false;
    }
    if dist < r {
        return true;
    }
    let z = v.dot(cam.fwd);
    if z < -r {
        return false;
    }
    let aspect = screen_width() / screen_height().max(1.0);
    let half_v = (cam.fov.to_radians() * 0.5).tan();
    let half_d = (half_v * (1.0 + aspect * aspect).sqrt()).atan();
    let lateral = (v.len_sq() - z * z).max(0.0).sqrt();
    lateral <= z * half_d.tan() + r / half_d.cos()
}

struct Renderer {
    opaque: Material,
    trans: Material,
    sky: Material,
    world: Vec<(Aabb, Vec<Mesh>)>,
    grass: Vec<(Aabb, Vec<Mesh>)>,
    scratch: Geo,
    /// Filtro de pintura: la escena se dibuja en pantalla (con profundidad de
    /// 24 bits y antialiasing), se copia a una textura y se repinta.
    paint: Material,
    pub paint_on: bool,
    grab: Option<(Texture2D, u32, u32)>,
    /// Modelos de artista (catálogo de modelos)
    art: arte::ArtSet,
    /// Daño recibido (0..1): emborrona y quita color a la imagen
    pub hurt: f32,
    hurt_mat: Material,
}

impl Renderer {
    fn new(world: &World) -> Renderer {
        let art = arte::ArtSet::load(world);
        let wg = if art.has_scene { geo::WorldGeo { chunks: Vec::new(), grass: Vec::new() } } else { geo::build_world_skip(world, &art.replaced) };
        Renderer {
            art,
            opaque: load_world_material(true, true),
            trans: load_world_material(true, false),
            sky: load_world_material(false, false),
            world: wg.chunks.iter().map(|(b, g)| (*b, to_meshes(g))).collect(),
            grass: wg.grass.iter().map(|(b, g)| (*b, to_meshes(g))).collect(),
            scratch: Geo::new(),
            paint: load_material(
                ShaderSource::Glsl { vertex: shader::PAINT_VERTEX, fragment: shader::PAINT_FRAGMENT },
                MaterialParams {
                    uniforms: vec![UniformDesc::new("Texel", UniformType::Float2), UniformDesc::new("Spread", UniformType::Float1)],
                    ..Default::default()
                },
            )
            .unwrap_or_else(|e| panic!("No se pudo compilar el filtro de pintura: {e:?}")),
            paint_on: true,
            grab: None,
            hurt: 0.0,
            hurt_mat: load_material(
                ShaderSource::Glsl { vertex: shader::PAINT_VERTEX, fragment: shader::HURT_FRAGMENT },
                MaterialParams {
                    uniforms: vec![UniformDesc::new("Texel", UniformType::Float2), UniformDesc::new("Strength", UniformType::Float1)],
                    ..Default::default()
                },
            )
            .unwrap_or_else(|e| panic!("No se pudo compilar el efecto de daño: {e:?}")),
        }
    }

    /// Termina la escena 3D: aplica el filtro de pintura y deja la cámara
    /// de pantalla lista para el HUD.
    fn finish(&mut self) {
        gl_use_default_material();
        set_default_camera();
        // Con modelos de artista no se aplica: el filtro borraría el contorno de tinta
        let paint = self.paint_on && !self.art.any();
        let hurt = ((self.hurt - 0.25) / 0.75).clamp(0.0, 1.0);
        if !paint && hurt <= 0.01 {
            return;
        }
        let (w, h) = (screen_width().max(1.0) as u32, screen_height().max(1.0) as u32);
        if self.grab.as_ref().map(|g| g.1 != w || g.2 != h).unwrap_or(true) {
            let tex = Texture2D::from_rgba8(w as u16, h as u16, &vec![0u8; (w * h * 4) as usize]);
            tex.set_filter(FilterMode::Linear);
            self.grab = Some((tex, w, h));
        }
        let Some((tex, _, _)) = self.grab.as_ref() else { return };
        // Dibujar todo lo pendiente y copiar la pantalla a la textura
        unsafe {
            get_internal_gl().flush();
        }
        let params = DrawTextureParams { dest_size: Some(vec2(screen_width(), screen_height())), flip_y: true, ..Default::default() };
        if paint {
            tex.grab_screen();
            self.paint.set_uniform("Texel", vec2(1.0 / w as f32, 1.0 / h as f32));
            self.paint.set_uniform("Spread", (h as f32 / 720.0).clamp(1.0, 2.5));
            gl_use_material(&self.paint);
            draw_texture_ex(tex, 0.0, 0.0, WHITE, params.clone());
            gl_use_default_material();
        }
        if hurt > 0.01 {
            unsafe {
                get_internal_gl().flush();
            }
            tex.grab_screen();
            self.hurt_mat.set_uniform("Texel", vec2(1.0 / w as f32, 1.0 / h as f32));
            self.hurt_mat.set_uniform("Strength", hurt);
            gl_use_material(&self.hurt_mat);
            draw_texture_ex(tex, 0.0, 0.0, WHITE, params);
            gl_use_default_material();
        }
    }

    /// Cielo, nubes y escenario. Deja activo el material opaco.
    fn begin(&mut self, cam: &Cam, time: f32) {
        set_camera(&Camera3D {
            position: tv(cam.eye),
            target: tv(cam.eye + cam.fwd),
            up: tv(cam.up),
            fovy: cam.fov.to_radians(),
            // Plano cercano no demasiado pequeño: más precisión de profundidad
            z_near: 0.08,
            z_far: 1500.0,
            ..Default::default()
        });
        clear_background(Color::new(0.80, 0.89, 0.95, 1.0));
        let sun = geo::sun_dir();
        for m in [&self.opaque, &self.trans, &self.sky] {
            m.set_uniform("SunDir", vec3(sun.x, sun.y, sun.z));
            m.set_uniform("CamPos", tv(cam.eye));
            m.set_uniform("Time", time);
        }
        let right = cam.fwd.cross(cam.up).norm();
        let up = right.cross(cam.fwd).norm();
        gl_use_material(&self.sky);
        self.scratch.clear();
        geo::sky_dome(&mut self.scratch, cam.eye);
        geo::clouds(&mut self.scratch, cam.eye, right, up, time);
        draw_geo(&self.scratch);
        gl_use_material(&self.opaque);
        for (b, meshes) in &self.world {
            if visible(cam, b, 2000.0) {
                for m in meshes {
                    draw_mesh(m);
                }
            }
        }
        for (b, meshes) in &self.grass {
            if visible(cam, b, 60.0) {
                for m in meshes {
                    draw_mesh(m);
                }
            }
        }
        self.art.mats.set_camera(cam.eye);
        for s in &self.art.statics {
            if visible(cam, &s.bounds, 2000.0) {
                s.draw_opaque(&self.art.mats);
            }
        }
        for s in &self.art.statics {
            if visible(cam, &s.bounds, 2000.0) {
                s.draw_blend(&self.art.mats);
            }
        }
        gl_use_material(&self.opaque);
    }

    fn opaque(&self, g: &Geo) {
        gl_use_material(&self.opaque);
        draw_geo(g);
    }

    fn transparent(&self, g: &Geo) {
        gl_use_material(&self.trans);
        draw_geo(g);
    }

    fn end(&self) {
        gl_use_default_material();
    }
}

/// Luz solar en un punto (0 = a la sombra).
fn sun_vis(world: &World, p: V3) -> f32 {
    if world.raycast(p, geo::sun_dir(), 80.0).is_some() {
        0.0
    } else {
        1.0
    }
}

// ----------------------------------------------------------------------
// Interfaz: textos, botones y campos
// ----------------------------------------------------------------------

struct Fonts {
    ui: Font,
    ui_bold: Font,
    title: Font,
}

thread_local! {
    // Fuentes «filtradas» a propósito: si se liberasen al salir, cuando ya no
    // hay contexto gráfico, el programa fallaría al cerrarse.
    static FONTS: RefCell<Option<&'static Fonts>> = const { RefCell::new(None) };
}

fn load_fonts() {
    let load = |b: &[u8]| load_ttf_font_from_bytes(b).ok();
    let (Some(ui), Some(ui_bold), Some(title)) = (
        load(include_bytes!("../assets/fonts/Outfit-Regular.ttf")),
        load(include_bytes!("../assets/fonts/Outfit-Bold.ttf")),
        load(include_bytes!("../assets/fonts/Lora-Bold.ttf")),
    ) else {
        return;
    };
    FONTS.with(|f| *f.borrow_mut() = Some(Box::leak(Box::new(Fonts { ui, ui_bold, title }))));
}

#[derive(Clone, Copy)]
enum FontKind {
    Ui,
    Bold,
    Title,
}

fn with_font<R>(k: FontKind, f: impl FnOnce(Option<&Font>) -> R) -> R {
    FONTS.with(|fs| {
        let fs = fs.borrow();
        let font = fs.as_ref().map(|x| match k {
            FontKind::Ui => &x.ui,
            FontKind::Bold => &x.ui_bold,
            FontKind::Title => &x.title,
        });
        f(font)
    })
}

fn text_wk(k: FontKind, s: &str, size: f32) -> f32 {
    with_font(k, |f| measure_text(s, f, size as u16, 1.0).width)
}

fn txt_k(k: FontKind, s: &str, x: f32, y: f32, size: f32, col: Color) {
    with_font(k, |font| {
        let params = |c: Color| TextParams { font, font_size: size as u16, color: c, ..Default::default() };
        draw_text_ex(s, x + 1.5, y + 1.5, params(Color::new(0.0, 0.0, 0.0, 0.5 * col.a)));
        draw_text_ex(s, x, y, params(col));
    });
}

fn title_c(s: &str, x: f32, y: f32, size: f32, col: Color) {
    txt_k(FontKind::Title, s, x - text_wk(FontKind::Title, s, size) / 2.0, y, size, col);
}

fn bold_c(s: &str, x: f32, y: f32, size: f32, col: Color) {
    txt_k(FontKind::Bold, s, x - text_wk(FontKind::Bold, s, size) / 2.0, y, size, col);
}

fn text_w(s: &str, size: f32) -> f32 {
    text_wk(FontKind::Ui, s, size)
}

fn txt(s: &str, x: f32, y: f32, size: f32, col: Color) {
    txt_k(FontKind::Ui, s, x, y, size, col);
}

fn txt_c(s: &str, x: f32, y: f32, size: f32, col: Color) {
    txt(s, x - text_w(s, size) / 2.0, y, size, col);
}

fn txt_r(s: &str, x: f32, y: f32, size: f32, col: Color) {
    txt(s, x - text_w(s, size), y, size, col);
}

/// Rectángulo con esquinas redondeadas.
fn round_rect(r: Rect, rad: f32, c: Color) {
    use std::f32::consts::PI;
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0);
    draw_rectangle(r.x + rad, r.y, r.w - 2.0 * rad, r.h, c);
    draw_rectangle(r.x, r.y + rad, rad, r.h - 2.0 * rad, c);
    draw_rectangle(r.x + r.w - rad, r.y + rad, rad, r.h - 2.0 * rad, c);
    // Esquinas como abanicos de triángulos, sin solaparse con los rectángulos
    let corners = [
        (r.x + rad, r.y + rad, PI),
        (r.x + r.w - rad, r.y + rad, 1.5 * PI),
        (r.x + r.w - rad, r.y + r.h - rad, 0.0),
        (r.x + rad, r.y + r.h - rad, 0.5 * PI),
    ];
    let n = 6;
    for (cx, cy, a0) in corners {
        for k in 0..n {
            let a = a0 + k as f32 / n as f32 * PI * 0.5;
            let b = a0 + (k + 1) as f32 / n as f32 * PI * 0.5;
            draw_triangle(vec2(cx, cy), vec2(cx + rad * a.cos(), cy + rad * a.sin()), vec2(cx + rad * b.cos(), cy + rad * b.sin()), c);
        }
    }
}

fn panel(r: Rect, a: f32) {
    round_rect(Rect::new(r.x + 3.0, r.y + 5.0, r.w, r.h), 14.0, Color::new(0.0, 0.05, 0.1, 0.18 * a));
    round_rect(r, 14.0, Color::new(0.10, 0.17, 0.16, a * 0.92));
}

fn button(r: Rect, label: &str) -> bool {
    let hover = r.contains(mouse_position().into());
    round_rect(Rect::new(r.x + 2.0, r.y + 4.0, r.w, r.h), 12.0, Color::new(0.0, 0.05, 0.1, 0.25));
    let bg = if hover { Color::new(0.96, 0.78, 0.42, 0.97) } else { Color::new(0.98, 0.95, 0.87, 0.93) };
    round_rect(r, 12.0, bg);
    let ink = Color::new(0.24, 0.18, 0.12, 1.0);
    with_font(FontKind::Title, |font| {
        let w = measure_text(label, font, 24, 1.0).width;
        draw_text_ex(label, r.x + (r.w - w) / 2.0, r.y + r.h / 2.0 + 8.0, TextParams { font, font_size: 24, color: ink, ..Default::default() });
    });
    hover && is_mouse_button_pressed(MouseButton::Left)
}

fn small_button(r: Rect, label: &str) -> bool {
    let hover = r.contains(mouse_position().into());
    let bg = if hover { Color::new(0.96, 0.78, 0.42, 0.97) } else { Color::new(0.94, 0.90, 0.80, 0.9) };
    round_rect(r, 8.0, bg);
    let ink = Color::new(0.24, 0.18, 0.12, 1.0);
    with_font(FontKind::Bold, |font| {
        let w = measure_text(label, font, 20, 1.0).width;
        draw_text_ex(label, r.x + (r.w - w) / 2.0, r.y + r.h / 2.0 + 7.0, TextParams { font, font_size: 20, color: ink, ..Default::default() });
    });
    hover && is_mouse_button_pressed(MouseButton::Left)
}

/// Campo de texto. `focus` guarda qué campo tiene el foco.
fn text_field(r: Rect, label: &str, value: &mut String, id: u32, focus: &mut u32, max: usize, allowed: fn(char) -> bool) {
    let hover = r.contains(mouse_position().into());
    if is_mouse_button_pressed(MouseButton::Left) {
        if hover {
            *focus = id;
        } else if *focus == id {
            *focus = 0;
        }
    }
    let focused = *focus == id;
    txt(label, r.x, r.y - 8.0, 20.0, DIM);
    round_rect(r, 8.0, Color::new(0.03, 0.07, 0.07, 0.7));
    if focused {
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, with_alpha(GOLD, 0.9));
    }
    if focused {
        while let Some(c) = get_char_pressed() {
            if !c.is_control() && allowed(c) && value.chars().count() < max {
                value.push(c);
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            value.pop();
        }
    }
    let caret = if focused && (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
    txt(&format!("{value}{caret}"), r.x + 10.0, r.y + r.h / 2.0 + 8.0, 24.0, CREAM);
}

fn any_char(_: char) -> bool {
    true
}
fn addr_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '-'
}
fn digit_char(c: char) -> bool {
    c.is_ascii_digit()
}

fn stepper(x: f32, y: f32, label: &str, value: &str) -> i32 {
    txt(label, x, y + 24.0, 22.0, DIM);
    let mut d = 0;
    if small_button(Rect::new(x + 260.0, y, 36.0, 34.0), "-") {
        d = -1;
    }
    txt_c(value, x + 340.0, y + 25.0, 24.0, CREAM);
    if small_button(Rect::new(x + 384.0, y, 36.0, 34.0), "+") {
        d = 1;
    }
    d
}

// ----------------------------------------------------------------------
// Aplicación
// ----------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum Screen {
    Main,
    Join,
    Host,
    Options,
    Connecting,
    Game,
    Error(String),
}

struct Host {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    port: u16,
}

struct App {
    settings: Settings,
    screen: Screen,
    world: World,
    r: Renderer,
    dg: Geo,
    pg: Geo,
    sounds: Sounds,
    client: Option<NetClient>,
    host: Option<Host>,
    scanner: Option<LanScanner>,
    focus: u32,
    port_text: String,
    // En partida
    last_mouse: Option<Vec2>,
    crouch: bool,
    pause: bool,
    team_menu: bool,
    chat: Option<(String, bool)>,
    feed: Vec<FeedLine>,
    chat_log: Vec<(String, Color, f32)>,
    announces: Vec<(String, u8, f32)>,
    tracers: Vec<(V3, V3, f32)>,
    fx: Vec<Fx>,
    indicators: Vec<(V3, f32)>,
    hitmarker: f32,
    shake: f32,
    corpses: Vec<Corpse>,
    alive_prev: HashMap<u8, bool>,
    walk: HashMap<u8, (V3, f32)>,
    fired: HashMap<u8, f32>,
    flash: HashMap<u8, f32>,
    my_death: Option<(V3, f32, f32)>,
    killed_by: Option<String>,
    killer_id: Option<u8>,
    /// Menú de equipamiento (clase, arma y ventaja)
    loadout_menu: bool,
    /// Ya se ha mostrado el menú de equipamiento al entrar
    loadout_shown: bool,
    /// «+25 Baja» y similares: (texto, tiempo)
    xp_pops: Vec<(String, f32)>,
    heart_t: f32,
    /// Balanceo de la mira telescópica ya aplicado (yaw, pitch)
    sway: (f32, f32),
    breath: f32,
    /// Pasos propios: último paso dado, si estaba en el suelo y velocidad de caída
    step_n: i64,
    was_grounded: bool,
    fall: f32,
    /// Pasos de los demás (último paso de cada uno)
    remote_steps: HashMap<u8, i64>,
    spectate: usize,
    rng: Rng,
    clock: f32,
    /// Paso del modo de capturas (0 = desactivado o sin cámara impuesta).
    demo_step: usize,
}

impl App {
    fn new(settings: Settings, sounds: Sounds) -> App {
        let world = World::load();
        let r = Renderer::new(&world);
        let port_text = settings.host_port.to_string();
        App {
            settings,
            screen: Screen::Main,
            world,
            r,
            dg: Geo::new(),
            pg: Geo::new(),
            sounds,
            client: None,
            host: None,
            scanner: None,
            focus: 0,
            port_text,
            last_mouse: None,
            crouch: false,
            pause: false,
            team_menu: false,
            chat: None,
            feed: Vec::new(),
            chat_log: Vec::new(),
            announces: Vec::new(),
            tracers: Vec::new(),
            fx: Vec::new(),
            indicators: Vec::new(),
            hitmarker: 0.0,
            shake: 0.0,
            corpses: Vec::new(),
            alive_prev: HashMap::new(),
            walk: HashMap::new(),
            fired: HashMap::new(),
            flash: HashMap::new(),
            my_death: None,
            killed_by: None,
            killer_id: None,
            loadout_menu: false,
            loadout_shown: false,
            xp_pops: Vec::new(),
            heart_t: 0.0,
            sway: (0.0, 0.0),
            breath: 4.0,
            step_n: 0,
            was_grounded: true,
            fall: 0.0,
            remote_steps: HashMap::new(),
            spectate: 0,
            rng: Rng::new((get_time() * 1e6) as u64 ^ 0xF44),
            clock: 0.0,
            demo_step: 0,
        }
    }

    fn grab(&self, on: bool) {
        set_cursor_grab(on);
        show_mouse(!on);
    }

    fn reset_game_state(&mut self) {
        self.last_mouse = None;
        self.crouch = false;
        self.pause = false;
        self.team_menu = false;
        self.chat = None;
        self.feed.clear();
        self.chat_log.clear();
        self.announces.clear();
        self.tracers.clear();
        self.fx.clear();
        self.indicators.clear();
        self.corpses.clear();
        self.alive_prev.clear();
        self.walk.clear();
        self.fired.clear();
        self.flash.clear();
        self.my_death = None;
        self.killed_by = None;
        self.killer_id = None;
        self.loadout_menu = false;
        self.loadout_shown = false;
        self.xp_pops.clear();
    }

    fn connect_to(&mut self, addr: std::net::SocketAddr) {
        self.reset_game_state();
        match NetClient::connect_map(addr, &self.settings.name, self.world.map_id) {
            Ok(c) => {
                self.client = Some(c);
                self.screen = Screen::Connecting;
            }
            Err(e) => self.screen = Screen::Error(format!("No se pudo abrir la red: {e}")),
        }
    }

    fn leave(&mut self, reason: Option<String>) {
        if let Some(c) = self.client.as_mut() {
            if c.status == Status::Connected {
                c.disconnect();
            }
        }
        self.client = None;
        if let Some(mut h) = self.host.take() {
            h.stop.store(true, Ordering::Relaxed);
            if let Some(t) = h.thread.take() {
                let _ = t.join();
            }
        }
        self.grab(false);
        self.screen = match reason {
            Some(r) => Screen::Error(r),
            None => Screen::Main,
        };
    }

    fn start_host(&mut self) {
        let modes: Vec<Mode> = self.settings.host_modes.iter().filter_map(|m| Mode::parse(m)).collect();
        let cfg = ServerConfig {
            name: clean_name(&self.settings.host_name, 32),
            port: self.settings.host_port,
            max_players: self.settings.host_max as usize,
            modes: if modes.is_empty() { vec![Mode::Tdm] } else { modes },
            bots: self.settings.host_bots as usize,
            bot_skill: deia::server::bot_skill_of(self.settings.host_bot_skill),
            allied_nation: self.settings.host_nation,
            // Con bots se puede empezar a jugar solo
            min_players: if self.settings.host_bots > 0 { 1 } else { 2 },
            ..Default::default()
        };
        match spawn_listen_server(cfg) {
            Ok((port, stop, thread)) => {
                self.host = Some(Host { stop, thread: Some(thread), port });
                let addr = format!("127.0.0.1:{port}").parse().unwrap();
                self.connect_to(addr);
            }
            Err(e) => {
                self.screen = Screen::Error(format!(
                    "No se pudo abrir el puerto {}: {e}\n¿Hay otro servidor abierto en este equipo? Prueba con otro puerto.",
                    self.settings.host_port
                ))
            }
        }
    }

    // ------------------------------------------------------------------
    // Menús
    // ------------------------------------------------------------------

    fn menu_camera(&self) -> Cam {
        // Vuelo lento por fuera del pueblo, a la altura de los tejados
        let a = 2.3 + self.clock * 0.035;
        let eye = v3(a.cos() * 80.0, 15.0 + 3.0 * (self.clock * 0.05).sin(), a.sin() * 80.0);
        Cam { eye, fwd: (v3(0.0, 7.0, 0.0) - eye).norm(), up: V3::UP, fov: 55.0 }
    }

    fn menu_background(&mut self) {
        let cam = self.menu_camera();
        self.r.begin(&cam, self.clock);
        let mut dg = std::mem::take(&mut self.dg);
        dg.clear();
        for s in self.world.bomb_sites.iter() {
            geo::bomb_site(&mut dg, *s);
        }
        for t in 0..2u8 {
            geo::flag(&mut dg, self.world.flag_bases[t as usize], t, self.clock, true);
        }
        self.r.opaque(&dg);
        self.dg = dg;
        self.r.end();
        self.r.finish();
        // Velo suave arriba para que el título se lea sobre el cielo
        let (sw, sh) = (screen_width(), screen_height());
        for k in 0..24 {
            let a = 0.3 * (1.0 - k as f32 / 24.0);
            draw_rectangle(0.0, k as f32 * 8.0, sw, 8.0, Color::new(0.05, 0.12, 0.2, a));
        }
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.03, 0.06, 0.12));
    }

    fn title(&self) {
        let cx = screen_width() / 2.0;
        let size = (screen_width() / 14.0).min(78.0);
        title_c("Eginbeharraren deia", cx, 112.0, size, Color::new(1.0, 0.98, 0.92, 1.0));
        txt_c("Multijugador · Saint-Aubin, Normandía, 1944", cx, 150.0, 24.0, Color::new(1.0, 0.97, 0.9, 0.95));
    }

    fn screen_main(&mut self) {
        self.menu_background();
        self.title();
        let cx = screen_width() / 2.0;
        let w = 360.0;
        let mut y = 220.0;
        let mut b = |label: &str| {
            let r = Rect::new(cx - w / 2.0, y, w, 52.0);
            y += 66.0;
            button(r, label)
        };
        let join = b("Unirse a una partida");
        let host = b("Crear partida");
        let opts = b("Ajustes");
        let quit = b("Salir");
        txt_c(&format!("Jugador: {}", self.settings.name), cx, y + 30.0, 22.0, DIM);
        if join {
            self.scanner = Some(LanScanner::new());
            self.focus = 0;
            self.screen = Screen::Join;
        }
        if host {
            self.port_text = self.settings.host_port.to_string();
            self.focus = 0;
            self.screen = Screen::Host;
        }
        if opts {
            self.focus = 0;
            self.screen = Screen::Options;
        }
        if quit {
            std::process::exit(0);
        }
    }

    fn screen_join(&mut self) {
        self.menu_background();
        self.title();
        if let Some(s) = self.scanner.as_mut() {
            s.poll();
        }
        let cx = screen_width() / 2.0;
        let w = 720.0f32.min(screen_width() - 40.0);
        let x = cx - w / 2.0;
        panel(Rect::new(x - 20.0, 180.0, w + 40.0, 470.0), 0.8);
        text_field(Rect::new(x, 220.0, 300.0, 40.0), "Tu nombre", &mut self.settings.name, 1, &mut self.focus, 20, any_char);

        txt("Partidas en tu red local", x, 300.0, 24.0, CREAM);
        let found: Vec<(std::net::SocketAddr, ServerInfo)> =
            self.scanner.as_ref().map(|s| s.found.iter().map(|f| (f.0, f.1.clone())).collect()).unwrap_or_default();
        let mut chosen = None;
        if found.is_empty() {
            txt("Buscando… (las partidas de internet se añaden por dirección)", x, 334.0, 20.0, DIM);
        }
        for (k, (addr, info)) in found.iter().take(5).enumerate() {
            let r = Rect::new(x, 314.0 + k as f32 * 44.0, w, 38.0);
            let hover = r.contains(mouse_position().into());
            draw_rectangle(r.x, r.y, r.w, r.h, if hover { Color::new(0.5, 0.42, 0.25, 0.8) } else { Color::new(0.1, 0.1, 0.08, 0.8) });
            txt(&info.name, r.x + 10.0, r.y + 26.0, 22.0, CREAM);
            txt_r(&format!("{} · {}/{} · {}", info.mode.name(), info.players, info.max, addr), r.x + r.w - 10.0, r.y + 26.0, 20.0, DIM);
            if hover && is_mouse_button_pressed(MouseButton::Left) {
                chosen = Some(*addr);
            }
        }
        text_field(
            Rect::new(x, 560.0, 420.0, 40.0),
            "Dirección del servidor (IP o IP:puerto)",
            &mut self.settings.last_address,
            2,
            &mut self.focus,
            60,
            addr_char,
        );
        let go = small_button(Rect::new(x + 440.0, 560.0, 130.0, 40.0), "Conectar")
            || (self.focus == 2 && is_key_pressed(KeyCode::Enter));
        let back = small_button(Rect::new(x + w - 120.0, 560.0, 120.0, 40.0), "Volver") || is_key_pressed(KeyCode::Escape);
        if go {
            match resolve(&self.settings.last_address) {
                Some(a) => chosen = Some(a),
                None => self.screen = Screen::Error(format!("No se reconoce la dirección «{}»", self.settings.last_address)),
            }
        }
        if let Some(a) = chosen {
            self.settings.name = clean_name(&self.settings.name, 20);
            self.settings.save();
            self.scanner = None;
            self.connect_to(a);
        } else if back {
            self.settings.save();
            self.scanner = None;
            self.screen = Screen::Main;
        }
    }

    fn screen_host(&mut self) {
        self.menu_background();
        self.title();
        let cx = screen_width() / 2.0;
        let w = 720.0f32.min(screen_width() - 40.0);
        let x = cx - w / 2.0;
        panel(Rect::new(x - 20.0, 180.0, w + 40.0, 500.0), 0.8);
        text_field(Rect::new(x, 220.0, 340.0, 40.0), "Nombre de la partida", &mut self.settings.host_name, 3, &mut self.focus, 32, any_char);
        text_field(Rect::new(x + 370.0, 220.0, 150.0, 40.0), "Puerto UDP", &mut self.port_text, 4, &mut self.focus, 5, digit_char);
        let d = stepper(x, 272.0, "Jugadores máximos", &self.settings.host_max.to_string());
        self.settings.host_max = (self.settings.host_max as i32 + d).clamp(2, MAX_PLAYERS as i32) as u8;
        let bots = if self.settings.host_bots == 0 { "Sin bots".to_string() } else { format!("Hasta {}", self.settings.host_bots) };
        let d = stepper(x, 314.0, "Bots", &bots);
        self.settings.host_bots = (self.settings.host_bots as i32 + d).clamp(0, MAX_PLAYERS as i32 - 1) as u8;
        let levels = ["Novatos", "Regulares", "Veteranos", "Élite"];
        let d = stepper(x + 370.0, 272.0, "Nivel de los bots", levels[self.settings.host_bot_skill.min(3) as usize]);
        self.settings.host_bot_skill = (self.settings.host_bot_skill as i32 + d).clamp(0, 3) as u8;
        let nats = [nation::US, nation::UK, nation::RU];
        let cur = nats.iter().position(|&n| n == self.settings.host_nation).unwrap_or(0);
        let d = stepper(x + 370.0, 314.0, "Aliados", nation_name(nats[cur]));
        self.settings.host_nation = nats[(cur as i32 + d).rem_euclid(3) as usize];

        txt("Modos (se juegan en rotación, en este orden):", x, 374.0, 22.0, CREAM);
        for (k, m) in Mode::ALL.iter().enumerate() {
            let on = self.settings.host_modes.iter().any(|s| s == m.short());
            let r = Rect::new(x + (k % 3) as f32 * 240.0, 388.0 + (k / 3) as f32 * 44.0, 228.0, 36.0);
            let label = format!("{} {}", if on { "[x]" } else { "[ ]" }, m.name());
            if small_button(r, &label) {
                if on {
                    if self.settings.host_modes.len() > 1 {
                        self.settings.host_modes.retain(|s| s != m.short());
                    }
                } else {
                    self.settings.host_modes.push(m.short().to_string());
                }
            }
        }
        let port: Option<u16> = self.port_text.parse().ok().filter(|p| *p > 0);
        let info = format!(
            "Por internet: abre el puerto UDP {} en tu router hacia este PC y pasa tu IP pública a tus amigos.",
            port.map(|p| p.to_string()).unwrap_or_else(|| "?".into())
        );
        txt(&info, x, 500.0, 18.0, DIM);
        txt("En la misma red local te verán automáticamente en «Unirse a una partida».", x, 522.0, 18.0, DIM);
        let go = button(Rect::new(x, 560.0, 300.0, 52.0), "Crear y entrar");
        let back = button(Rect::new(x + w - 200.0, 560.0, 200.0, 52.0), "Volver") || is_key_pressed(KeyCode::Escape);
        if go {
            match port {
                Some(p) => {
                    self.settings.host_port = p;
                    self.settings.save();
                    self.start_host();
                }
                None => self.screen = Screen::Error("El puerto debe ser un número entre 1 y 65535".into()),
            }
        } else if back {
            self.settings.save();
            self.screen = Screen::Main;
        }
    }

    fn screen_options(&mut self) {
        self.menu_background();
        self.title();
        let cx = screen_width() / 2.0;
        let x = cx - 260.0;
        panel(Rect::new(x - 20.0, 180.0, 560.0, 500.0), 0.8);
        text_field(Rect::new(x, 220.0, 300.0, 40.0), "Tu nombre", &mut self.settings.name, 5, &mut self.focus, 20, any_char);
        let s = &mut self.settings;
        let d = stepper(x, 280.0, "Sensibilidad del ratón", &format!("{:.1}", s.sensitivity));
        s.sensitivity = (s.sensitivity + d as f32 * 0.2).clamp(0.2, 10.0);
        let d = stepper(x, 330.0, "Campo de visión", &format!("{:.0}°", s.fov));
        s.fov = (s.fov + d as f32 * 5.0).clamp(60.0, 90.0);
        let d = stepper(x, 380.0, "Volumen de efectos", &format!("{:.0} %", s.volume * 100.0));
        s.volume = (s.volume + d as f32 * 0.1).clamp(0.0, 1.0);
        let d = stepper(x, 550.0, "Música y ambiente", &format!("{:.0} %", s.music * 100.0));
        s.music = (s.music + d as f32 * 0.1).clamp(0.0, 1.0);
        txt("Invertir eje vertical", x, 454.0, 22.0, DIM);
        if small_button(Rect::new(x + 260.0, 430.0, 160.0, 34.0), if s.invert { "Sí" } else { "No" }) {
            s.invert = !s.invert;
        }
        txt("Efecto de pintura", x, 504.0, 22.0, DIM);
        if small_button(Rect::new(x + 260.0, 480.0, 160.0, 34.0), if s.paint { "Sí" } else { "No" }) {
            s.paint = !s.paint;
        }
        self.r.paint_on = s.paint;
        self.sounds.volume = self.settings.volume;
        if button(Rect::new(x, 600.0, 260.0, 52.0), "Guardar y volver") || is_key_pressed(KeyCode::Escape) {
            self.settings.name = clean_name(&self.settings.name, 20);
            self.settings.save();
            self.screen = Screen::Main;
        }
    }

    fn screen_message(&mut self, title: &str, msg: &str, cancel: &str) -> bool {
        self.menu_background();
        self.title();
        let cx = screen_width() / 2.0;
        panel(Rect::new(cx - 360.0, 230.0, 720.0, 240.0), 0.85);
        txt_c(title, cx, 280.0, 32.0, GOLD);
        for (k, line) in msg.lines().enumerate() {
            txt_c(line, cx, 330.0 + k as f32 * 28.0, 22.0, CREAM);
        }
        button(Rect::new(cx - 120.0, 400.0, 240.0, 50.0), cancel) || is_key_pressed(KeyCode::Escape)
    }

    // ------------------------------------------------------------------
    // Partida
    // ------------------------------------------------------------------

    fn read_input(&mut self, ads: f32, typing: bool) -> FrameInput {
        let mp: Vec2 = mouse_position().into();
        let md = self.last_mouse.map(|l| mp - l).unwrap_or(Vec2::ZERO).clamp(vec2(-250.0, -250.0), vec2(250.0, 250.0));
        self.last_mouse = Some(mp);
        let mut i = FrameInput::default();
        if self.pause || self.team_menu || self.loadout_menu || typing {
            return i;
        }
        let snap = self.client.as_ref().and_then(|c| c.snap.as_ref());
        let fov_k = snap
            .and_then(|_| self.client.as_ref()?.pred.as_ref().map(|p| p.gun().ads_fov / self.settings.fov))
            .unwrap_or(1.0);
        let s = self.settings.sensitivity * 0.001 * lerp(1.0, fov_k, ads);
        i.look_yaw = md.x * s;
        i.look_pitch = -md.y * s * if self.settings.invert { -1.0 } else { 1.0 };
        let k = |a: KeyCode, b: KeyCode| is_key_down(a) || is_key_down(b);
        i.fwd = k(KeyCode::W, KeyCode::Up) as i32 as f32 - k(KeyCode::S, KeyCode::Down) as i32 as f32;
        i.strafe = k(KeyCode::D, KeyCode::Right) as i32 as f32 - k(KeyCode::A, KeyCode::Left) as i32 as f32;
        if is_key_pressed(KeyCode::C) || is_key_pressed(KeyCode::LeftControl) {
            self.crouch = !self.crouch;
        }
        i.jump = is_key_pressed(KeyCode::Space);
        if i.jump {
            self.crouch = false;
        }
        i.sprint = is_key_down(KeyCode::LeftShift);
        if i.sprint && i.fwd > 0.5 {
            self.crouch = false;
        }
        i.crouch = self.crouch;
        i.fire = is_mouse_button_down(MouseButton::Left);
        i.fire_pressed = is_mouse_button_pressed(MouseButton::Left);
        i.ads = is_mouse_button_down(MouseButton::Right);
        i.reload = is_key_pressed(KeyCode::R);
        i.grenade = is_key_pressed(KeyCode::G);
        i.smoke = is_key_pressed(KeyCode::H) || is_key_pressed(KeyCode::Key4);
        i.melee = is_key_pressed(KeyCode::V) || is_mouse_button_pressed(MouseButton::Middle);
        i.use_key = is_key_down(KeyCode::F);
        i.mount = is_key_pressed(KeyCode::F);
        if is_key_pressed(KeyCode::Key1) {
            i.slot = Some(0);
        }
        if is_key_pressed(KeyCode::Key2) {
            i.slot = Some(1);
        }
        let (_, wy) = mouse_wheel();
        i.next_slot = wy.abs() > 0.1 || is_key_pressed(KeyCode::Q);
        i
    }

    fn game_frame(&mut self, dt: f32) {
        // Teclas de interfaz
        let typing = self.chat.is_some();
        if typing {
            let (mut text, team) = self.chat.take().unwrap();
            while let Some(c) = get_char_pressed() {
                if !c.is_control() && text.chars().count() < 100 {
                    text.push(c);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                text.pop();
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                if let Some(c) = self.client.as_mut() {
                    c.send_chat(&text, team);
                }
            } else if !is_key_pressed(KeyCode::Escape) {
                self.chat = Some((text, team));
            }
        } else if !self.pause && !self.team_menu && !self.loadout_menu {
            if is_key_pressed(KeyCode::T) {
                while get_char_pressed().is_some() {}
                self.chat = Some((String::new(), false));
            } else if is_key_pressed(KeyCode::Y) {
                while get_char_pressed().is_some() {}
                self.chat = Some((String::new(), true));
            } else if is_key_pressed(KeyCode::M) {
                self.team_menu = true;
                self.grab(false);
            } else if is_key_pressed(KeyCode::L) || is_key_pressed(KeyCode::B) {
                self.loadout_menu = true;
                self.grab(false);
            } else if is_key_pressed(KeyCode::Escape) {
                self.pause = true;
                self.grab(false);
            }
        } else if is_key_pressed(KeyCode::Escape) {
            self.pause = false;
            self.team_menu = false;
            self.loadout_menu = false;
            self.last_mouse = None;
            self.grab(true);
        }
        // Al entrar, elegir equipamiento
        if !self.loadout_shown && self.client.as_ref().map(|c| c.snap.is_some()).unwrap_or(false) && self.demo_step == 0 {
            self.loadout_shown = true;
            self.loadout_menu = true;
            self.grab(false);
        }

        let ads = self.client.as_ref().and_then(|c| c.pred.as_ref().map(|p| p.ads)).unwrap_or(0.0);
        let mut inp = self.read_input(ads, self.chat.is_some());
        // Balanceo de la mira telescópica (menos agachado, con pulso firme o
        // aguantando la respiración con Mayúsculas)
        let scoped = self.client.as_ref().and_then(|c| c.pred.as_ref()).map(|p| (p.gun().scoped() && p.ads > 0.3, p.crouch, p.perk)).unwrap_or((false, 0.0, 0));
        let hold = scoped.0 && is_key_down(KeyCode::LeftShift) && self.breath > 0.0;
        self.breath = if hold { self.breath - dt } else { (self.breath + dt * 0.5).min(4.0) };
        let amp = if scoped.0 { 0.012 * lerp(1.0, 0.5, scoped.1) * if scoped.2 == perk::STEADY { 0.5 } else { 1.0 } * if hold { 0.15 } else { 1.0 } } else { 0.0 };
        let t = self.clock;
        let want = (amp * (t * 0.9).sin() + amp * 0.4 * (t * 2.3).sin(), amp * 0.7 * (t * 1.3).sin());
        inp.look_yaw += want.0 - self.sway.0;
        inp.look_pitch += want.1 - self.sway.1;
        self.sway = want;
        if let Some(c) = self.client.as_mut() {
            c.loadout = Loadout { class: self.settings.class, primary: self.settings.primary, perk: self.settings.perk, secondary: self.settings.secondary };
            c.level = 1;
        }
        let mut client = self.client.take().unwrap();
        client.update(&inp, dt, &self.world);
        if let Status::Failed(why) = client.status.clone() {
            self.client = Some(client);
            self.leave(Some(why));
            return;
        }
        self.process_events(&client, dt);
        if let Some(p) = client.pred.as_ref() {
            // Un paso cada media zancada del balanceo al andar
            let n = (p.bob / std::f32::consts::PI).floor() as i64;
            if n != self.step_n && p.grounded && p.moving > 0.15 {
                let v = if p.crouching { 0.15 } else if p.sprinting { 0.5 } else { 0.32 };
                self.sounds.play(&self.sounds.steps[(n.rem_euclid(4)) as usize], v);
            }
            self.step_n = n;
            if self.was_grounded && !p.grounded && p.vel.y > 1.0 {
                self.sounds.play(&self.sounds.jump, 0.45);
            }
            if !p.grounded {
                self.fall = self.fall.max(-p.vel.y);
            }
            if !self.was_grounded && p.grounded {
                if self.fall > 2.5 {
                    self.sounds.play(&self.sounds.land, (self.fall / 8.0).clamp(0.3, 0.8));
                }
                self.fall = 0.0;
            }
            self.was_grounded = p.grounded;
        }
        self.client = Some(client);

        // Espectador: clic para cambiar de jugador
        let c = self.client.as_ref().unwrap();
        if !c.alive() && !self.pause && !self.team_menu && !self.loadout_menu && is_mouse_button_pressed(MouseButton::Left) {
            self.spectate += 1;
        }

        let cam = self.game_camera();
        self.r.paint_on = self.settings.paint;
        self.render_game(&cam);
        self.r.finish();
        if self.demo_camera(self.demo_step).is_none() {
            self.hud(&cam);
        }
        if self.team_menu {
            self.draw_team_menu();
        }
        if self.loadout_menu {
            self.draw_loadout_menu();
        }
        if self.pause {
            self.draw_pause();
        }
    }

    fn process_events(&mut self, c: &NetClient, dt: f32) {
        let me = c.my_id;
        let snap = c.snap.as_ref();
        let my_team = snap.map(|s| s.my_team).unwrap_or(0);
        let teams = snap.map(|s| s.mode.teams()).unwrap_or(false);
        let ear = c.eye().unwrap_or_else(|| self.my_death.map(|d| d.0 + V3::UP).unwrap_or(V3::ZERO));
        let near = |p: V3, range: f32| (1.0 - p.dist(ear) / range).clamp(0.0, 1.0);
        let yaw = c.view_yaw;
        let team_of = |id: u8| snap.and_then(|s| s.players.iter().find(|p| p.id == id)).map(|p| p.team).unwrap_or(0);

        for fx in &c.local {
            match *fx {
                LocalFx::Shot { gun: g } => {
                    self.sounds.play(&self.sounds.guns[gun_sound(g)], 0.75);
                    if matches!(gun(g).kind, GunKind::Rifle | GunKind::Sniper) && gun(g).name != "M1 Garand" {
                        self.sounds.play(&self.sounds.bolt, 0.35);
                    }
                }
                LocalFx::Reload { .. } => self.sounds.play(&self.sounds.reload, 0.6),
                LocalFx::Ping => self.sounds.play(&self.sounds.ping, 0.7),
                LocalFx::Dry => self.sounds.play(&self.sounds.dry, 0.6),
                LocalFx::Throw => {
                    self.sounds.play(&self.sounds.pin, 0.5);
                    self.sounds.play(&self.sounds.throw, 0.6);
                }
                LocalFx::Switch => self.sounds.play(&self.sounds.switch, 0.5),
                LocalFx::Melee => self.sounds.play(&self.sounds.swing, 0.6),
                LocalFx::Mount => self.sounds.play(&self.sounds.reload, 0.5),
            }
        }

        for e in &c.events {
            match e {
                Ev::Shot { shooter, from, to, gun } => {
                    self.tracers.push((*from, *to, 0.05));
                    if *shooter != me {
                        // La bala pasa silbando cerca
                        let seg = *to - *from;
                        let len = seg.len().max(0.01);
                        let t = ((ear - *from).dot(seg) / (len * len)).clamp(0.0, 1.0);
                        let close = (*from + seg * t).dist(ear);
                        if close < 2.5 && from.dist(ear) > 4.0 && t < 0.99 {
                            self.sounds.play(&self.sounds.whiz, 0.7 * (1.0 - close / 2.5));
                        }
                    }
                    if to.dist(ear) < 25.0 {
                        self.sounds.play_at(&self.sounds.impact, 0.35 * near(*to, 25.0), *to, ear, yaw);
                    }
                    spray(&mut self.fx, &mut self.rng, *to, 5, [0.55, 0.5, 0.42], 2.2, 1.4, 0.07, 0.5, 9.0, 0.0, 1.0);
                    if *shooter != me {
                        self.flash.insert(*shooter, 0.05);
                        self.fired.insert(*shooter, 0.0);
                        let d = from.dist(ear);
                        let g = gun_sound(*gun);
                        if d < 25.0 || g == 6 {
                            self.sounds.play_at(&self.sounds.guns_at[g], 0.55 * near(*from, if g == 6 { 220.0 } else { 60.0 }).max(0.3), *from, ear, yaw);
                        } else {
                            let far = if deia::sim::gun(*gun).auto { &self.sounds.far_smg } else { &self.sounds.far_rifle };
                            self.sounds.play_at(far, 0.6 * near(*from, 160.0).max(0.08), *from, ear, yaw);
                        }
                    }
                }
                Ev::Hit { shooter } if *shooter == me => {
                    self.hitmarker = 0.2;
                    self.sounds.play(&self.sounds.hit, 0.6);
                }
                Ev::Hit { .. } => {}
                Ev::Hurt { victim, from } => {
                    if *victim == me {
                        self.indicators.push((*from, 1.6));
                        self.shake = (self.shake + 0.15).min(1.0);
                        self.sounds.play(&self.sounds.hurt, 0.7);
                    } else if let Some(p) = c.remotes().iter().find(|p| p.id == *victim) {
                        spray(&mut self.fx, &mut self.rng, p.pos + V3::UP * 1.2, 8, [0.55, 0.05, 0.04], 2.0, 1.0, 0.06, 0.5, 9.0, 0.0, 1.0);
                    }
                }
                Ev::Kill { killer, victim, weapon, head, killer_name, victim_name } => {
                    self.feed.push(FeedLine {
                        killer: killer_name.clone(),
                        kteam: team_of(*killer),
                        victim: victim_name.clone(),
                        vteam: team_of(*victim),
                        weapon: *weapon,
                        head: *head,
                        ttl: 6.0,
                    });
                    if self.feed.len() > 5 {
                        self.feed.remove(0);
                    }
                    if *victim == me {
                        self.killed_by = if killer != victim { Some(killer_name.clone()) } else { None };
                        self.killer_id = if killer != victim { Some(*killer) } else { None };
                    }
                    if *killer == me && *victim != me {
                        let t = if *head { format!("¡Disparo a la cabeza a {victim_name}!") } else { format!("Has abatido a {victim_name}") };
                        self.announces.push((t, 99, 2.5));
                    }
                }
                Ev::Explosion { pos, big } => {
                    let k = if *big { 2.0 } else { 1.0 };
                    spray(&mut self.fx, &mut self.rng, *pos, (14.0 * k) as usize, [1.0, 0.62, 0.2], 7.0 * k, 2.0, 0.35 * k, 0.35, 0.0, 1.5, 0.95);
                    spray(&mut self.fx, &mut self.rng, *pos, (26.0 * k) as usize, [0.32, 0.31, 0.29], 3.0 * k, 2.5, 0.7 * k, 2.6, -0.4, 1.3, 0.75);
                    spray(&mut self.fx, &mut self.rng, *pos, (22.0 * k) as usize, [0.35, 0.28, 0.18], 9.0, 6.0, 0.12, 1.2, 16.0, 0.0, 1.0);
                    self.sounds.play_at(&self.sounds.explosion, near(*pos, 150.0 * k).max(0.1), *pos, ear, yaw);
                    self.shake = (self.shake + (1.0 - pos.dist(ear) / (30.0 * k)).max(0.0)).min(1.2);
                }
                Ev::Bounce { pos } => self.sounds.play(&self.sounds.bounce, 0.5 * near(*pos, 30.0)),
                Ev::Throw { who, pos } if *who != me => self.sounds.play(&self.sounds.throw, 0.4 * near(*pos, 25.0)),
                Ev::Reload { who, pos } if *who != me => self.sounds.play(&self.sounds.reload, 0.35 * near(*pos, 20.0)),
                Ev::Ping { who, pos } if *who != me => self.sounds.play(&self.sounds.ping, 0.6 * near(*pos, 40.0)),
                Ev::Throw { .. } | Ev::Reload { .. } | Ev::Ping { .. } => {}
                Ev::Chat { from, team_only, name, text } => {
                    let t = team_of(*from);
                    if *team_only && (!teams || t != my_team) {
                        continue;
                    }
                    let col = if teams { team_color(t) } else { CREAM };
                    let prefix = if *team_only { "(equipo) " } else { "" };
                    self.chat_log.push((format!("{prefix}{name}: {text}"), col, 10.0));
                    if self.chat_log.len() > 8 {
                        self.chat_log.remove(0);
                    }
                }
                Ev::Announce { text, tone } => {
                    self.announces.push((text.clone(), *tone, 4.5));
                    if self.announces.len() > 4 {
                        self.announces.remove(0);
                    }
                }
                Ev::Whistle => self.sounds.play(&self.sounds.whistle, 0.6),
                // Sin rangos: la experiencia no se usa
                Ev::Xp { who, amount, reason } if *who == me && false => {
                    let before = level_of(self.settings.xp);
                    self.settings.xp = self.settings.xp.saturating_add(*amount as u32);
                    let label = match *reason {
                        xp_reason::KILL => "Baja",
                        xp_reason::HEADSHOT => "Disparo a la cabeza",
                        xp_reason::OBJECTIVE => "Objetivo",
                        xp_reason::WIN => "Victoria",
                        xp_reason::WAVE => "Oleada superada",
                        _ => "Partida jugada",
                    };
                    self.xp_pops.push((format!("+{amount} {label}"), 2.2));
                    if self.xp_pops.len() > 4 {
                        self.xp_pops.remove(0);
                    }
                    let after = level_of(self.settings.xp);
                    if after > before {
                        self.sounds.play(&self.sounds.rank_up, 0.7);
                        self.announces.push((format!("¡Ascenso! Ahora eres {}", RANKS[after as usize - 1]), tone::ALERT, 5.0));
                        for line in unlocks_at(after) {
                            self.announces.push((line, 99, 6.0));
                        }
                    }
                    self.settings.save();
                }
                Ev::Xp { .. } => {}
                Ev::Melee { who, pos, hit } => {
                    if *hit {
                        self.sounds.play_at(&self.sounds.bash, 0.8 * near(*pos, 25.0), *pos, ear, yaw);
                    } else if *who != me {
                        self.sounds.play(&self.sounds.swing, 0.4 * near(*pos, 12.0));
                    }
                }
                Ev::Smoke { pos } => {
                    self.sounds.play_at(&self.sounds.hiss, 0.6 * near(*pos, 40.0), *pos, ear, yaw);
                }
                Ev::Growl { pos, attack, .. } => {
                    if *attack && pos.dist(ear) < 3.0 {
                        self.sounds.play(&self.sounds.claw, 0.8);
                    }
                    self.sounds.play_at(&self.sounds.growl, 0.6 * near(*pos, 35.0), *pos, ear, yaw);
                }
            }
        }

        // Cadáveres y animación de pasos de los demás
        let remotes = c.remotes();
        for p in &remotes {
            let was = self.alive_prev.insert(p.id, p.alive).unwrap_or(p.alive);
            if was && !p.alive {
                self.corpses.push(Corpse { id: p.id, pos: p.pos, yaw: p.yaw, side: p.side, nation: p.nation, gun: p.gun, t: 0.0 });
            }
            if p.alive {
                self.corpses.retain(|k| k.id != p.id);
            }
            let e = self.walk.entry(p.id).or_insert((p.pos, 0.0));
            e.1 += e.0.dist_flat(p.pos) * 2.6;
            e.0 = p.pos;
            let n = (e.1 / std::f32::consts::PI).floor() as i64;
            let last = self.remote_steps.insert(p.id, n).unwrap_or(n);
            if n != last && p.alive && p.pos.dist(ear) < 28.0 {
                let v = if p.crouch > 0.5 { 0.12 } else if p.sprint { 0.45 } else { 0.3 } * near(p.pos, 28.0);
                let k = (n.rem_euclid(2)) as usize;
                self.sounds.play_at(&self.sounds.steps_at[k], v, p.pos, ear, yaw);
            }
        }
        // Mi propia muerte
        match (c.pred.as_ref(), self.my_death) {
            (None, None) => {
                if let Some(info) = c.me_info() {
                    if c.snap.is_some() && self.alive_prev.get(&me).copied().unwrap_or(false) {
                        self.my_death = Some((info.pos, c.view_yaw, 0.0));
                    }
                }
            }
            (Some(_), Some(_)) => {
                self.my_death = None;
                self.killed_by = None;
            }
            _ => {}
        }
        self.alive_prev.insert(me, c.pred.is_some());
        if let Some(d) = self.my_death.as_mut() {
            d.2 += dt;
        }

        // Temporizadores
        for k in self.corpses.iter_mut() {
            k.t += dt;
        }
        self.corpses.retain(|k| k.t < 12.0);
        for f in self.fx.iter_mut() {
            f.vel.y -= f.gravity * dt;
            f.pos += f.vel * dt;
            if f.pos.y < 0.02 {
                f.pos.y = 0.02;
                f.vel *= 0.3;
            }
            f.size += f.grow * dt;
            f.ttl -= dt;
        }
        self.fx.retain(|f| f.ttl > 0.0);
        for t in self.tracers.iter_mut() {
            t.2 -= dt;
        }
        self.tracers.retain(|t| t.2 > 0.0);
        for i in self.indicators.iter_mut() {
            i.1 -= dt;
        }
        self.indicators.retain(|i| i.1 > 0.0);
        for f in self.feed.iter_mut() {
            f.ttl -= dt;
        }
        self.feed.retain(|f| f.ttl > 0.0);
        for l in self.chat_log.iter_mut() {
            l.2 -= dt;
        }
        for a in self.announces.iter_mut() {
            a.2 -= dt;
        }
        self.announces.retain(|a| a.2 > 0.0);
        for v in self.fired.values_mut() {
            *v += dt;
        }
        for v in self.flash.values_mut() {
            *v -= dt;
        }
        self.hitmarker = (self.hitmarker - dt).max(0.0);
        self.shake = (self.shake - dt * 1.8).max(0.0);
        for p in self.xp_pops.iter_mut() {
            p.1 -= dt;
        }
        self.xp_pops.retain(|p| p.1 > 0.0);
        // Latido con poca salud
        if let Some(s) = snap {
            if s.my_alive && s.health < 35.0 {
                self.heart_t -= dt;
                if self.heart_t <= 0.0 {
                    self.heart_t = lerp(0.65, 1.0, s.health / 35.0);
                    self.sounds.play(&self.sounds.heartbeat, 0.7);
                }
            }
        }
    }

    fn game_camera(&self) -> Cam {
        if let Some(cam) = self.demo_camera(self.demo_step) {
            return cam;
        }
        let c = self.client.as_ref().unwrap();
        let t = self.clock;
        let snap = c.snap.as_ref();
        if snap.map(|s| s.phase == Phase::Intermission).unwrap_or(false) {
            let a = t * 0.06;
            let eye = v3(a.cos() * 50.0, 22.0, a.sin() * 50.0);
            return Cam { eye, fwd: (v3(0.0, 2.0, 4.0) - eye).norm(), up: V3::UP, fov: 55.0 };
        }
        if let (Some(p), Some(eye)) = (c.pred.as_ref(), c.eye()) {
            let fwd = V3::from_yaw_pitch(c.view_yaw, c.view_pitch);
            let right = V3::right_of_yaw(c.view_yaw);
            let mut e = eye;
            if self.shake > 0.0 {
                let s = self.shake * 0.05;
                e += V3::UP * ((t * 41.0).sin() * s) + right * ((t * 37.0).cos() * s);
            }
            let fov = lerp(self.settings.fov, p.gun().ads_fov, p.ads);
            return Cam { eye: e, fwd, up: V3::UP, fov };
        }
        // Muerto: en Buscar y destruir se sigue a un compañero; si no, cámara de muerte
        if let Some(s) = snap {
            let sd_wait = s.mode == Mode::Sd && (s.phase == Phase::Playing || s.phase == Phase::RoundEnd);
            let watch = self.my_death.map(|d| d.2 > 2.5).unwrap_or(true);
            if sd_wait && watch {
                let mates: Vec<PlayerInfo> = c.remotes().into_iter().filter(|p| p.alive && p.team == s.my_team).collect();
                if !mates.is_empty() {
                    let m = &mates[self.spectate % mates.len()];
                    let head = m.pos + V3::UP * STAND_EYE;
                    let back = -V3::from_yaw(m.yaw);
                    let dir = (back + V3::UP * 0.25).norm();
                    let dist = self.world.raycast(head, dir, 3.2).map(|h| (h.t - 0.3).max(0.3)).unwrap_or(3.2);
                    let eye = head + dir * dist;
                    return Cam { eye, fwd: (head + V3::from_yaw(m.yaw) * 4.0 - eye).norm(), up: V3::UP, fov: self.settings.fov };
                }
            }
        }
        match self.my_death {
            Some((pos, yaw, dt)) => {
                // Muerte cinematográfica: caída con giro, luego la cámara se
                // eleva sobre el cuerpo y se vuelve hacia quien te ha abatido
                let k = (dt / 0.9).min(1.0);
                let k = k * k * (3.0 - 2.0 * k);
                let fall_eye = pos + V3::UP * lerp(STAND_EYE, 0.35, k);
                let roll = 0.55 * k;
                let right = V3::right_of_yaw(yaw);
                if dt < 1.3 {
                    return Cam { eye: fall_eye, fwd: V3::from_yaw_pitch(yaw, -0.15 * k), up: V3::UP * roll.cos() + right * roll.sin(), fov: self.settings.fov };
                }
                let body = pos + V3::UP * 0.3;
                let killer = self.killer_id.and_then(|id| c.remotes().into_iter().find(|p| p.id == id && p.alive)).map(|p| p.pos + V3::UP * 1.3);
                let u = ((dt - 1.3) / 2.5).min(1.0);
                let u = u * u * (3.0 - 2.0 * u);
                // Órbita lenta alrededor del cuerpo, apartándose de las paredes
                let a = yaw + std::f32::consts::PI + (dt - 1.3) * 0.25;
                let dir = (V3::from_yaw(a) * 1.0 + V3::UP * lerp(0.4, 0.9, u)).norm();
                let dist = self.world.raycast(body, dir, lerp(1.5, 5.5, u)).map(|h| (h.t - 0.4).max(0.6)).unwrap_or(lerp(1.5, 5.5, u));
                let eye = body + dir * dist;
                let look = match killer {
                    Some(kp) if dt > 2.8 => body.lerp(kp, ((dt - 2.8) / 1.5).min(1.0) * 0.6),
                    _ => body,
                };
                let roll = 0.55 * (1.0 - u);
                let r = V3::right_of_yaw(a);
                return Cam { eye, fwd: (look - eye).norm(), up: V3::UP * roll.cos() + r * roll.sin(), fov: lerp(self.settings.fov, 55.0, u) };
            }
            None => {
                let base = c.me_info().map(|p| p.pos).unwrap_or(v3(0.0, 0.0, 0.0));
                let eye = base + v3(0.0, 30.0, -25.0);
                Cam { eye, fwd: (base - eye).norm(), up: V3::UP, fov: 60.0 }
            }
        }
    }

    fn render_game(&mut self, cam: &Cam) {
        self.r.begin(cam, self.clock);
        let c = self.client.as_ref().unwrap();
        let snap = c.snap.as_ref();
        let mut dg = std::mem::take(&mut self.dg);
        let mut pg = std::mem::take(&mut self.pg);
        dg.clear();
        pg.clear();

        if let Some(s) = snap {
            match s.mode {
                Mode::Ctf => {
                    for t in 0..2 {
                        let f = s.flags[t];
                        geo::flag(&mut dg, self.world.flag_bases[t], t as u8, self.clock, true);
                        if f.state == flag_state::DROPPED {
                            geo::flag(&mut dg, f.pos, t as u8, self.clock, false);
                        }
                    }
                }
                Mode::Hq if s.hq_state != hq_state::NONE => {
                    let owner = if s.hq_state == hq_state::HELD { Some(s.hq_owner) } else { None };
                    geo::hq_radio(&mut dg, s.hq_pos, owner, self.clock);
                }
                Mode::Sd => {
                    for site in self.world.bomb_sites.iter() {
                        geo::bomb_site(&mut dg, *site);
                    }
                    if s.bomb_state == bomb_state::PLANTED {
                        geo::planted_bomb(&mut dg, self.world.bomb_sites[s.bomb_site as usize % 2], self.clock);
                    }
                }
                _ => {}
            }
        }

        let flag_of = |id: u8| -> Option<geo::Col> {
            let s = snap?;
            (0..2).find(|&t| s.flags[t].state == flag_state::CARRIED && s.flags[t].carrier == id).map(|t| geo::FLAG_COLORS[t])
        };
        let right = cam.fwd.cross(cam.up).norm();
        let up = right.cross(cam.fwd).norm();
        for p in c.remotes().iter().filter(|p| p.alive) {
            let look = SoldierLook {
                side: p.side,
                nation: p.nation,
                gun: p.gun,
                smg: matches!(gun(p.gun).kind, GunKind::Smg | GunKind::Support),
                crouch: p.crouch,
                pitch: p.pitch,
                flash: 0.0,
                has_flag: if p.has_flag { flag_of(p.id) } else { None },
            };
            let walk = self.walk.get(&p.id).map(|w| w.1).unwrap_or(0.0);
            dg.sun = sun_vis(&self.world, p.pos + V3::UP * 1.2);
            let pose = arte::SoldierPose {
                pos: p.pos,
                yaw: p.yaw,
                pitch: p.pitch,
                moving: p.moving,
                sprint: p.sprint,
                crouch: p.crouch,
                time: self.clock + p.id as f32 * 0.37,
                dead: None,
                sun: dg.sun,
            };
            let art_ok = p.nation != nation::ZOMBIE && self.r.art.draw_soldier(p.side, gun(p.gun).kind == GunKind::Smg, &pose);
            if !art_ok {
                geo::soldier(&mut dg, p.pos, p.yaw, None, walk, p.moving, &look);
            }
            pg.blob_shadow(p.pos, 0.55, 0.35);
            if self.flash.get(&p.id).copied().unwrap_or(-1.0) > 0.0 {
                let f = V3::from_yaw_pitch(p.yaw, p.pitch);
                let m = p.pos + V3::UP * (lerp(1.3, 0.85, p.crouch)) + f * 1.05 + V3::right_of_yaw(p.yaw) * 0.08;
                pg.set(geo::m::GLOW);
                pg.billboard(m, right, up, 0.12, [1.0, 0.86, 0.5, 0.95], 0.0);
            }
        }
        for k in &self.corpses {
            let look = SoldierLook { side: k.side, nation: k.nation, gun: k.gun, smg: false, crouch: 0.0, pitch: 0.0, flash: 0.0, has_flag: None };
            dg.sun = sun_vis(&self.world, k.pos + V3::UP * 0.4);
            let pose = arte::SoldierPose { pos: k.pos, yaw: k.yaw, pitch: 0.0, moving: 0.0, sprint: false, crouch: 0.0, time: 0.0, dead: Some(k.t), sun: dg.sun };
            if k.nation == nation::ZOMBIE || !self.r.art.draw_soldier(k.side, false, &pose) {
                geo::soldier(&mut dg, k.pos, k.yaw, Some(k.t), 0.0, 0.0, &look);
            }
        }
        if self.demo_step == 8 {
            // Fila de modelos para revisar el diseño de los soldados
            let poses: [(u8, bool, f32, f32, f32); 5] = [(0, false, 0.0, 0.0, 0.0), (0, false, 0.0, 1.0, 1.2), (1, true, 0.0, 0.0, 0.0), (1, false, 1.0, 0.0, 0.0), (0, true, 0.0, 0.0, -0.4)];
            for (k, (side, smg, crouch, moving, pitch)) in poses.iter().enumerate() {
                let pos = v3(-24.0 + k as f32 * 1.5, 0.0, 4.5);
                let nat = [nation::US, nation::UK, nation::DE, nation::RU, nation::ZOMBIE][k];
                let g = if *smg { deia::sim::nation_gun(nat.min(3), GunKind::Smg) } else { deia::sim::nation_gun(nat.min(3), GunKind::Rifle) };
                let look = SoldierLook { side: *side, nation: nat, gun: g, smg: *smg, crouch: *crouch, pitch: *pitch, flash: 0.0, has_flag: None };
                dg.sun = sun_vis(&self.world, pos + V3::UP * 1.2);
                let yaw = std::f32::consts::FRAC_PI_2 - 0.35 + k as f32 * 0.15;
                let pose = arte::SoldierPose { pos, yaw, pitch: *pitch, moving: *moving, sprint: false, crouch: *crouch, time: self.clock + k as f32, dead: None, sun: dg.sun };
                if !self.r.art.draw_soldier(*side, *smg, &pose) {
                    geo::soldier(&mut dg, pos, yaw, None, 0.9, *moving, &look);
                }
                pg.blob_shadow(pos, 0.55, 0.35);
            }
        }
        // Ametralladoras montadas (la propia se ve en primera persona)
        if let Some(s) = snap {
            let my_turret = c.pred.as_ref().map(|p| p.mounted).unwrap_or(NO_TURRET);
            for (k, t) in self.world.turrets.iter().enumerate() {
                if k as u8 == my_turret {
                    continue;
                }
                let user = c.remotes().into_iter().find(|p| p.alive && p.mounted as usize == k);
                let (yaw, pitch) = user.map(|p| (p.yaw, p.pitch)).unwrap_or((t.yaw, -0.05));
                let nat = if t.side == 1 { nation::DE } else { s.allied_nation };
                dg.sun = sun_vis(&self.world, t.pos + V3::UP * 0.3);
                geo::turret(&mut dg, t.pos, yaw, pitch, mounted_gun(nat));
            }
            // Humo: bolas grises que crecen y se deshacen
            for (k, (sp, age)) in s.smokes.iter().enumerate() {
                let fade = (age / 1.5).min(1.0) * ((deia::server::SMOKE_TIME - age) / 5.0).clamp(0.0, 1.0);
                let grow = (age / 4.0).min(1.0);
                let mut r = Rng::new(k as u64 * 31 + 7 + (sp.x * 10.0) as u64);
                for _ in 0..22 {
                    let a = r.range(0.0, std::f32::consts::TAU);
                    let d = r.range(0.0, deia::server::SMOKE_RADIUS * 0.75) * grow;
                    let h = r.range(0.3, 3.2) * (0.4 + 0.6 * grow);
                    let drift = V3::from_yaw(0.6) * (age * 0.08);
                    let p = *sp + v3(a.cos() * d, h, a.sin() * d) + drift;
                    let size = r.range(1.6, 2.6) * (0.5 + 0.5 * grow);
                    let shade = r.range(0.72, 0.86);
                    pg.set(geo::m::SOFT);
                    pg.billboard(p, right, up, size, [shade, shade, shade * 0.98, 0.55 * fade], 0.0);
                }
            }
        }
        dg.sun = 1.0;
        for g in c.grenades() {
            geo::grenade_of(&mut dg, g.pos, self.clock * 8.0 + g.id as f32, g.nation, g.smoke);
        }
        self.r.opaque(&dg);

        // Arma en primera persona (encima de todo lo demás)
        let override_cam = self.demo_camera(self.demo_step).is_some();
        if let (Some(p), false) = (c.pred.as_ref(), override_cam) {
            dg.clear();
            dg.sun = sun_vis(&self.world, cam.eye);
            let right = V3::right_of_yaw(c.view_yaw);
            let up = right.cross(cam.fwd).norm();
            let vp = ViewParams {
                gun: p.gun_id(),
                nation: p.nation,
                melee: if p.melee_t > 0.0 { 1.0 - p.melee_t / 0.75 } else { 0.0 },
                ads: p.ads,
                bob: p.bob,
                moving: p.moving,
                recoil: c.recoil,
                lowered: p.lowered(),
                sprint: p.sprinting,
                muzzle: c.muzzle,
                flash_seed: self.clock,
            };
            let gi = p.gun_id() as usize;
            let scoped_in = p.gun().scoped() && p.ads > 0.85;
            if scoped_in {
                // Con la mira telescópica puesta no se ve el arma
            } else if gi < 4 && self.r.art.has_view(gi) && p.melee_t <= 0.0 {
                let pose = arte::ViewPose {
                    eye: cam.eye,
                    fwd: cam.fwd,
                    right,
                    up,
                    ads: p.ads,
                    bob: p.bob,
                    moving: p.moving,
                    recoil: c.recoil,
                    lowered: p.lowered(),
                    sprint: p.sprinting,
                    sun: dg.sun,
                    time: self.clock,
                };
                if let Some(tip) = self.r.art.draw_view(gi, &pose) {
                    if c.muzzle > 0.0 {
                        pg.set(geo::m::GLOW);
                        pg.billboard(tip + cam.fwd * 0.03, right, up, 0.07, [1.0, 0.86, 0.5, 0.95], self.clock * 7.0);
                    }
                }
            } else {
                geo::viewmodel(&mut dg, cam.eye, cam.fwd, right, up, &vp);
                self.r.opaque(&dg);
            }
        }

        // Humo, polvo, fogonazos y sombras de contacto
        for f in &self.fx {
            let a = f.alpha * (f.ttl / f.max).clamp(0.0, 1.0);
            let glow = f.color[0] > 0.9 && f.color[2] < 0.4;
            pg.set(if glow { geo::m::GLOW } else { geo::m::SOFT });
            pg.billboard(f.pos, right, up, f.size * 0.7, [f.color[0], f.color[1], f.color[2], a], 0.0);
        }
        self.r.transparent(&pg);
        self.r.end();
        for (a, b, t) in &self.tracers {
            draw_line_3d(tv(*a), tv(*b), Color::new(1.0, 0.9, 0.6, 0.8 * (t / 0.05).clamp(0.0, 1.0)));
        }
        self.dg = dg;
        self.pg = pg;
    }

    // ------------------------------------------------------------------
    // HUD
    // ------------------------------------------------------------------

    fn hud(&mut self, cam: &Cam) {
        let c = self.client.as_ref().unwrap();
        let Some(s) = c.snap.as_ref() else {
            txt_c("Recibiendo datos del servidor…", screen_width() / 2.0, screen_height() / 2.0, 28.0, CREAM);
            return;
        };
        let (sw, sh) = (screen_width(), screen_height());
        let (cx, cy) = (sw / 2.0, sh / 2.0);
        let teams = s.mode.teams();
        let me = c.my_id;

        // Sin barra de salud: el daño se ve en la pantalla (sangre en los
        // bordes, que late, y la imagen se emborrona y pierde color)
        if s.my_alive {
            let hurt = (1.0 - s.health / MAX_HEALTH).clamp(0.0, 1.0);
            self.r.hurt = hurt;
            if hurt > 0.01 {
                let pulse = if hurt > 0.6 { 0.85 + 0.15 * (self.clock * 7.0).sin() } else { 1.0 };
                draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.45, 0.0, 0.0, hurt * 0.14 * pulse));
                for k in 0..12 {
                    let inset = k as f32 * sh * 0.016;
                    let th = sh * 0.016;
                    let col = Color::new(0.5, 0.0, 0.0, hurt * 0.55 * pulse * (1.0 - k as f32 / 12.0).powf(1.5));
                    draw_rectangle(inset, inset, sw - 2.0 * inset, th, col);
                    draw_rectangle(inset, sh - inset - th, sw - 2.0 * inset, th, col);
                    draw_rectangle(inset, inset + th, th, sh - 2.0 * inset - 2.0 * th, col);
                    draw_rectangle(sw - inset - th, inset + th, th, sh - 2.0 * inset - 2.0 * th, col);
                }
                // Salpicaduras en las esquinas
                if hurt > 0.3 {
                    let a = (hurt - 0.3) / 0.7;
                    let mut r = Rng::new(7);
                    for _ in 0..18 {
                        let corner = r.index(4);
                        let (cx0, cy0) = [(0.0, 0.0), (sw, 0.0), (0.0, sh), (sw, sh)][corner];
                        let ang = r.range(0.0, std::f32::consts::TAU);
                        let d = r.range(0.0, sh * 0.25);
                        let rad = r.range(sh * 0.02, sh * 0.07) * (0.6 + a);
                        draw_circle(cx0 + ang.cos() * d, cy0 + ang.sin() * d, rad, Color::new(0.42, 0.02, 0.02, 0.55 * a * pulse));
                    }
                }
            }
        } else {
            self.r.hurt = 0.0;
        }

        // Dentro de una nube de humo no se ve casi nada
        let inside = s.smokes.iter().map(|(p, age)| {
            let d = (*p + V3::UP * 1.5).dist(cam.eye);
            let k = (1.0 - d / deia::server::SMOKE_RADIUS).clamp(0.0, 1.0) * (age / 2.0).min(1.0) * ((deia::server::SMOKE_TIME - age) / 5.0).clamp(0.0, 1.0);
            k
        }).fold(0.0f32, f32::max);
        if inside > 0.0 {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.78, 0.78, 0.76, (inside * 1.4).min(0.92)));
        }

        // Nombres sobre las cabezas
        let remotes = c.remotes();
        for p in remotes.iter().filter(|p| p.alive) {
            let head = p.pos + V3::UP * (lerp(2.05, 1.5, p.crouch));
            let mate = teams && p.team == s.my_team;
            let Some(sp) = project(cam, head) else { continue };
            let dist = head.dist(cam.eye);
            if mate && dist < 80.0 {
                txt_c(&p.name, sp.x, sp.y, 18.0, with_alpha(ALLIED_C, 0.9));
            } else if !mate && (sp - vec2(cx, cy)).length() < 40.0 && self.world.los(cam.eye, head - V3::UP * 0.3) {
                txt_c(&p.name, sp.x, sp.y, 18.0, AXIS_C);
            }
        }
        // Objetivos en pantalla: rombo con la distancia, como en la brújula
        let my_pos = c.pred.as_ref().map(|p| p.pos).unwrap_or(cam.eye);
        let marker = |at: V3, label: &str, col: Color| {
            if let Some(sp) = project(cam, at) {
                diamond(sp.x, sp.y, 9.0, col);
                let d = format!("{:.0} m", at.dist_flat(my_pos));
                txt_c(&d, sp.x, sp.y - 16.0, 15.0, with_alpha(CREAM, 0.9));
                if !label.is_empty() {
                    txt_c(label, sp.x, sp.y + 30.0, 16.0, col);
                }
            }
        };
        if s.mode == Mode::Sd {
            for (k, site) in self.world.bomb_sites.iter().enumerate() {
                let planted = s.bomb_state == bomb_state::PLANTED && s.bomb_site as usize == k;
                let col = if planted { Color::new(1.0, 0.3, 0.2, 1.0) } else { GOLD };
                marker(*site + V3::UP * 2.6, if k == 0 { "A" } else { "B" }, col);
            }
        }
        if s.mode == Mode::Ctf {
            for t in 0..2 {
                let f = s.flags[t];
                if f.state == flag_state::CARRIED && f.carrier == me {
                    continue;
                }
                let label = if t as u8 == s.my_team { "Tu bandera" } else { "Bandera enemiga" };
                marker(f.pos + V3::UP * 3.2, label, team_color(t as u8));
            }
            // Si llevo la bandera, el objetivo es mi base
            if s.flags.iter().any(|f| f.state == flag_state::CARRIED && f.carrier == me) {
                marker(self.world.flag_bases[s.my_team as usize % 2] + V3::UP * 3.2, "Tu base", GOLD);
            }
        }

        if let Some(p) = c.pred.as_ref() {
            // Retícula
            if p.ads < 0.4 && !p.sprinting && !p.busy {
                let gap = 5.0 + p.spread() / (cam.fov.to_radians() * 0.5).tan() * sh * 0.5;
                let len = 9.0;
                let col = Color::new(1.0, 1.0, 1.0, 0.85 * (1.0 - p.ads * 2.0).max(0.0));
                draw_line(cx - gap - len, cy, cx - gap, cy, 2.0, col);
                draw_line(cx + gap, cy, cx + gap + len, cy, 2.0, col);
                draw_line(cx, cy - gap - len, cx, cy - gap, 2.0, col);
                draw_line(cx, cy + gap, cx, cy + gap + len, 2.0, col);
            }
            if self.hitmarker > 0.0 {
                let col = Color::new(1.0, 1.0, 1.0, self.hitmarker / 0.2);
                for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    draw_line(cx + sx * 6.0, cy + sy * 6.0, cx + sx * 14.0, cy + sy * 14.0, 2.0, col);
                }
            }
            // Indicadores de daño
            let f = V3::from_yaw(c.view_yaw);
            let r = V3::right_of_yaw(c.view_yaw);
            let ang_to = |from: V3| {
                let d = (from - p.pos).flat();
                d.dot(r).atan2(d.dot(f))
            };
            for (from, ttl) in &self.indicators {
                let ang = ang_to(*from);
                let d = vec2(ang.sin(), -ang.cos());
                let perp = vec2(ang.cos(), ang.sin());
                let base = vec2(cx, cy) + d * (sh * 0.17);
                draw_triangle(base + perp * 30.0, base - perp * 30.0, vec2(cx, cy) + d * (sh * 0.17 + 28.0), Color::new(0.85, 0.05, 0.05, 0.75 * (ttl / 1.6).min(1.0)));
            }
            // Granadas cercanas (también las propias: también hacen daño)
            let mut warn = false;
            for g in c.grenades() {
                if g.pos.dist(p.pos) < 8.0 {
                    warn = true;
                    let ang = ang_to(g.pos);
                    let dir = vec2(ang.sin(), -ang.cos());
                    let at = vec2(cx, cy) + dir * (sh * 0.12);
                    draw_circle(at.x, at.y, 15.0, Color::new(0.1, 0.1, 0.1, 0.75));
                    draw_circle_lines(at.x, at.y, 15.0, 2.0, GOLD);
                    txt_c("!", at.x, at.y + 8.0, 26.0, GOLD);
                }
            }
            if warn && (self.clock * 6.0).sin() > -0.3 {
                txt_c("¡GRANADA!", cx, sh * 0.3, 34.0, GOLD);
            }
            if p.reload_t > 0.0 {
                let prog = 1.0 - p.reload_t / p.reload_total;
                txt_c("Recargando", cx, cy + 58.0, 20.0, CREAM);
                draw_rectangle(cx - 70.0, cy + 66.0, 140.0, 5.0, Color::new(0.0, 0.0, 0.0, 0.5));
                draw_rectangle(cx - 70.0, cy + 66.0, 140.0 * prog, 5.0, CREAM);
            }
            // Munición (abajo a la derecha): cargador | reserva | granadas
            let def = p.gun();
            let a = p.ammo[p.slot as usize];
            if p.is_mounted() {
                // Calor del cañón en lugar de munición
                let w = 200.0;
                let (x, y) = (sw - 24.0 - w, sh - 34.0);
                txt_r(def.name, sw - 24.0, y - 18.0, 18.0, CREAM);
                draw_rectangle(x, y, w, 10.0, Color::new(0.0, 0.0, 0.0, 0.5));
                let col = if p.overheated { Color::new(1.0, 0.3, 0.2, 1.0) } else { Color::new(1.0, lerp(0.85, 0.35, p.heat), 0.3, 1.0) };
                draw_rectangle(x, y, w * p.heat, 10.0, col);
                if p.overheated {
                    txt_c("¡Cañón recalentado!", cx, cy + 70.0, 22.0, col);
                }
                txt_c("F o salto: soltar la ametralladora", cx, sh - 40.0, 16.0, DIM);
            } else {
                hud_ammo(sw, sh, a.mag, a.reserve, def.mag, p.grenades, matches!(def.kind, GunKind::Smg | GunKind::Pistol), def.name);
                if p.smokes > 0 {
                    txt_r(&format!("Humo ×{} (H)", p.smokes), sw - 24.0, sh - 112.0, 15.0, DIM);
                }
                // Nido de ametralladora libre cerca
                let near_turret = self.world.turrets.iter().enumerate().find(|(k, t)| {
                    t.stand.dist_flat(p.pos) < MOUNT_RANGE && !c.remotes().iter().any(|o| o.alive && o.mounted as usize == *k)
                });
                if let Some((_, t)) = near_turret {
                    let nat = if t.side == 1 { nation::DE } else { s.allied_nation };
                    txt_c(&format!("Pulsa F para usar la {}", gun(mounted_gun(nat)).name), cx, cy + 150.0, 22.0, GOLD);
                }
            }
            if a.mag == 0 && a.reserve == 0 {
                txt_c("Sin munición: cambia de arma (Q)", cx, cy + 90.0, 20.0, Color::new(1.0, 0.6, 0.4, 1.0));
            }
            // Pista de objetivo
            let mut hint: Option<String> = None;
            if s.mode == Mode::Sd && s.phase == Phase::Playing {
                let at_site = |k: usize| self.world.bomb_sites[k].dist_flat(p.pos) < SITE_RADIUS;
                if s.bomb_state == bomb_state::NONE && s.my_team == s.attackers && (at_site(0) || at_site(1)) {
                    hint = Some("Mantén F para colocar la bomba".into());
                }
                if s.bomb_state == bomb_state::PLANTED && s.my_team != s.attackers && at_site(s.bomb_site as usize % 2) {
                    hint = Some("Mantén F para desactivar la bomba".into());
                }
            }
            if s.mode == Mode::Ctf && s.flags.iter().any(|f| f.state == flag_state::CARRIED && f.carrier == me) {
                hint = Some("¡Llevas la bandera! Vuelve a tu base".into());
            }
            if s.progress > 0.0 {
                let w = 260.0;
                draw_rectangle(cx - w / 2.0, cy + 100.0, w, 12.0, Color::new(0.0, 0.0, 0.0, 0.6));
                draw_rectangle(cx - w / 2.0, cy + 100.0, w * s.progress.min(1.0), 12.0, GOLD);
            }
            if let Some(h) = hint {
                txt_c(&h, cx, cy + 140.0, 24.0, GOLD);
            }
            // Granada a los pies: se puede devolver
            if c.grenades().iter().any(|g| g.pos.dist_flat(p.pos) < 1.7 && (g.pos.y - p.pos.y).abs() < 1.6) {
                txt_c("¡Pulsa F para devolver la granada!", cx, cy + 120.0, 26.0, GOLD);
            }
            // Mira telescópica
            if p.gun().scoped() && p.ads > 0.85 {
                scope_overlay(sw, sh);
            }
            // Cuartel General: progreso dentro
            if s.mode == Mode::Hq && s.hq_state != hq_state::NONE && p.pos.dist_flat(s.hq_pos) < deia::server::HQ_RADIUS && s.hq_progress > 0.0 {
                let label = if s.hq_state == hq_state::NEUTRAL { "Tomando el cuartel general…" } else if s.hq_owner == s.my_team { "¡Están destruyendo vuestro cuartel!" } else { "Destruyendo el cuartel general…" };
                let w = 260.0;
                txt_c(label, cx, cy + 90.0, 20.0, CREAM);
                draw_rectangle(cx - w / 2.0, cy + 100.0, w, 12.0, Color::new(0.0, 0.0, 0.0, 0.6));
                draw_rectangle(cx - w / 2.0, cy + 100.0, w * s.hq_progress.min(1.0), 12.0, GOLD);
            }
        } else if s.phase != Phase::Intermission {
            // Muerto
            let line = if s.mode == Mode::Zombies && s.phase == Phase::Playing {
                "Has caído: volverás con la próxima oleada si tus compañeros resisten".to_string()
            } else if s.mode == Mode::Sd && s.phase == Phase::Playing {
                "Estás fuera de combate hasta la próxima ronda · Clic: cambiar de cámara".to_string()
            } else if s.respawn_in > 0.0 {
                format!("Reaparecerás en {:.0}", s.respawn_in.ceil())
            } else {
                "Esperando…".to_string()
            };
            txt_c(&line, cx, sh * 0.72, 26.0, CREAM);
            if let Some(k) = &self.killed_by {
                txt_c(&format!("Te ha abatido {k}"), cx, sh * 0.72 + 32.0, 22.0, AXIS_C);
            }
        }

        // Experiencia ganada
        for (k, (t, ttl)) in self.xp_pops.iter().enumerate() {
            let a = (ttl / 0.5).min(1.0);
            txt_c(t, cx + 160.0, cy - 40.0 - k as f32 * 24.0 - (2.2 - ttl) * 10.0, 22.0, with_alpha(GOLD, a));
        }
        // Cuartel General en pantalla
        if s.mode == Mode::Hq && s.hq_state != hq_state::NONE {
            if let Some(sp) = project(cam, s.hq_pos + V3::UP * 3.5) {
                let col = if s.hq_state == hq_state::HELD { team_color(s.hq_owner) } else { CREAM };
                diamond(sp.x, sp.y, 10.0, col);
                let my_pos = c.pred.as_ref().map(|p| p.pos).unwrap_or(cam.eye);
                txt_c(&format!("{:.0} m", s.hq_pos.dist_flat(my_pos)), sp.x, sp.y - 16.0, 15.0, CREAM);
                txt_c("Cuartel", sp.x, sp.y + 30.0, 16.0, col);
            }
        }
        self.hud_top(s, me);
        self.hud_objective(c, s);
        self.hud_compass(c, s);

        // Muertes (arriba a la derecha)
        let mut y = 100.0;
        for f in &self.feed {
            let a = (f.ttl / 0.6).min(1.0);
            let w_name = match f.weapon {
                w if (w as usize) < GUNS.len() => GUNS[w as usize].name,
                weapon::GRENADE => "granada",
                weapon::BOMB => "bomba",
                weapon::MELEE => "zarpazo",
                weapon::BASH => "culatazo",
                _ => "",
            };
            let mid = if f.weapon == weapon::SUICIDE {
                " se ha suicidado".to_string()
            } else {
                format!(" [{}{}] ", w_name, if f.head { " · cabeza" } else { "" })
            };
            let kc = if teams { team_color(f.kteam) } else { CREAM };
            let vc = if teams { team_color(f.vteam) } else { CREAM };
            if f.weapon == weapon::SUICIDE || f.killer.is_empty() {
                let x = sw - 20.0 - text_w(&f.victim, 20.0) - text_w(&mid, 20.0);
                txt(&f.victim, x, y, 20.0, with_alpha(vc, a));
                txt(&mid, x + text_w(&f.victim, 20.0), y, 20.0, with_alpha(DIM, a));
            } else {
                let total = text_w(&f.killer, 20.0) + text_w(&mid, 20.0) + text_w(&f.victim, 20.0);
                let x = sw - 20.0 - total;
                txt(&f.killer, x, y, 20.0, with_alpha(kc, a));
                txt(&mid, x + text_w(&f.killer, 20.0), y, 20.0, with_alpha(DIM, a));
                txt(&f.victim, x + text_w(&f.killer, 20.0) + text_w(&mid, 20.0), y, 20.0, with_alpha(vc, a));
            }
            y += 24.0;
        }

        // Avisos centrales
        for (k, (text, tone_id, ttl)) in self.announces.iter().enumerate() {
            let a = (ttl / 0.6).min(1.0);
            let col = match *tone_id {
                tone::ALLIES => ALLIED_C,
                tone::AXIS => AXIS_C,
                tone::ALERT => GOLD,
                99 => CREAM,
                _ => DIM,
            };
            txt_c(text, cx, sh * 0.2 + k as f32 * 28.0, 22.0, with_alpha(col, a));
        }

        // Chat
        let base_y = sh - 290.0;
        let typing = self.chat.is_some();
        let lines: Vec<&(String, Color, f32)> = self.chat_log.iter().filter(|l| typing || l.2 > 0.0).collect();
        for (k, (text, col, ttl)) in lines.iter().rev().take(6).enumerate() {
            let a = if typing { 1.0 } else { (ttl / 1.0).min(1.0) };
            txt(text, 20.0, base_y - k as f32 * 22.0, 20.0, with_alpha(*col, a));
        }
        if let Some((text, team)) = &self.chat {
            let label = if *team { "Equipo: " } else { "Todos: " };
            draw_rectangle(16.0, base_y + 10.0, 560.0, 30.0, Color::new(0.0, 0.0, 0.0, 0.6));
            let caret = if (self.clock * 2.0) as i64 % 2 == 0 { "|" } else { "" };
            txt(&format!("{label}{text}{caret}"), 24.0, base_y + 32.0, 20.0, CREAM);
        }

        // Marcador
        if s.phase == Phase::Intermission || (is_key_down(KeyCode::Tab) && !typing) {
            self.scoreboard(s, me);
        }

        let ping = c.me_info().map(|p| p.ping).unwrap_or(0);
        txt_r(&format!("{} fps · {} ms", get_fps(), ping), sw - 10.0, 22.0, 16.0, Color::new(1.0, 1.0, 1.0, 0.45));
        if !typing && !self.pause && !self.team_menu && s.phase != Phase::Intermission {
            let help = if teams { "Tab: marcador · T/Y: chat · M: equipo · L: equipamiento · V: culatazo · H: humo · F: usar · Esc: menú" } else { "Tab: marcador · T: chat · L: equipamiento · Esc: menú" };
            txt_c(help, cx, sh - 14.0, 14.0, Color::new(1.0, 1.0, 1.0, 0.35));
        }
    }

    fn hud_top(&self, s: &Snapshot, me: u8) {
        let cx = screen_width() / 2.0;
        let t = s.timer.max(0.0) as u32;
        let clock = format!("{}:{:02}", t / 60, t % 60);
        match s.phase {
            Phase::Warmup => {
                let msg = if s.timer > 0.0 {
                    format!("La partida empieza en {:.0}", s.timer.ceil())
                } else if s.mode.teams() {
                    "Calentamiento: hace falta al menos un jugador en cada equipo".to_string()
                } else {
                    "Calentamiento: esperando a más jugadores".to_string()
                };
                txt_c(s.mode.name(), cx, 34.0, 24.0, CREAM);
                txt_c(&msg, cx, 62.0, 20.0, GOLD);
                return;
            }
            Phase::RoundEnd => txt_c(&format!("Siguiente ronda en {:.0}", s.timer.ceil()), cx, 92.0, 20.0, GOLD),
            _ => {}
        }
        if s.mode == Mode::Zombies {
            txt_c(&format!("Oleada {}", s.wave.max(1)), cx, 40.0, 32.0, Color::new(0.75, 0.95, 0.6, 1.0));
            let sub = if s.zombies_left == 0 && s.timer > 0.0 { format!("La siguiente oleada llega en {:.0} s", s.timer.ceil()) } else { format!("Quedan {} zombis", s.zombies_left) };
            txt_c(&sub, cx, 66.0, 18.0, DIM);
            return;
        }
        if s.mode.teams() {
            let left = format!("Aliados {}", s.team_scores[0]);
            let right = format!("{} Eje", s.team_scores[1]);
            txt_r(&left, cx - 50.0, 38.0, 30.0, ALLIED_C);
            txt(&right, cx + 50.0, 38.0, 30.0, AXIS_C);
            let mid = if s.mode == Mode::Sd && s.bomb_state == bomb_state::PLANTED {
                format!("{:.0}", s.bomb_timer.ceil())
            } else {
                clock
            };
            let col = if s.mode == Mode::Sd && s.bomb_state == bomb_state::PLANTED { Color::new(1.0, 0.3, 0.2, 1.0) } else { CREAM };
            txt_c(&mid, cx, 38.0, 26.0, col);
            let sub = match s.mode {
                Mode::Sd => format!(
                    "Ronda {} · atacan los {} · gana quien llegue a {}{}",
                    s.round,
                    team_name(s.attackers),
                    s.score_limit,
                    if s.bomb_state == bomb_state::PLANTED { " · ¡BOMBA COLOCADA!" } else { "" }
                ),
                Mode::Ctf => format!("Captura la bandera · {} capturas para ganar", s.score_limit),
                Mode::Hq => format!("Cuartel General · {} puntos para ganar", s.score_limit),
                _ => format!("Por equipos · {} bajas para ganar", s.score_limit),
            };
            txt_c(&sub, cx, 64.0, 18.0, DIM);
            let mine = format!("Tu equipo: {}", team_name(s.my_team));
            txt_c(&mine, cx, 84.0, 16.0, team_color(s.my_team));
        } else {
            let mut ps: Vec<&PlayerInfo> = s.players.iter().collect();
            ps.sort_by_key(|p| -(p.score as i32));
            let my = s.players.iter().find(|p| p.id == me).map(|p| p.score).unwrap_or(0);
            let lead = ps.first().map(|p| format!("Líder: {} ({})", p.name, p.score)).unwrap_or_default();
            txt_c(&format!("Tú: {my}   ·   {lead}   ·   {clock}"), cx, 38.0, 24.0, CREAM);
            txt_c(&format!("Todos contra todos · {} bajas para ganar", s.score_limit), cx, 62.0, 18.0, DIM);
        }
    }

    /// Panel «OBJETIVO ACTUAL» (arriba a la izquierda).
    fn hud_objective(&self, c: &NetClient, s: &Snapshot) {
        let me = c.my_id;
        let enemy = if s.my_team == 0 { "las tropas del Eje" } else { "las tropas aliadas" };
        let carrying = s.flags.iter().any(|f| f.state == flag_state::CARRIED && f.carrier == me);
        let text = match s.mode {
            Mode::Dm => format!("Abate a quien encuentres · {} bajas para ganar", s.score_limit),
            Mode::Hq => match s.hq_state {
                hq_state::NEUTRAL => "Toma el cuartel general".to_string(),
                hq_state::HELD if s.hq_owner == s.my_team => "Defiende el cuartel general".to_string(),
                hq_state::HELD => "Destruye el cuartel general enemigo".to_string(),
                _ => "Prepárate: va a aparecer un cuartel general".to_string(),
            },
            Mode::Zombies => format!("Sobrevive · oleada {} · quedan {} zombis", s.wave, s.zombies_left),
            Mode::Tdm => format!("Elimina a {enemy}"),
            Mode::Ctf => {
                let mine = s.flags[s.my_team as usize % 2];
                if carrying {
                    "Lleva la bandera enemiga a tu base".to_string()
                } else if mine.state != flag_state::HOME {
                    "Recupera tu bandera".to_string()
                } else {
                    "Captura la bandera enemiga".to_string()
                }
            }
            Mode::Sd => {
                let site = if s.bomb_site % 2 == 0 { "A" } else { "B" };
                let planted = s.bomb_state == bomb_state::PLANTED;
                match (s.my_team == s.attackers, planted) {
                    (true, false) => "Coloca la bomba en A o B".to_string(),
                    (true, true) => format!("Defiende la bomba en {site}"),
                    (false, false) => "Defiende los objetivos A y B".to_string(),
                    (false, true) => format!("Desactiva la bomba en {site}"),
                }
            }
        };
        let w = text_w(&text, 19.0).max(text_w("OBJETIVO ACTUAL", 14.0)) + 62.0;
        let (x, y) = (16.0, 14.0);
        round_rect(Rect::new(x, y, w, 58.0), 6.0, Color::new(0.08, 0.08, 0.07, 0.55));
        txt_k(FontKind::Bold, "OBJETIVO ACTUAL", x + 14.0, y + 20.0, 14.0, with_alpha(CREAM, 0.85));
        diamond(x + 24.0, y + 39.0, 7.5, GOLD);
        txt(&text, x + 40.0, y + 46.0, 19.0, CREAM);
    }

    /// Minimapa (abajo a la izquierda): gira con la vista; casas y muros,
    /// compañeros en azul, enemigos en rojo cuando disparan.
    fn hud_compass(&self, c: &NetClient, s: &Snapshot) {
        let sh = screen_height();
        let size = 170.0;
        let (x0, y0) = (18.0, sh - 52.0 - size);
        let (bx, by) = (x0 + size / 2.0, y0 + size / 2.0);
        let br = size / 2.0;
        round_rect(Rect::new(x0 - 3.0, y0 - 3.0, size + 6.0, size + 6.0), 8.0, Color::new(0.08, 0.08, 0.07, 0.6));
        round_rect(Rect::new(x0, y0, size, size), 6.0, Color::new(0.33, 0.36, 0.24, 0.75));
        let center = c
            .pred
            .as_ref()
            .map(|p| p.pos)
            .or_else(|| self.my_death.map(|d| d.0))
            .unwrap_or(V3::ZERO);
        let f = V3::from_yaw(c.view_yaw);
        let r = V3::right_of_yaw(c.view_yaw);
        let scale = br / 42.0;
        let raw = |d: V3| vec2(bx, by) + vec2(d.dot(r), -d.dot(f)) * scale;
        let to = |d: V3| {
            let v = vec2(d.dot(r), -d.dot(f)) * scale;
            let lim = br - 7.0;
            vec2(bx, by) + vec2(v.x.clamp(-lim, lim), v.y.clamp(-lim, lim))
        };
        // Planta del pueblo, recortada al recuadro
        let dpi = screen_dpi_scale();
        unsafe {
            get_internal_gl().quad_gl.scissor(Some(((x0 * dpi) as i32, (y0 * dpi) as i32, (size * dpi) as i32, (size * dpi) as i32)));
        }
        for b in &self.world.blocks {
            if b.b.max.y < 1.0 || b.b.center().dist_flat(center) > 70.0 {
                continue;
            }
            let (lo, hi) = (b.b.min - center, b.b.max - center);
            let p = [raw(v3(lo.x, 0.0, lo.z)), raw(v3(hi.x, 0.0, lo.z)), raw(v3(hi.x, 0.0, hi.z)), raw(v3(lo.x, 0.0, hi.z))];
            let col = Color::new(0.86, 0.8, 0.66, 0.85);
            draw_triangle(p[0], p[1], p[2], col);
            draw_triangle(p[0], p[2], p[3], col);
        }
        unsafe {
            get_internal_gl().quad_gl.scissor(None);
        }
        let n = to(V3::Z * 1000.0);
        txt_c("N", n.x, n.y + 6.0, 16.0, CREAM);
        let teams = s.mode.teams();
        for p in c.remotes().iter().filter(|p| p.alive) {
            let v = to(p.pos - center);
            if teams && p.team == s.my_team {
                draw_circle(v.x, v.y, 4.0, Color::new(0.35, 0.6, 1.0, 1.0));
                draw_circle_lines(v.x, v.y, 4.0, 1.0, Color::new(0.05, 0.1, 0.25, 0.8));
            } else if let Some(t) = self.fired.get(&p.id) {
                if *t < 2.5 {
                    draw_circle(v.x, v.y, 4.5, Color::new(0.95, 0.15, 0.1, 1.0 - t / 2.5));
                }
            }
        }
        match s.mode {
            Mode::Sd => {
                for (k, site) in self.world.bomb_sites.iter().enumerate() {
                    let v = to(*site - center);
                    diamond(v.x, v.y, 8.0, GOLD);
                    txt_c(if k == 0 { "A" } else { "B" }, v.x, v.y - 10.0, 15.0, GOLD);
                }
            }
            Mode::Ctf => {
                for t in 0..2 {
                    let v = to(s.flags[t].pos - center);
                    draw_rectangle(v.x - 5.0, v.y - 5.0, 10.0, 10.0, team_color(t as u8));
                }
            }
            Mode::Hq if s.hq_state != hq_state::NONE => {
                let v = to(s.hq_pos - center);
                let col = if s.hq_state == hq_state::HELD { team_color(s.hq_owner) } else { CREAM };
                diamond(v.x, v.y, 8.0, col);
            }
            Mode::Zombies => {
                // Los zombis cercanos se oyen: puntos verdes
                for p in c.remotes().iter().filter(|p| p.alive && p.nation == nation::ZOMBIE && p.pos.dist_flat(center) < 30.0) {
                    let v = to(p.pos - center);
                    draw_circle(v.x, v.y, 3.5, Color::new(0.5, 0.9, 0.3, 0.9));
                }
            }
            _ => {}
        }
        draw_triangle(vec2(bx, by - 9.0), vec2(bx - 6.5, by + 6.0), vec2(bx + 6.5, by + 6.0), WHITE);
    }

    fn scoreboard(&self, s: &Snapshot, me: u8) {
        let (sw, sh) = (screen_width(), screen_height());
        let w = 760.0f32.min(sw - 40.0);
        let x = sw / 2.0 - w / 2.0;
        let y0 = 120.0;
        let rows = s.players.len().max(1) as f32;
        panel(Rect::new(x - 10.0, y0 - 40.0, w + 20.0, ((rows + 4.0) * 28.0 + 60.0).min(sh - 140.0)), 0.85);
        if s.phase == Phase::Intermission {
            txt_c("FIN DE LA PARTIDA", sw / 2.0, y0 - 10.0, 30.0, GOLD);
        } else {
            txt_c(&format!("{} · {}", c_name(&self.client), s.mode.name()), sw / 2.0, y0 - 10.0, 22.0, CREAM);
        }
        let header = |y: f32| {
            txt("Jugador", x + 10.0, y, 18.0, DIM);
            txt_r("Puntos", x + w - 250.0, y, 18.0, DIM);
            txt_r("Bajas", x + w - 170.0, y, 18.0, DIM);
            txt_r("Muertes", x + w - 80.0, y, 18.0, DIM);
            txt_r("Ping", x + w - 10.0, y, 18.0, DIM);
        };
        let row = |p: &PlayerInfo, y: f32, col: Color| {
            if p.id == me {
                draw_rectangle(x, y - 19.0, w, 26.0, Color::new(1.0, 1.0, 1.0, 0.08));
            }
            let name = if p.alive || s.mode != Mode::Sd { p.name.clone() } else { format!("{} (caído)", p.name) };
            txt(&name, x + 10.0, y, 20.0, col);
            txt_r(&p.score.to_string(), x + w - 250.0, y, 20.0, CREAM);
            txt_r(&p.kills.to_string(), x + w - 170.0, y, 20.0, CREAM);
            txt_r(&p.deaths.to_string(), x + w - 80.0, y, 20.0, CREAM);
            txt_r(&p.ping.to_string(), x + w - 10.0, y, 20.0, DIM);
        };
        let mut y = y0 + 24.0;
        if s.mode.teams() {
            for t in [0u8, 1u8] {
                let mut ps: Vec<&PlayerInfo> = s.players.iter().filter(|p| p.team == t).collect();
                if ps.is_empty() && s.mode == Mode::Zombies {
                    continue;
                }
                ps.sort_by_key(|p| (-(p.score as i32), -(p.kills as i32)));
                txt(&format!("{} — {}", team_name(t), s.team_scores[t as usize]), x + 10.0, y, 24.0, team_color(t));
                y += 26.0;
                header(y);
                y += 26.0;
                for p in ps {
                    row(p, y, team_color(t));
                    y += 26.0;
                }
                y += 12.0;
            }
        } else {
            let mut ps: Vec<&PlayerInfo> = s.players.iter().collect();
            ps.sort_by_key(|p| (-(p.score as i32), -(p.kills as i32)));
            header(y);
            y += 26.0;
            for p in ps {
                row(p, y, CREAM);
                y += 26.0;
            }
        }
        if s.phase == Phase::Intermission {
            txt_c(&format!("Siguiente partida en {:.0} s", s.timer.ceil()), sw / 2.0, y + 20.0, 20.0, DIM);
        }
    }

    /// Clase, arma principal y ventaja; las bloqueadas muestran el nivel necesario.
    fn draw_loadout_menu(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.45));
        let w = 980.0f32.min(sw - 40.0);
        let x = sw / 2.0 - w / 2.0;
        let y = (sh / 2.0 - 300.0).max(20.0);
        panel(Rect::new(x - 10.0, y - 10.0, w + 20.0, 600.0), 0.92);
        // Como en CoD2: sin rangos ni desbloqueos, todo está disponible
        let level = 20u8;
        let snap = self.client.as_ref().and_then(|c| c.snap.as_ref());
        let my_nation = self.client.as_ref().and_then(|c| c.me_info()).map(|p| p.nation).filter(|&n| n != nation::ZOMBIE).unwrap_or_else(|| {
            snap.map(|s| if s.my_team == 1 && s.mode.teams() && s.mode != Mode::Zombies { nation::DE } else { s.allied_nation }).unwrap_or(nation::US)
        });
        title_c("Equipamiento", sw / 2.0, y + 34.0, 34.0, CREAM);
        txt_r(nation_name(my_nation), x + w - 10.0, y + 36.0, 18.0, DIM);

        let col_w = (w - 40.0) / 3.0;
        let cx0 = x;
        let cx1 = x + col_w + 20.0;
        let cx2 = x + 2.0 * (col_w + 20.0);
        let top = y + 80.0;
        txt("Clase", cx0, top, 22.0, CREAM);
        txt("Arma principal", cx1, top, 22.0, CREAM);
        txt("Ventaja", cx2, top, 22.0, CREAM);
        // Clases
        for (k, c) in CLASSES.iter().enumerate() {
            let r = Rect::new(cx0, top + 14.0 + k as f32 * 62.0, col_w, 54.0);
            let sel = self.settings.class == k as u8;
            round_rect(r, 6.0, if sel { Color::new(0.62, 0.5, 0.28, 0.95) } else { Color::new(0.12, 0.12, 0.1, 0.85) });
            txt(c.name, r.x + 12.0, r.y + 24.0, 21.0, CREAM);
            txt(&shorten(c.desc, 44), r.x + 12.0, r.y + 44.0, 14.0, DIM);
            if r.contains(mouse_position().into()) && is_mouse_button_pressed(MouseButton::Left) {
                self.settings.class = k as u8;
                self.settings.primary = 255;
            }
        }
        // Armas de la clase para tu nación
        let opts = Loadout::options(self.settings.class, my_nation);
        let chosen = if opts.contains(&self.settings.primary) { self.settings.primary } else { opts.iter().copied().find(|&g| gun(g).unlock <= level).unwrap_or(opts[0]) };
        for (k, &g) in opts.iter().enumerate() {
            let d = gun(g);
            let r = Rect::new(cx1, top + 14.0 + k as f32 * 62.0, col_w, 54.0);
            let locked = d.unlock > level;
            let sel = g == chosen && !locked;
            round_rect(r, 6.0, if sel { Color::new(0.62, 0.5, 0.28, 0.95) } else { Color::new(0.12, 0.12, 0.1, if locked { 0.5 } else { 0.85 }) });
            txt(d.name, r.x + 12.0, r.y + 24.0, 21.0, if locked { DIM } else { CREAM });
            let info = if locked {
                format!("Se desbloquea en el nivel {}", d.unlock)
            } else {
                format!("Cargador {} · {}", d.mag, if d.auto { "automática" } else { "tiro a tiro" })
            };
            txt(&info, r.x + 12.0, r.y + 44.0, 14.0, DIM);
            gun_silhouette(r.x + r.w - 10.0, r.y + 22.0, matches!(d.kind, GunKind::Smg), with_alpha(CREAM, if locked { 0.3 } else { 0.7 }));
            if !locked && r.contains(mouse_position().into()) && is_mouse_button_pressed(MouseButton::Left) {
                self.settings.primary = g;
            }
        }
        if !opts.contains(&self.settings.primary) || gun(self.settings.primary).unlock > level {
            self.settings.primary = chosen;
        }
        let pistols = Loadout::pistols(my_nation);
        let cur = pistols.iter().position(|&g| g == self.settings.secondary).unwrap_or(0);
        let py = top + 14.0 + opts.len().max(3) as f32 * 62.0 + 6.0;
        txt(&format!("Pistola: {}", gun(pistols[cur]).name), cx1, py + 20.0, 18.0, CREAM);
        if pistols.len() > 1 && small_button(Rect::new(cx1 + col_w - 110.0, py, 110.0, 30.0), "Cambiar") {
            self.settings.secondary = pistols[(cur + 1) % pistols.len()];
        }
        // Ventajas
        for k in 0..perk::COUNT {
            let (name, desc, lvl) = perk_info(k);
            let r = Rect::new(cx2, top + 14.0 + k as f32 * 52.0, col_w, 46.0);
            let locked = lvl > level;
            let sel = self.settings.perk == k;
            round_rect(r, 6.0, if sel { Color::new(0.62, 0.5, 0.28, 0.95) } else { Color::new(0.12, 0.12, 0.1, if locked { 0.5 } else { 0.85 }) });
            txt(name, r.x + 12.0, r.y + 20.0, 19.0, if locked { DIM } else { CREAM });
            let info = if locked { format!("Nivel {lvl}") } else { desc.to_string() };
            txt(&shorten(&info, 46), r.x + 12.0, r.y + 38.0, 14.0, DIM);
            if !locked && r.contains(mouse_position().into()) && is_mouse_button_pressed(MouseButton::Left) {
                self.settings.perk = k;
            }
        }
        if perk_info(self.settings.perk).2 > level {
            self.settings.perk = perk::NONE;
        }
        txt_c("El equipamiento nuevo se usa al reaparecer", sw / 2.0, y + 548.0, 16.0, DIM);
        let ok = button(Rect::new(sw / 2.0 - 120.0, y + 560.0 - 4.0, 240.0, 40.0), "Aceptar") || is_key_pressed(KeyCode::Enter);
        if ok {
            self.settings.save();
            self.loadout_menu = false;
            self.last_mouse = None;
            self.grab(true);
        }
    }

    fn draw_team_menu(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.4));
        let cx = sw / 2.0;
        let teams = self.client.as_ref().and_then(|c| c.snap.as_ref()).map(|s| s.mode.teams()).unwrap_or(false);
        panel(Rect::new(cx - 200.0, sh * 0.3 - 50.0, 400.0, 300.0), 0.9);
        txt_c("Elegir equipo", cx, sh * 0.3 - 10.0, 30.0, CREAM);
        if !teams {
            txt_c("En este modo no hay equipos", cx, sh * 0.3 + 40.0, 22.0, DIM);
        }
        let mut pick = None;
        if teams {
            if button(Rect::new(cx - 160.0, sh * 0.3 + 20.0, 320.0, 48.0), "Aliados") {
                pick = Some(1);
            }
            if button(Rect::new(cx - 160.0, sh * 0.3 + 80.0, 320.0, 48.0), "Eje") {
                pick = Some(2);
            }
        }
        let close = button(Rect::new(cx - 160.0, sh * 0.3 + 160.0, 320.0, 48.0), "Cerrar");
        if let Some(t) = pick {
            if let Some(c) = self.client.as_mut() {
                c.team_req = t;
            }
        }
        if pick.is_some() || close {
            self.team_menu = false;
            self.last_mouse = None;
            self.grab(true);
        }
    }

    fn draw_pause(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.45));
        let cx = sw / 2.0;
        panel(Rect::new(cx - 220.0, sh * 0.25 - 50.0, 440.0, 400.0), 0.9);
        txt_c("MENÚ", cx, sh * 0.25 - 10.0, 34.0, CREAM);
        let hosting = self.host.as_ref().map(|h| h.port);
        if let Some(p) = hosting {
            txt_c(&format!("Estás alojando la partida (puerto UDP {p})"), cx, sh * 0.25 + 18.0, 18.0, DIM);
        }
        let back = button(Rect::new(cx - 180.0, sh * 0.25 + 40.0, 360.0, 48.0), "Volver a la partida");
        let team = button(Rect::new(cx - 180.0, sh * 0.25 + 100.0, 175.0, 48.0), "Equipo");
        let kit = button(Rect::new(cx + 5.0, sh * 0.25 + 100.0, 175.0, 48.0), "Equipamiento");
        if kit {
            self.pause = false;
            self.loadout_menu = true;
        }
        let d = stepper(cx - 210.0, sh * 0.25 + 170.0, "  Sensibilidad", &format!("{:.1}", self.settings.sensitivity));
        if d != 0 {
            self.settings.sensitivity = (self.settings.sensitivity + d as f32 * 0.2).clamp(0.2, 10.0);
            self.settings.save();
        }
        let label = if hosting.is_some() { "Cerrar la partida" } else { "Desconectar" };
        let quit = button(Rect::new(cx - 180.0, sh * 0.25 + 240.0, 360.0, 48.0), label);
        if back {
            self.pause = false;
            self.last_mouse = None;
            self.grab(true);
        }
        if team {
            self.pause = false;
            self.team_menu = true;
        }
        if quit {
            self.leave(None);
        }
    }

    fn frame(&mut self) {
        let dt = get_frame_time().min(0.1);
        self.clock += dt;
        // Música en los menús y ambiente de batalla en partida
        let which = if self.screen == Screen::Game { 2 } else { 1 };
        let vol = self.settings.music * if which == 1 { 0.6 } else { 0.5 };
        self.sounds.background(if vol > 0.01 { which } else { 0 }, vol);
        match self.screen.clone() {
            Screen::Main => self.screen_main(),
            Screen::Join => self.screen_join(),
            Screen::Host => self.screen_host(),
            Screen::Options => self.screen_options(),
            Screen::Connecting => {
                let mut c = self.client.take().unwrap();
                c.update(&FrameInput::default(), dt, &self.world);
                let target = c.server.to_string();
                match c.status.clone() {
                    Status::Connected => {
                        self.client = Some(c);
                        self.screen = Screen::Game;
                        self.last_mouse = None;
                        self.grab(true);
                    }
                    Status::Failed(why) => {
                        self.client = Some(c);
                        self.leave(Some(why));
                    }
                    Status::Connecting => {
                        self.client = Some(c);
                        if self.screen_message("Conectando…", &format!("Contactando con {target}"), "Cancelar") {
                            self.leave(None);
                        }
                    }
                }
            }
            Screen::Game => {
                if is_mouse_button_pressed(MouseButton::Left) && !self.pause && !self.team_menu {
                    self.grab(true);
                }
                self.game_frame(dt);
            }
            Screen::Error(msg) => {
                if self.screen_message("Aviso", &msg, "Volver") {
                    self.screen = Screen::Main;
                }
            }
        }
    }
}

/// Lo que se desbloquea al llegar a un nivel.
fn unlocks_at(level: u8) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let guns: Vec<&str> = GUNS.iter().filter(|g| g.unlock == level && g.kind != GunKind::Melee).map(|g| g.name).collect();
    if !guns.is_empty() {
        v.push(format!("Desbloqueado: {}", guns.join(", ")));
    }
    for p in 0..perk::COUNT {
        let (name, _, lvl) = perk_info(p);
        if lvl == level && p != perk::NONE && lvl > 1 {
            v.push(format!("Nueva ventaja: {name}"));
        }
    }
    v
}

fn shorten(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

/// Mira telescópica: todo negro salvo el círculo, con retícula.
fn scope_overlay(sw: f32, sh: f32) {
    let (cx, cy) = (sw / 2.0, sh / 2.0);
    let r = sh * 0.44;
    let black = Color::new(0.02, 0.02, 0.02, 1.0);
    draw_rectangle(0.0, 0.0, cx - r, sh, black);
    draw_rectangle(cx + r, 0.0, sw - cx - r, sh, black);
    // Esquinas del cuadrado alrededor del círculo
    let steps = 48;
    for k in 0..steps {
        let a0 = k as f32 / steps as f32 * std::f32::consts::TAU;
        let a1 = (k + 1) as f32 / steps as f32 * std::f32::consts::TAU;
        let p0 = vec2(cx + a0.cos() * r, cy + a0.sin() * r);
        let p1 = vec2(cx + a1.cos() * r, cy + a1.sin() * r);
        let far = |a: f32| {
            let (c, s) = (a.cos(), a.sin());
            let k = r * 1.5 / c.abs().max(s.abs());
            vec2(cx + c * k, cy + s * k)
        };
        draw_triangle(p0, p1, far(a1), black);
        draw_triangle(p0, far(a1), far(a0), black);
    }
    draw_circle_lines(cx, cy, r, 3.0, black);
    let ink = Color::new(0.02, 0.02, 0.02, 0.9);
    draw_line(cx - r, cy, cx - 6.0, cy, 1.5, ink);
    draw_line(cx + 6.0, cy, cx + r, cy, 1.5, ink);
    draw_line(cx, cy + 6.0, cx, cy + r, 2.5, ink);
    draw_line(cx, cy - r, cx, cy - r * 0.35, 1.5, ink);
}

/// Rombo de objetivo (contorno oscuro y relleno de color).
fn diamond(x: f32, y: f32, r: f32, col: Color) {
    let ink = Color::new(0.1, 0.08, 0.06, 0.8);
    let (a, b, c, d) = (vec2(x, y - r - 2.0), vec2(x + r + 2.0, y), vec2(x, y + r + 2.0), vec2(x - r - 2.0, y));
    draw_triangle(a, b, c, ink);
    draw_triangle(a, c, d, ink);
    let (a, b, c, d) = (vec2(x, y - r), vec2(x + r, y), vec2(x, y + r), vec2(x - r, y));
    draw_triangle(a, b, c, col);
    draw_triangle(a, c, d, col);
    let k = r * 0.45;
    let (a, b, c, d) = (vec2(x, y - k), vec2(x + k, y), vec2(x, y + k), vec2(x - k, y));
    draw_triangle(a, b, c, ink);
    draw_triangle(a, c, d, ink);
}

/// Cruz y barra de salud (ya no se usa: el daño se muestra en pantalla).
#[allow(dead_code)]
fn hud_health(h: f32, sh: f32) {
    let h = h.clamp(0.0, 1.0);
    let (x, y) = (22.0, sh - 30.0);
    let col = if h < 0.3 { Color::new(1.0, 0.42, 0.35, 1.0) } else { WHITE };
    let shadow = Color::new(0.0, 0.0, 0.0, 0.45);
    for (dx, dy, c) in [(1.5, 1.5, shadow), (0.0, 0.0, col)] {
        draw_rectangle(x + dx + 6.0, y + dy - 9.0, 6.0, 18.0, c);
        draw_rectangle(x + dx, y + dy - 3.0, 18.0, 6.0, c);
    }
    let (bx, w) = (x + 30.0, 140.0);
    round_rect(Rect::new(bx, y - 4.5, w, 9.0), 4.5, Color::new(0.0, 0.0, 0.0, 0.45));
    if h > 0.0 {
        round_rect(Rect::new(bx, y - 4.5, (w * h).max(9.0), 9.0), 4.5, col);
    }
}

/// Silueta del arma: fusil largo o subfusil.
fn gun_silhouette(x: f32, y: f32, smg: bool, col: Color) {
    if smg {
        draw_rectangle(x - 62.0, y - 4.0, 40.0, 7.0, col); // cajón
        draw_rectangle(x - 22.0, y - 2.0, 22.0, 3.0, col); // cañón
        draw_rectangle(x - 50.0, y + 3.0, 5.0, 15.0, col); // cargador
        draw_triangle(vec2(x - 62.0, y - 3.0), vec2(x - 88.0, y + 2.0), vec2(x - 62.0, y + 6.0), col); // culata plegable
        draw_rectangle(x - 60.0, y + 3.0, 4.0, 8.0, col);
    } else {
        draw_rectangle(x - 76.0, y - 2.5, 76.0, 3.0, col); // cañón
        draw_rectangle(x - 96.0, y - 3.0, 52.0, 6.0, col); // madera
        draw_triangle(vec2(x - 96.0, y - 3.0), vec2(x - 120.0, y + 1.0), vec2(x - 120.0, y + 9.0), col);
        draw_triangle(vec2(x - 96.0, y - 3.0), vec2(x - 120.0, y + 9.0), vec2(x - 94.0, y + 3.0), col);
        draw_rectangle(x - 80.0, y - 6.0, 10.0, 3.0, col); // cerrojo
    }
}

fn hud_ammo(sw: f32, sh: f32, mag: u32, reserve: u32, mag_size: u32, grenades: u8, smg: bool, name: &str) {
    let y = sh - 24.0;
    let low = mag * 4 <= mag_size;
    let big_col = if low { Color::new(1.0, 0.45, 0.35, 1.0) } else { WHITE };
    let gn = grenades.to_string();
    let res = reserve.to_string();
    let big = mag.to_string();
    // De derecha a izquierda: granadas, reserva, cargador
    let mut x = sw - 24.0;
    let wg = text_w(&gn, 30.0);
    txt(&gn, x - wg, y, 30.0, WHITE);
    x -= wg + 8.0;
    // granada: cuerpo y espoleta
    draw_circle(x - 8.0, y - 10.0, 8.0, WHITE);
    draw_rectangle(x - 11.0, y - 23.0, 6.0, 5.0, WHITE);
    draw_line(x - 5.0, y - 21.0, x, y - 15.0, 2.0, WHITE);
    x -= 26.0;
    draw_line(x, y - 30.0, x, y + 2.0, 1.5, Color::new(1.0, 1.0, 1.0, 0.6));
    x -= 12.0;
    let wr = text_w(&res, 24.0);
    txt(&res, x - wr, y, 24.0, Color::new(1.0, 1.0, 1.0, 0.85));
    x -= wr + 12.0;
    draw_line(x, y - 30.0, x, y + 2.0, 1.5, Color::new(1.0, 1.0, 1.0, 0.6));
    x -= 12.0;
    let wb = text_w(&big, 46.0);
    txt(&big, x - wb, y + 2.0, 46.0, big_col);
    // Silueta y nombre del arma encima
    gun_silhouette(sw - 26.0, y - 56.0, smg, WHITE);
    txt_r(name, sw - 26.0, y - 72.0, 16.0, with_alpha(CREAM, 0.8));
}

fn c_name(c: &Option<NetClient>) -> String {
    c.as_ref().map(|c| c.server_name.clone()).unwrap_or_default()
}

// ----------------------------------------------------------------------
// Capturas automáticas (solo para desarrollo)
// ----------------------------------------------------------------------

/// Con la variable de entorno `DEIA_CAPTURA=carpeta`, el juego abre una
/// partida local con bots, coloca la cámara en varios sitios y guarda
/// capturas en esa carpeta. Sirve para revisar los gráficos sin jugar.
struct Demo {
    dir: String,
    step: usize,
    t: f32,
    stop: Arc<AtomicBool>,
}

fn demo_bot(addr: std::net::SocketAddr, name: String, stop: Arc<AtomicBool>, seed: u64) {
    let world = World::load();
    let Ok(mut c) = NetClient::connect_map(addr, &name, world.map_id) else { return };
    let mut rng = Rng::new(seed);
    let mut goal = world.waypoints[rng.index(world.waypoints.len())];
    let mut yaw_off = 0.0f32;
    while !stop.load(Ordering::Relaxed) {
        let mut inp = FrameInput::default();
        if let Some(p) = c.pred.as_ref() {
            if p.pos.dist_flat(goal) < 2.0 || rng.chance(0.002) {
                goal = world.waypoints[rng.index(world.waypoints.len())];
            }
            let next = world.nav_target(p.pos, goal);
            let want = (next - p.pos).yaw();
            inp.look_yaw = deia::math::wrap_angle(want - c.view_yaw) * 0.2;
            yaw_off += 0.01;
            inp.fwd = if rng.chance(0.97) { 1.0 } else { 0.0 };
            inp.crouch = (yaw_off * 0.3).sin() > 0.85;
        }
        c.update(&inp, 1.0 / 60.0, &world);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    c.disconnect();
}

impl App {
    /// Devuelve la cámara impuesta por la demo, si la hay.
    fn demo_camera(&self, step: usize) -> Option<Cam> {
        let look = |eye: V3, target: V3, fov: f32| Some(Cam { eye, fwd: (target - eye).norm(), up: V3::UP, fov });
        match step {
            2 => look(v3(-1.0, 1.7, -46.0), v3(0.5, 2.5, 0.0), 70.0),
            3 => look(v3(9.0, 1.8, 3.0), v3(32.0, 7.0, 24.0), 70.0),
            4 => {
                let c = self.client.as_ref()?;
                let p = c.remotes().into_iter().filter(|p| p.alive).min_by(|a, b| {
                    a.pos.dist(v3(0.0, 0.0, 0.0)).partial_cmp(&b.pos.dist(v3(0.0, 0.0, 0.0))).unwrap()
                })?;
                let f = V3::from_yaw(p.yaw);
                let eye = p.pos + f * 3.2 + V3::right_of_yaw(p.yaw) * 1.2 + V3::UP * 1.5;
                look(eye, p.pos + V3::UP * 1.1, 55.0)
            }
            6 => look(v3(-58.0, 7.5, 40.0), v3(-160.0, 14.0, 110.0), 70.0),
            7 => look(v3(-29.0, 1.9, -50.5), v3(-13.0, 3.5, -34.0), 70.0),
            8 => look(v3(-21.0, 1.25, 8.6), v3(-21.0, 0.95, 4.5), 60.0),
            9 => look(v3(10.5, 2.0, 4.6), v3(5.0, 1.2, -2.5), 60.0),
            10 => look(v3(22.0, 3.5, 17.0), v3(27.0, 3.8, 22.5), 60.0),
            12 => look(v3(-3.6, 1.9, -45.0), v3(-6.0, 1.0, -41.6), 55.0),
            _ => None,
        }
    }

    fn demo_frame(&mut self, demo: &mut Demo, dt: f32) -> bool {
        demo.t += dt;
        let shot = |name: &str, dir: &str| {
            let path = format!("{dir}/{name}.png");
            get_screen_data().export_png(&path);
            println!("captura: {path}");
        };
        match demo.step {
            0 => {
                if demo.t > 3.0 {
                    shot("menu", &demo.dir);
                    self.settings.host_port = 28977;
                    self.settings.host_modes = vec!["tdm".into()];
                    self.start_host();
                    if let Some(h) = &self.host {
                        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", h.port).parse().unwrap();
                        for k in 0..5 {
                            let stop = demo.stop.clone();
                            std::thread::spawn(move || demo_bot(addr, format!("Bot{k}"), stop, k as u64 + 7));
                        }
                    }
                    demo.step = 1;
                    demo.t = 0.0;
                }
            }
            1 => {
                if demo.t > 9.0 {
                    demo.step = 2;
                    demo.t = 0.0;
                }
            }
            2 | 3 | 4 | 6 | 7 | 8 | 9 | 10 => {
                if demo.t > 1.5 {
                    let name = ["", "", "pueblo", "iglesia", "soldados", "", "colinas", "casas", "modelos", "tanque", "detalle"][demo.step];
                    shot(name, &demo.dir);
                    demo.step = if demo.step == 4 { 5 } else { demo.step + 1 };
                    demo.t = 0.0;
                }
            }
            5 => {
                if demo.t > 1.5 {
                    shot("primera_persona", &demo.dir);
                    demo.step = 6;
                    demo.t = 0.0;
                }
            }
            11 => {
                self.loadout_menu = true;
                if demo.t > 1.0 {
                    shot("equipamiento", &demo.dir);
                    self.loadout_menu = false;
                    demo.step = 12;
                    demo.t = 0.0;
                }
            }
            12 => {
                if demo.t > 1.5 {
                    shot("ametralladora", &demo.dir);
                    demo.step = 13;
                    demo.t = 0.0;
                }
            }
            _ => {
                demo.stop.store(true, Ordering::Relaxed);
                self.leave(None);
                return false;
            }
        }
        true
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    load_fonts();
    // Pantalla de carga mientras se prepara el escenario
    for _ in 0..2 {
        clear_background(Color::new(0.80, 0.89, 0.95, 1.0));
        title_c("Eginbeharraren deia", screen_width() / 2.0, screen_height() / 2.0, 56.0, Color::new(0.24, 0.30, 0.36, 1.0));
        bold_c("Preparando Saint-Aubin…", screen_width() / 2.0, screen_height() / 2.0 + 44.0, 22.0, Color::new(0.30, 0.38, 0.44, 1.0));
        next_frame().await;
    }
    let mut demo = std::env::var("DEIA_CAPTURA").ok().map(|dir| {
        let _ = std::fs::create_dir_all(&dir);
        Demo { dir, step: 0, t: 0.0, stop: Arc::new(AtomicBool::new(false)) }
    });
    let mut settings = if demo.is_some() { Settings::default() } else { Settings::load() };
    if std::env::var("DEIA_PINTURA").map(|v| v == "no").unwrap_or(false) {
        settings.paint = false;
    }
    let sounds = Sounds::load(settings.volume).await;
    let t0 = get_time();
    let mut app = App::new(settings, sounds);
    if demo.is_some() {
        println!("escenario preparado en {:.2} s", get_time() - t0);
    }
    prevent_quit();
    loop {
        if is_quit_requested() {
            app.leave(None);
            if demo.is_none() {
                app.settings.save();
            }
            break;
        }
        if let Some(d) = demo.as_mut() {
            app.demo_step = d.step;
        }
        app.frame();
        if let Some(mut d) = demo.take() {
            let dt = get_frame_time().min(0.1);
            if app.demo_frame(&mut d, dt) {
                demo = Some(d);
            } else {
                break;
            }
        }
        next_frame().await;
    }
}

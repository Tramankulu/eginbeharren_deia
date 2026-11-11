//! Modelos de artista en el juego: texturas, mallas para macroquad,
//! soldados animados, arma en primera persona, objetos del mapa y escenario.
//!
//! Todo es opcional: si falta un modelo en el catálogo
//! (`assets/modelos/modelos.txt`) se usa el de siempre, hecho con código.

use deia::geo::{self, sun_dir};
use deia::math::*;
use deia::model::{self, Alpha, Entry, Model, Posed, M4};
use deia::world::{Prop, World};
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, CullFace, Equation, MipmapFilterMode, TextureWrap};
use macroquad::prelude::*;
use std::collections::HashMap;

const MAX_V: usize = 3000;
const MAX_I: usize = 4500;

// ----------------------------------------------------------------------
// Materiales (programas de dibujo)
// ----------------------------------------------------------------------

pub struct ArtMaterials {
    pub opaque: Material,
    pub blend: Material,
    pub ink: Material,
    pub ink_thin: Material,
}

fn art_material(thick: f32, cut: f32, blend: bool, cull: CullFace) -> Material {
    let pipeline_params = PipelineParams {
        depth_write: !blend,
        depth_test: Comparison::LessOrEqual,
        cull_face: cull,
        color_blend: Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::OneMinusValue(BlendValue::SourceAlpha))),
        ..Default::default()
    };
    let m = load_material(
        ShaderSource::Glsl { vertex: deia::shader::ART_VERTEX, fragment: deia::shader::ART_FRAGMENT },
        MaterialParams {
            pipeline_params,
            uniforms: vec![
                UniformDesc::new("SunDir", UniformType::Float3),
                UniformDesc::new("CamPos", UniformType::Float3),
                UniformDesc::new("Thick", UniformType::Float1),
                UniformDesc::new("Cut", UniformType::Float1),
            ],
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("No se pudo compilar el sombreador de modelos: {e:?}"));
    m.set_uniform("Thick", thick);
    m.set_uniform("Cut", cut);
    m
}

impl ArtMaterials {
    pub fn new() -> ArtMaterials {
        ArtMaterials {
            opaque: art_material(0.0, 0.5, false, CullFace::Nothing),
            blend: art_material(0.0, 0.02, true, CullFace::Nothing),
            ink: art_material(1.0, 0.5, false, CullFace::Front),
            ink_thin: art_material(0.3, 0.5, false, CullFace::Front),
        }
    }
    pub fn set_camera(&self, eye: V3) {
        let s = sun_dir();
        for (m, thick, cut) in [(&self.opaque, 0.0f32, 0.5f32), (&self.blend, 0.0, 0.02), (&self.ink, 1.0, 0.5), (&self.ink_thin, 0.3, 0.5)] {
            m.set_uniform("SunDir", vec3(s.x, s.y, s.z));
            m.set_uniform("CamPos", vec3(eye.x, eye.y, eye.z));
            m.set_uniform("Thick", thick);
            m.set_uniform("Cut", cut);
        }
    }
}

// ----------------------------------------------------------------------
// Modelo preparado para dibujar
// ----------------------------------------------------------------------

struct Chunk {
    /// Vértice original de cada vértice del trozo
    src: Vec<u32>,
    mesh: Mesh,
    ink: Option<Mesh>,
}

struct Part {
    blend: bool,
    color: Vec<[u8; 4]>,
    uv: Vec<Vec2>,
    /// Grupo de vértices en la misma posición (para el contorno)
    group: Vec<u32>,
    groups: usize,
    chunks: Vec<Chunk>,
}

/// Transformación del modelo a «metros del juego»: pies en 0, centrado,
/// mirando a +Z (convenio de glTF).
#[derive(Clone, Copy)]
struct Fit {
    scale: f32,
    offset: [f32; 3],
    turn: f32,
}

impl Fit {
    fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        let (s, c) = self.turn.sin_cos();
        let x = (p[0] + self.offset[0]) * self.scale;
        let y = (p[1] + self.offset[1]) * self.scale;
        let z = (p[2] + self.offset[2]) * self.scale;
        [x * c + z * s, y, -x * s + z * c]
    }
    fn dir(&self, n: [f32; 3]) -> [f32; 3] {
        let (s, c) = self.turn.sin_cos();
        [n[0] * c + n[2] * s, n[1], -n[0] * s + n[2] * c]
    }
}

pub struct Art {
    pub model: Model,
    parts: Vec<Part>,
    fit: Fit,
    posed: Vec<Posed>,
    ink: bool,
    /// Tamaño (ya ajustado) del modelo en reposo
    #[allow(dead_code)]
    pub size: V3,
}

fn load_texture(bytes: &[u8], opaque: bool) -> Option<Texture2D> {
    let mut img = Image::from_file_with_format(bytes, None).ok()?;
    if opaque {
        for p in img.get_image_data_mut() {
            p[3] = 255;
        }
    }
    let t = Texture2D::from_image(&img);
    t.set_filter(FilterMode::Linear);
    unsafe {
        let ctx = get_internal_gl().quad_context;
        let id = t.raw_miniquad_id();
        ctx.texture_set_wrap(id, TextureWrap::Repeat, TextureWrap::Repeat);
        ctx.texture_generate_mipmaps(id);
        ctx.texture_set_min_filter(id, FilterMode::Linear, MipmapFilterMode::Linear);
    }
    Some(t)
}

/// Nodos que solo sirven de marca en los escenarios (no se dibujan).
pub fn marker_name(n: &str) -> bool {
    let u = n.to_uppercase();
    ["COL", "SALIDA_", "BANDERA_", "BOMBA_", "PASO", "LIMITE"].iter().any(|p| u.starts_with(p))
}

impl Art {
    /// `height`: altura deseada en metros (None = usar la escala del archivo).
    pub fn load(e: &Entry, height: Option<f32>, center: bool) -> Result<Art, String> {
        let model = Model::load(&e.file)?;
        let globals = model.rest_globals();
        let mut posed = Vec::new();
        model.pose(&globals, &mut posed);
        let (lo, hi) = Model::bounds(&posed);
        let h = (hi[1] - lo[1]).max(1e-3);
        let scale = e.num("escala").unwrap_or_else(|| height.map(|t| t / h).unwrap_or(1.0));
        let offset = if center { [-(lo[0] + hi[0]) * 0.5, -lo[1], -(lo[2] + hi[2]) * 0.5] } else { [0.0; 3] };
        let fit = Fit { scale, offset, turn: e.num("giro").unwrap_or(0.0).to_radians() };
        let ink = !matches!(e.text("contorno"), Some("no"));

        let mut textures: HashMap<(usize, bool), Option<Texture2D>> = HashMap::new();
        let hidden: Vec<bool> = model.drawables().iter().flat_map(|&(n, m)| std::iter::repeat(marker_name(&model.nodes[n].name)).take(model.meshes[m].len())).collect();
        let mut parts = Vec::new();
        for (k, prim) in model.prims_in_order().into_iter().enumerate() {
            let mat = &model.materials[prim.material];
            let blend = mat.alpha == Alpha::Blend;
            let tex = mat.image.and_then(|i| {
                let opaque = mat.alpha == Alpha::Opaque;
                textures.entry((i, opaque)).or_insert_with(|| model.images.get(i).and_then(|b| load_texture(b, opaque))).clone()
            });
            let n = prim.pos.len();
            let color: Vec<[u8; 4]> = (0..n)
                .map(|v| {
                    let vc = prim.col.get(v).copied().unwrap_or([1.0; 4]);
                    let c = |a: f32, b: f32| ((a * b).clamp(0.0, 1.0) * 255.0).round() as u8;
                    [c(vc[0], mat.color[0]), c(vc[1], mat.color[1]), c(vc[2], mat.color[2]), c(vc[3], mat.color[3])]
                })
                .collect();
            let uv = prim.uv.iter().map(|u| vec2(u[0], u[1])).collect();
            // Grupos por posición para engordar el contorno sin que se abra
            let mut map: HashMap<(i64, i64, i64), u32> = HashMap::new();
            let group = prim
                .pos
                .iter()
                .map(|p| {
                    let key = ((p[0] * 4000.0).round() as i64, (p[1] * 4000.0).round() as i64, (p[2] * 4000.0).round() as i64);
                    let l = map.len() as u32;
                    *map.entry(key).or_insert(l)
                })
                .collect();
            let groups = map.len();
            let mut chunks = Vec::new();
            if !hidden.get(k).copied().unwrap_or(false) {
                let part_ink = ink && !blend && !mat.double_sided && !matches!(mat.alpha, Alpha::Mask(_));
                let mut src: Vec<u32> = Vec::new();
                let mut idx: Vec<u16> = Vec::new();
                let mut remap: HashMap<u32, u16> = HashMap::new();
                let mut flush = |src: &mut Vec<u32>, idx: &mut Vec<u16>, remap: &mut HashMap<u32, u16>| {
                    if idx.is_empty() {
                        return;
                    }
                    let verts = vec![Vertex::new(0.0, 0.0, 0.0, 0.0, 0.0, WHITE); src.len()];
                    let mesh = Mesh { vertices: verts.clone(), indices: idx.clone(), texture: tex.clone() };
                    let inkm = part_ink.then(|| Mesh { vertices: verts, indices: idx.clone(), texture: tex.clone() });
                    chunks.push(Chunk { src: std::mem::take(src), mesh, ink: inkm });
                    idx.clear();
                    remap.clear();
                };
                for t in prim.idx.chunks(3) {
                    if src.len() + 3 > MAX_V || idx.len() + 3 > MAX_I {
                        flush(&mut src, &mut idx, &mut remap);
                    }
                    for &i in t {
                        let l = src.len() as u16;
                        let j = *remap.entry(i).or_insert_with(|| {
                            src.push(i);
                            l
                        });
                        idx.push(j);
                    }
                }
                flush(&mut src, &mut idx, &mut remap);
            }
            parts.push(Part { blend, color, uv, group, groups, chunks });
        }
        let size = v3((hi[0] - lo[0]) * scale, (hi[1] - lo[1]) * scale, (hi[2] - lo[2]) * scale);
        Ok(Art { model, parts, fit, posed, ink, size })
    }

    /// Coloca el modelo con las matrices globales dadas y rellena las mallas.
    /// `place` lleva un punto del espacio del modelo (ya ajustado) al mundo.
    fn fill(&mut self, globals: &[M4], place: &dyn Fn([f32; 3]) -> V3, place_dir: &dyn Fn([f32; 3]) -> V3, sun: &dyn Fn(V3) -> f32) {
        let mut posed = std::mem::take(&mut self.posed);
        self.model.pose(globals, &mut posed);
        for (part, p) in self.parts.iter_mut().zip(&posed) {
            if part.chunks.is_empty() {
                continue;
            }
            let mut gn = vec![[0.0f32; 3]; part.groups];
            for (v, n) in p.nrm.iter().enumerate() {
                let g = &mut gn[part.group[v] as usize];
                g[0] += n[0];
                g[1] += n[1];
                g[2] += n[2];
            }
            for ch in &mut part.chunks {
                for (k, &s) in ch.src.iter().enumerate() {
                    let s = s as usize;
                    let w = place(self.fit.apply(p.pos[s]));
                    let n = place_dir(self.fit.dir(p.nrm[s])).norm();
                    let sv = sun(w);
                    let c = part.color[s];
                    let uv = part.uv[s];
                    ch.mesh.vertices[k] = Vertex { position: vec3(w.x, w.y, w.z), uv, color: c, normal: vec4(n.x, n.y, n.z, sv) };
                    if let Some(ink) = ch.ink.as_mut() {
                        let o = gn[part.group[s] as usize];
                        let o = place_dir(self.fit.dir(o)).norm();
                        ink.vertices[k] = Vertex { position: vec3(w.x, w.y, w.z), uv, color: c, normal: vec4(o.x, o.y, o.z, 1.0) };
                    }
                }
            }
        }
        self.posed = posed;
    }

    pub fn draw(&self, mats: &ArtMaterials, thin: bool) {
        gl_use_material(&mats.opaque);
        for p in self.parts.iter().filter(|p| !p.blend) {
            for c in &p.chunks {
                draw_mesh(&c.mesh);
            }
        }
        if self.ink {
            gl_use_material(if thin { &mats.ink_thin } else { &mats.ink });
            for p in &self.parts {
                for c in &p.chunks {
                    if let Some(m) = &c.ink {
                        draw_mesh(m);
                    }
                }
            }
        }
        if self.parts.iter().any(|p| p.blend) {
            gl_use_material(&mats.blend);
            for p in self.parts.iter().filter(|p| p.blend) {
                for c in &p.chunks {
                    draw_mesh(&c.mesh);
                }
            }
        }
    }

    /// Copia las mallas ya colocadas (para objetos fijos del mapa).
    fn snapshot(&self) -> (Vec<Mesh>, Vec<Mesh>, Vec<Mesh>) {
        let (mut o, mut i, mut b) = (Vec::new(), Vec::new(), Vec::new());
        for p in &self.parts {
            for c in &p.chunks {
                let m = Mesh { vertices: c.mesh.vertices.clone(), indices: c.mesh.indices.clone(), texture: c.mesh.texture.clone() };
                if p.blend {
                    b.push(m);
                } else {
                    o.push(m);
                }
                if let Some(k) = &c.ink {
                    i.push(Mesh { vertices: k.vertices.clone(), indices: k.indices.clone(), texture: k.texture.clone() });
                }
            }
        }
        (o, i, b)
    }
}

/// Giro alrededor de Y que lleva el +Z del modelo a la dirección `yaw` del juego.
fn yaw_place(pos: V3, yaw: f32, tilt: f32) -> (impl Fn([f32; 3]) -> V3, impl Fn([f32; 3]) -> V3) {
    let th = std::f32::consts::FRAC_PI_2 - yaw;
    let (s, c) = th.sin_cos();
    let (ts, tc) = tilt.sin_cos();
    let rot = move |p: [f32; 3]| {
        // Inclinación hacia delante (para caer si no hay animación de muerte)
        let (y, z) = (p[1] * tc - p[2] * ts, p[1] * ts + p[2] * tc);
        v3(p[0] * c + z * s, y, -p[0] * s + z * c)
    };
    (move |p: [f32; 3]| pos + rot(p), rot)
}

// ----------------------------------------------------------------------
// Soldados
// ----------------------------------------------------------------------

pub struct SoldierArt {
    pub art: Art,
    idle: Option<usize>,
    /// Sin animación de reposo: primera imagen de la de andar
    idle_frozen: bool,
    walk: Option<usize>,
    run: Option<usize>,
    crouch: Option<usize>,
    crouch_walk: Option<usize>,
    death: Option<usize>,
    aim: Option<usize>,
    spine: Option<(usize, Vec<usize>)>,
    speed: f32,
}

pub struct SoldierPose {
    pub pos: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub moving: f32,
    pub sprint: bool,
    pub crouch: f32,
    pub time: f32,
    /// Segundos desde la muerte
    pub dead: Option<f32>,
    pub sun: f32,
}

impl SoldierArt {
    fn new(e: &Entry) -> Result<SoldierArt, String> {
        let art = Art::load(e, Some(e.num("altura").unwrap_or(1.8)), true)?;
        let m = &art.model;
        let pick = |key: &str, def: &[&str], avoid: &[&str]| -> Option<usize> {
            if let Some(name) = e.text(key) {
                return m.find_anim(&[name], &[]);
            }
            m.find_anim(def, avoid)
        };
        let idle = pick("anim_quieto", &["idle", "quieto", "reposo", "stand", "survey"], &["crouch", "agach"]);
        let mut walk = pick("anim_andar", &["walk", "andar", "caminar"], &["crouch", "agach"]);
        // Una sola animación sin nombre conocido: se usa para andar
        if walk.is_none() && idle.is_none() && !m.anims.is_empty() {
            walk = Some(0);
        }
        let idle_frozen = idle.is_none() && walk.is_some();
        let idle = idle.or(walk);
        let run = pick("anim_correr", &["run", "correr", "sprint"], &["crouch", "agach"]);
        let crouch = pick("anim_agachado", &["crouch idle", "crouch_idle", "agachado", "crouch"], &["walk", "andar"]);
        let crouch_walk = pick("anim_agachado_andar", &["crouch walk", "crouch_walk", "agachado_andar", "crouched walking"], &[]);
        let death = pick("anim_morir", &["death", "die", "dying", "morir", "muerte"], &[]);
        let aim = pick("anim_apuntar", &["aim", "apuntar", "rifle idle"], &[]);
        let spine = e
            .text("columna")
            .and_then(|n| m.find_node(&[n]))
            .or_else(|| m.find_node(&["spine2", "spine1", "chest", "pecho", "spine", "columna"]))
            .map(|n| (n, m.subtree(n)));
        Ok(SoldierArt { idle, idle_frozen, walk, run, crouch, crouch_walk, death, aim, spine, speed: e.num("vel_anim").unwrap_or(1.0), art })
    }

    pub fn pose_and_draw(&mut self, p: &SoldierPose, mats: &ArtMaterials) {
        let m = &self.art.model;
        let t = p.time * self.speed;
        let mut mix: Vec<(usize, f32, f32)> = Vec::new();
        let mut tilt = 0.0;
        if let Some(d) = p.dead {
            match self.death {
                Some(a) => mix.push((a, d.min(m.anims[a].duration * 0.999), 1.0)),
                None => {
                    if let Some(i) = self.idle {
                        mix.push((i, 0.0, 1.0));
                    }
                    tilt = (d * 3.0).min(1.0).powi(2) * std::f32::consts::FRAC_PI_2;
                }
            }
        } else {
            let mv = p.moving.clamp(0.0, 1.0);
            let stand_idle = self.aim.filter(|_| mv < 0.5).or(self.idle);
            let stand_move = if p.sprint { self.run.or(self.walk) } else { self.walk.or(self.run) };
            let c = p.crouch.clamp(0.0, 1.0);
            let crouch_idle = self.crouch.or(stand_idle);
            let crouch_move = self.crouch_walk.or(self.crouch).or(stand_move);
            let still = if self.idle_frozen { 0.0 } else { t };
            for (a, w, tt) in [(stand_idle, (1.0 - mv) * (1.0 - c), still), (stand_move, mv * (1.0 - c), t), (crouch_idle, (1.0 - mv) * c, still), (crouch_move, mv * c, t)] {
                if let (Some(a), true) = (a, w > 0.001) {
                    mix.push((a, tt, w));
                }
            }
        }
        let local = m.local_pose(&mix);
        let mut g = m.globals(&local);
        // Inclinar el torso según hacia dónde apunta
        if let (Some((spine, sub)), None) = (&self.spine, p.dead) {
            let piv = [g[*spine][12], g[*spine][13], g[*spine][14]];
            let r = model::mul(&model::mul(&model::translation(piv), &model::rotation([1.0, 0.0, 0.0], -p.pitch * 0.7)), &model::translation([-piv[0], -piv[1], -piv[2]]));
            for &n in sub {
                g[n] = model::mul(&r, &g[n]);
            }
        }
        // Sin animación de agacharse: bajar el cuerpo un poco
        let drop = if self.crouch.is_none() && self.crouch_walk.is_none() { p.crouch * 0.35 } else { 0.0 };
        let (place, dir) = yaw_place(p.pos - V3::UP * drop, p.yaw, tilt);
        let sun = p.sun;
        self.art.fill(&g, &place, &dir, &|_| sun);
        self.art.draw(mats, false);
    }
}

// ----------------------------------------------------------------------
// Arma en primera persona
// ----------------------------------------------------------------------

pub struct ViewArt {
    pub art: Art,
    hip: V3,
    aim: V3,
    muzzle: [f32; 3],
    idle: Option<usize>,
}

pub struct ViewPose {
    pub eye: V3,
    pub fwd: V3,
    pub right: V3,
    pub up: V3,
    pub ads: f32,
    pub bob: f32,
    pub moving: f32,
    pub recoil: f32,
    pub lowered: f32,
    pub sprint: bool,
    pub sun: f32,
    pub time: f32,
}

impl ViewArt {
    fn new(e: &Entry) -> Result<ViewArt, String> {
        let art = Art::load(e, None, false)?;
        let m = &art.model;
        let g = m.rest_globals();
        let muzzle = e.vec3("boca").or_else(|| m.find_node(&["boca", "muzzle", "canon", "cañon"]).map(|n| [g[n][12], g[n][13], g[n][14]]));
        let muzzle = muzzle.unwrap_or_else(|| {
            let mut posed = Vec::new();
            m.pose(&g, &mut posed);
            let (lo, hi) = Model::bounds(&posed);
            [(lo[0] + hi[0]) * 0.5, hi[1] * 0.7 + lo[1] * 0.3, hi[2]]
        });
        let v = |k: &str, d: V3| e.vec3(k).map(|a| v3(a[0], a[1], a[2])).unwrap_or(d);
        Ok(ViewArt {
            hip: v("cadera", v3(0.16, -0.15, 0.28)),
            aim: v("apuntar", v3(0.0, 0.0, 0.2)),
            muzzle,
            idle: m.find_anim(&["idle", "quieto", "reposo"], &[]),
            art,
        })
    }

    /// Dibuja el arma y devuelve dónde está la boca del cañón.
    pub fn draw(&mut self, v: &ViewPose, mats: &ArtMaterials) -> V3 {
        let mut off = self.hip.lerp(self.aim, v.ads);
        let b = 1.0 - v.ads * 0.85;
        off.x += v.bob.sin() * 0.012 * v.moving * b;
        off.y += -(v.bob.cos().abs()) * 0.01 * v.moving * b;
        off.z -= v.recoil * 0.05;
        off.y += v.recoil * 0.008;
        off.y -= v.lowered * 0.28;
        if v.sprint {
            off.x -= 0.06;
            off.y -= 0.07;
        }
        let mut f = (v.fwd + v.up * (v.recoil * 0.06) + if v.sprint { -v.right * 0.6 } else { V3::ZERO }).norm();
        let mut r = f.cross(v.up).norm();
        let mut u = r.cross(f).norm();
        if !f.len().is_finite() || r.len() < 0.5 {
            f = v.fwd;
            r = v.right;
            u = v.up;
        }
        let base = v.eye + v.right * off.x + v.up * off.y + v.fwd * off.z;
        // +X del modelo es su izquierda
        let dir = move |p: [f32; 3]| -r * p[0] + u * p[1] + f * p[2];
        let place = move |p: [f32; 3]| base + dir(p);
        let m = &self.art.model;
        let g = match self.idle {
            Some(a) => m.globals(&m.local_pose(&[(a, v.time, 1.0)])),
            None => m.rest_globals(),
        };
        let sun = v.sun;
        self.art.fill(&g, &place, &dir, &|_| sun);
        self.art.draw(mats, true);
        place(self.art.fit.apply(self.muzzle))
    }
}

// ----------------------------------------------------------------------
// Catálogo cargado
// ----------------------------------------------------------------------

/// Trozo fijo del mapa ya colocado.
pub struct StaticArt {
    pub bounds: deia::world::Aabb,
    opaque: Vec<Mesh>,
    ink: Vec<Mesh>,
    blend: Vec<Mesh>,
}

impl StaticArt {
    pub fn draw_opaque(&self, mats: &ArtMaterials) {
        gl_use_material(&mats.opaque);
        for m in &self.opaque {
            draw_mesh(m);
        }
        gl_use_material(&mats.ink);
        for m in &self.ink {
            draw_mesh(m);
        }
    }
    pub fn draw_blend(&self, mats: &ArtMaterials) {
        if self.blend.is_empty() {
            return;
        }
        gl_use_material(&mats.blend);
        for m in &self.blend {
            draw_mesh(m);
        }
    }
}

pub struct ArtSet {
    pub mats: ArtMaterials,
    /// [Aliados, Eje] x [fusil, subfusil]
    soldiers: [[Option<usize>; 2]; 2],
    soldier_pool: Vec<SoldierArt>,
    /// Índice de `gun_index` (0..4)
    views: [Option<ViewArt>; 4],
    pub statics: Vec<StaticArt>,
    /// Roles de objetos sustituidos (para no dibujar los de código)
    pub replaced: Vec<&'static str>,
    pub has_scene: bool,
    pub errors: Vec<String>,
}

fn hashf(p: V3) -> f32 {
    let h = (p.x * 12.9898 + p.z * 78.233 + p.y * 37.719).sin() * 43758.547;
    h - h.floor()
}

/// Luz del sol en un punto, contando solo las cajas que no lo contienen
/// (las colisiones envuelven a los modelos y los dejarían a oscuras).
fn sun_at(world: &World, p: V3, n: V3) -> f32 {
    let o = p + n * 0.06 + V3::UP * 0.02;
    let d = sun_dir();
    for b in &world.blocks {
        if b.b.contains(o) {
            continue;
        }
        if b.b.ray(o, d, 90.0).is_some() {
            return 0.0;
        }
    }
    1.0
}

impl ArtSet {
    pub fn load(world: &World) -> ArtSet {
        let cat = model::catalog();
        let mut set = ArtSet {
            mats: ArtMaterials::new(),
            soldiers: [[None; 2]; 2],
            soldier_pool: Vec::new(),
            views: [None, None, None, None],
            statics: Vec::new(),
            replaced: Vec::new(),
            has_scene: false,
            errors: Vec::new(),
        };
        let find = |r: &str| cat.iter().find(|e| e.role == r);
        // Soldados: aliado/eje, con variante opcional de subfusil
        for (side, name) in [(0usize, "aliado"), (1, "eje")] {
            for (slot, suffix) in [(0usize, ""), (1, "_subfusil")] {
                let role = format!("soldado_{name}{suffix}");
                if let Some(e) = find(&role) {
                    match SoldierArt::new(e) {
                        Ok(s) => {
                            set.soldier_pool.push(s);
                            set.soldiers[side][slot] = Some(set.soldier_pool.len() - 1);
                        }
                        Err(err) => set.errors.push(err),
                    }
                }
            }
            if set.soldiers[side][1].is_none() {
                set.soldiers[side][1] = set.soldiers[side][0];
            }
        }
        // Armas en primera persona (mismo orden que gun_index)
        for (k, role) in ["vista_aliado_fusil", "vista_aliado_subfusil", "vista_eje_fusil", "vista_eje_subfusil"].iter().enumerate() {
            if let Some(e) = find(role) {
                match ViewArt::new(e) {
                    Ok(v) => set.views[k] = Some(v),
                    Err(err) => set.errors.push(err),
                }
            }
        }
        // Escenario
        if let Some(path) = &world.scene {
            if let Some(e) = cat.iter().find(|e| e.role == "escenario" && &e.file == path) {
                match Art::load(e, None, false) {
                    Ok(mut a) => {
                        a.fit.scale = e.num("escala").unwrap_or(1.0);
                        let g = a.model.rest_globals();
                        a.fill(&g, &|p| v3(p[0], p[1], p[2]), &|n| v3(n[0], n[1], n[2]), &|_| 1.0);
                        set.push_static(&a, world, true);
                        set.has_scene = true;
                    }
                    Err(err) => set.errors.push(err),
                }
            }
        }
        // Objetos del pueblo
        let roles = ["arbol", "tanque", "carro", "barril", "paja"];
        for role in roles {
            let variants: Vec<&Entry> = cat.iter().filter(|e| e.role == role || (e.role.starts_with(role) && e.role[role.len()..].chars().all(|c| c.is_ascii_digit()))).collect();
            if variants.is_empty() {
                continue;
            }
            let mut arts: Vec<(Art, &Entry)> = Vec::new();
            for e in &variants {
                let h = match role {
                    "arbol" => 8.0,
                    "tanque" => 2.7,
                    "carro" => 2.0,
                    "barril" => 0.95,
                    _ => 1.0,
                };
                match Art::load(e, Some(e.num("altura").unwrap_or(h)), true) {
                    Ok(a) => arts.push((a, e)),
                    Err(err) => set.errors.push(err),
                }
            }
            if arts.is_empty() {
                continue;
            }
            set.replaced.push(role);
            for p in &world.props {
                if geo::prop_role(p) != Some(role) {
                    continue;
                }
                let (pos, yaw, scale) = match p {
                    Prop::Tree { pos, size } => (*pos, hashf(*pos) * std::f32::consts::TAU, *size),
                    // El cañón del tanque apunta a -Z
                    Prop::Tank(c) => (*c, -std::f32::consts::FRAC_PI_2, 1.0),
                    // El carro va a lo largo de X
                    Prop::Cart(c) => (*c - V3::UP * c.y, 0.0, 1.0),
                    Prop::Barrel(c) => (*c, hashf(*c) * 6.0, 1.0),
                    Prop::Hay(c) => (*c, hashf(*c) * 6.0, 1.0),
                    _ => continue,
                };
                let k = ((hashf(pos + V3::UP) * arts.len() as f32) as usize).min(arts.len() - 1);
                let a = &mut arts[k].0;
                let g = a.model.rest_globals();
                let (place, dir) = yaw_place(V3::ZERO, yaw, 0.0);
                let place = move |q: [f32; 3]| pos + place([q[0] * scale, q[1] * scale, q[2] * scale]);
                a.fill(&g, &place, &dir, &|_| 1.0);
                set.push_static(a, world, false);
            }
        }
        for e in &set.errors {
            eprintln!("Modelo no válido: {e}");
        }
        set
    }

    /// Guarda la geometría ya colocada de `a` como trozos fijos, con la luz
    /// del sol calculada en cada vértice.
    fn push_static(&mut self, a: &Art, world: &World, split: bool) {
        let (mut o, mut i, mut b) = a.snapshot();
        for m in o.iter_mut().chain(b.iter_mut()) {
            for v in m.vertices.iter_mut() {
                let p = v3(v.position.x, v.position.y, v.position.z);
                let n = v3(v.normal.x, v.normal.y, v.normal.z);
                v.normal.w = sun_at(world, p, n);
            }
        }
        let bounds_of = |ms: &[Mesh]| {
            let mut lo = v3(f32::MAX, f32::MAX, f32::MAX);
            let mut hi = v3(f32::MIN, f32::MIN, f32::MIN);
            for m in ms {
                for v in &m.vertices {
                    lo = v3(lo.x.min(v.position.x), lo.y.min(v.position.y), lo.z.min(v.position.z));
                    hi = v3(hi.x.max(v.position.x), hi.y.max(v.position.y), hi.z.max(v.position.z));
                }
            }
            deia::world::Aabb::new(lo, hi)
        };
        if split {
            // Un trozo por malla: así se descarta lo que no se ve
            let ink_of = |m: &Mesh, inks: &mut Vec<Mesh>| -> Vec<Mesh> {
                // el contorno de cada malla tiene los mismos índices
                if let Some(k) = inks.iter().position(|x| x.indices == m.indices && x.vertices.len() == m.vertices.len()) {
                    vec![inks.swap_remove(k)]
                } else {
                    Vec::new()
                }
            };
            for m in o.drain(..) {
                let ink = ink_of(&m, &mut i);
                let bb = bounds_of(std::slice::from_ref(&m));
                self.statics.push(StaticArt { bounds: bb, opaque: vec![m], ink, blend: Vec::new() });
            }
            for m in b.drain(..) {
                let bb = bounds_of(std::slice::from_ref(&m));
                self.statics.push(StaticArt { bounds: bb, opaque: Vec::new(), ink: Vec::new(), blend: vec![m] });
            }
        } else {
            let all: Vec<&Mesh> = o.iter().chain(b.iter()).collect();
            let bb = bounds_of(&all.into_iter().map(|m| Mesh { vertices: m.vertices.clone(), indices: Vec::new(), texture: None }).collect::<Vec<_>>());
            self.statics.push(StaticArt { bounds: bb, opaque: o, ink: i, blend: b });
        }
    }

    #[allow(dead_code)]
    pub fn has_soldier(&self, side: u8, smg: bool) -> bool {
        self.soldiers[(side as usize).min(1)][smg as usize].is_some()
    }

    pub fn draw_soldier(&mut self, side: u8, smg: bool, p: &SoldierPose) -> bool {
        let Some(i) = self.soldiers[(side as usize).min(1)][smg as usize] else { return false };
        self.soldier_pool[i].pose_and_draw(p, &self.mats);
        true
    }

    pub fn has_view(&self, gun: usize) -> bool {
        self.views.get(gun).map(|v| v.is_some()).unwrap_or(false)
    }

    pub fn draw_view(&mut self, gun: usize, v: &ViewPose) -> Option<V3> {
        let mats = &self.mats;
        self.views.get_mut(gun)?.as_mut().map(|a| a.draw(v, mats))
    }

    pub fn any(&self) -> bool {
        !self.soldier_pool.is_empty() || self.views.iter().any(|v| v.is_some()) || !self.statics.is_empty()
    }
}

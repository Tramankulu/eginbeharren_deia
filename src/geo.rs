//! Geometría "low-poly" generada por código: cajas sombreadas por cara,
//! el mapa estático, los soldados y el arma en primera persona.
//! Es Rust puro; `main.rs` la convierte en mallas de macroquad.

use crate::math::*;
use crate::world::{Aabb, World};
use std::f32::consts::PI;

pub type Col = [f32; 4];

pub const IDENT: [V3; 3] = [V3::X, V3::UP, V3::Z];

pub struct Geo {
    /// Cuatro vértices por cuadrilátero: (posición, color RGBA ya sombreado).
    pub verts: Vec<(V3, Col)>,
}

impl Default for Geo {
    fn default() -> Self {
        Self::new()
    }
}

fn rgba(c: [f32; 3], a: f32) -> Col {
    [c[0], c[1], c[2], a]
}

impl Geo {
    pub fn new() -> Geo {
        Geo { verts: Vec::with_capacity(4096) }
    }
    pub fn clear(&mut self) {
        self.verts.clear();
    }
    pub fn quads(&self) -> usize {
        self.verts.len() / 4
    }

    pub fn quad(&mut self, c: [V3; 4], n: V3, col: Col) {
        let l = v3(0.42, 0.85, 0.32).norm();
        let mut s = 0.5 + 0.5 * n.dot(l).max(0.0);
        if n.y < -0.5 {
            s -= 0.12;
        }
        for p in c {
            let ao = if p.y < 0.06 { 0.8 } else { 1.0 };
            let k = s * ao;
            self.verts.push((p, [col[0] * k, col[1] * k, col[2] * k, col[3]]));
        }
    }

    /// Caja orientada: centro, ejes (derecha, arriba, adelante) y semitamaños.
    pub fn obox(&mut self, c: V3, ax: [V3; 3], h: [f32; 3], col: Col) {
        let (r, u, f) = (ax[0] * h[0], ax[1] * h[1], ax[2] * h[2]);
        let p = |sr: f32, su: f32, sf: f32| c + r * sr + u * su + f * sf;
        self.quad([p(1., -1., -1.), p(1., 1., -1.), p(1., 1., 1.), p(1., -1., 1.)], ax[0], col);
        self.quad([p(-1., -1., -1.), p(-1., -1., 1.), p(-1., 1., 1.), p(-1., 1., -1.)], -ax[0], col);
        self.quad([p(-1., 1., -1.), p(-1., 1., 1.), p(1., 1., 1.), p(1., 1., -1.)], ax[1], col);
        self.quad([p(-1., -1., -1.), p(1., -1., -1.), p(1., -1., 1.), p(-1., -1., 1.)], -ax[1], col);
        self.quad([p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.)], ax[2], col);
        self.quad([p(-1., -1., -1.), p(-1., 1., -1.), p(1., 1., -1.), p(1., -1., -1.)], -ax[2], col);
    }

    pub fn aabb(&mut self, b: &Aabb, col: Col) {
        let s = b.size() * 0.5;
        self.obox(b.center(), IDENT, [s.x, s.y, s.z], col);
    }

    pub fn cube(&mut self, c: V3, size: f32, col: Col) {
        let h = size * 0.5;
        self.obox(c, IDENT, [h, h, h], col);
    }
}

/// Construye la geometría estática del mapa (una sola vez).
pub fn build_world(w: &World) -> Geo {
    let mut g = Geo::new();
    // Suelo en losetas, con ligera variación de color
    let tile = 8.0;
    let n = (w.half / tile).ceil() as i32;
    for ix in -n..n {
        for iz in -n..n {
            let x0 = ix as f32 * tile;
            let z0 = iz as f32 * tile;
            let k = 0.9 + 0.18 * hash3(v3(x0, 0.0, z0));
            let c = [0.33 * k, 0.37 * k, 0.22 * k, 1.0];
            g.quad(
                [v3(x0, 0.0, z0), v3(x0, 0.0, z0 + tile), v3(x0 + tile, 0.0, z0 + tile), v3(x0 + tile, 0.0, z0)],
                V3::UP,
                c,
            );
        }
    }
    // Campos exteriores hasta el horizonte (la niebla los disuelve)
    let h = w.half;
    let far = 320.0;
    let outer = [0.31, 0.35, 0.21, 1.0];
    for (a, b) in [
        ((-far, -far), (far, -h)),
        ((-far, h), (far, far)),
        ((-far, -h), (-h, h)),
        ((h, -h), (far, h)),
    ] {
        let ((x0, z0), (x1, z1)) = (a, b);
        g.quad([v3(x0, -0.01, z0), v3(x0, -0.01, z1), v3(x1, -0.01, z1), v3(x1, -0.01, z0)], V3::UP, outer);
    }
    // Arboleda lejana
    let mut rng = Rng::new(44);
    for k in 0..90 {
        let ang = k as f32 / 90.0 * 2.0 * PI + rng.range(-0.03, 0.03);
        let d = rng.range(85.0, 140.0);
        let c = v3(ang.cos() * d, 0.0, ang.sin() * d);
        let s = v3(rng.range(8.0, 16.0), rng.range(9.0, 20.0), rng.range(8.0, 16.0));
        let col = [0.20, 0.30, 0.15, 1.0];
        g.obox(c + V3::UP * (s.y * 0.5), IDENT, [s.x * 0.5, s.y * 0.5, s.z * 0.5], col);
    }
    for d in &w.decals {
        g.aabb(&d.b, rgba(d.color, 1.0));
    }
    for b in &w.blocks {
        let c = b.mat.color();
        g.aabb(&b.b, [c[0] * b.tint, c[1] * b.tint, c[2] * b.tint, 1.0]);
    }
    g
}

fn mix(a: Col, b: Col, t: f32) -> Col {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t), lerp(a[3], b[3], t)]
}

/// Soldado enemigo hecho de cajas. Si `death_t` es Some, cae hacia atrás.
pub struct SoldierLook {
    pub side: u8,
    pub smg: bool,
    pub crouch: f32,
    pub pitch: f32,
    pub flash: f32,
    pub has_flag: Option<Col>,
}

pub fn soldier(g: &mut Geo, pos: V3, yaw: f32, death_t: Option<f32>, walk: f32, moving: f32, look: &SoldierLook) {
    let (smg, flash) = (look.smg, look.flash);
    let fwd0 = V3::from_yaw(yaw);
    let r = V3::right_of_yaw(yaw);
    let (theta, lift) = match death_t {
        Some(t) => {
            let k = (t * 2.2).min(1.0);
            let k = k * k * (3.0 - 2.0 * k);
            (k * PI * 0.5, k * 0.14)
        }
        None => (0.0, 0.0),
    };
    let u = V3::UP * theta.cos() - fwd0 * theta.sin();
    let f = fwd0 * theta.cos() + V3::UP * theta.sin();
    // Agachado: el cuerpo baja y las piernas se flexionan
    let crouch = if death_t.is_some() { 0.0 } else { look.crouch };
    let origin = pos + V3::UP * (lift - crouch * 0.45);
    let at = |x: f32, y: f32, z: f32| origin + r * x + u * y + f * z;
    let fl = (flash / 0.12).clamp(0.0, 1.0);
    let tint = |c: Col| mix(c, [1.0, 0.35, 0.3, 1.0], fl * 0.7);

    let allied = look.side == 0;
    let uniform = tint(if allied { [0.42, 0.38, 0.25, 1.0] } else { [0.36, 0.38, 0.32, 1.0] });
    let trousers = tint(if allied { [0.40, 0.34, 0.24, 1.0] } else { [0.30, 0.31, 0.27, 1.0] });
    let boots = if allied { [0.30, 0.22, 0.14, 1.0] } else { [0.12, 0.11, 0.10, 1.0] };
    let skin = tint([0.78, 0.62, 0.50, 1.0]);
    let helmet = if allied { [0.31, 0.33, 0.22, 1.0] } else { [0.27, 0.30, 0.25, 1.0] };
    let wood = [0.42, 0.28, 0.16, 1.0];
    let metal = [0.15, 0.15, 0.16, 1.0];
    let belt = [0.20, 0.17, 0.12, 1.0];

    // Extremidad: pivote local, ángulo (0 = abajo, PI/2 = adelante), largo, grosor
    let limb = |g: &mut Geo, px: f32, py: f32, pz: f32, a: f32, len: f32, wdt: f32, col: Col| {
        let d = (-u * a.cos() + f * a.sin()).norm();
        let third = r.cross(d).norm();
        let pivot = at(px, py, pz);
        g.obox(pivot + d * (len * 0.5), [r, d, third], [wdt, len * 0.5, wdt], col);
    };
    let swing = if death_t.is_some() { 0.0 } else { walk.sin() * 0.55 * moving.min(1.0) };
    // Piernas y botas (al agacharse, muslos hacia delante)
    let thigh = crouch * 1.3;
    if crouch > 0.3 {
        limb(g, -0.11, 0.9, 0.0, thigh, 0.45, 0.075, trousers);
        limb(g, 0.11, 0.9, 0.0, thigh, 0.45, 0.075, trousers);
        let knee = |sx: f32| at(sx * 0.11, 0.9, 0.0) + (-u * thigh.cos() + f * thigh.sin()) * 0.45;
        for sx in [-1.0f32, 1.0] {
            let k = knee(sx);
            let d = -u;
            g.obox(k + d * 0.22, [r, d, f], [0.075, 0.22, 0.075], trousers);
            g.obox(k + d * 0.44 + f * 0.04, [r, u, f], [0.07, 0.06, 0.13], boots);
        }
    } else {
        limb(g, -0.11, 0.9, 0.0, swing, 0.82, 0.075, trousers);
        limb(g, 0.11, 0.9, 0.0, -swing, 0.82, 0.075, trousers);
        let foot = |s: f32| at(s * 0.11, 0.06, 0.04);
        g.obox(foot(-1.0) + f * (swing.sin() * 0.4), [r, u, f], [0.07, 0.06, 0.13], boots);
        g.obox(foot(1.0) + f * (-swing.sin() * 0.4), [r, u, f], [0.07, 0.06, 0.13], boots);
    }
    // Torso, cinturón, cabeza y casco
    g.obox(at(0.0, 1.2, 0.0), [r, u, f], [0.23, 0.3, 0.14], uniform);
    g.obox(at(0.0, 0.93, 0.0), [r, u, f], [0.235, 0.045, 0.145], belt);
    g.obox(at(0.0, 1.6, 0.01), [r, u, f], [0.105, 0.12, 0.11], skin);
    if allied {
        // Casco M1: cúpula baja con ala uniforme
        g.obox(at(0.0, 1.73, 0.0), [r, u, f], [0.14, 0.065, 0.15], helmet);
        g.obox(at(0.0, 1.68, 0.0), [r, u, f], [0.17, 0.02, 0.18], helmet);
    } else {
        // Stahlhelm: más alto y con faldón trasero
        g.obox(at(0.0, 1.72, -0.01), [r, u, f], [0.145, 0.07, 0.16], helmet);
        g.obox(at(0.0, 1.66, -0.05), [r, u, f], [0.165, 0.025, 0.165], helmet);
    }
    if let Some(fc) = look.has_flag {
        g.obox(at(-0.1, 1.6, -0.2), [r, u, f], [0.02, 0.75, 0.02], [0.4, 0.3, 0.2, 1.0]);
        g.obox(at(-0.1, 2.1, -0.45), [r, u, f], [0.015, 0.22, 0.26], fc);
    }
    // Brazos sujetando el arma (siguen el cabeceo de la vista)
    let arm_a = if death_t.is_some() { 0.25 } else { 1.15 + look.pitch * 0.6 };
    limb(g, -0.27, 1.42, 0.0, arm_a, 0.55, 0.06, uniform);
    limb(g, 0.27, 1.42, 0.0, arm_a - 0.2, 0.5, 0.06, uniform);
    // Arma
    if death_t.map(|t| t < 0.3).unwrap_or(true) {
        // El arma gira con el cabeceo alrededor del hombro
        let pf = (f * look.pitch.cos() + u * look.pitch.sin()).norm();
        let pu = r.cross(pf).norm() * -1.0;
        let pu = if pu.dot(u) < 0.0 { -pu } else { pu };
        let base = at(0.08, 1.3, 0.1);
        let ax = [r, pu, pf];
        if smg {
            g.obox(base + pf * 0.3 - pu * 0.06, ax, [0.035, 0.055, 0.3], metal);
            g.obox(base + pf * 0.34 - pu * 0.2, ax, [0.02, 0.1, 0.03], metal);
        } else {
            g.obox(base + pf * 0.3 - pu * 0.05, ax, [0.03, 0.04, 0.55], wood);
            g.obox(base + pf * 0.88 - pu * 0.02, ax, [0.015, 0.015, 0.12], metal);
        }
    }
}

/// Caja de munición en el suelo.
pub fn ammo_box(g: &mut Geo, pos: V3, t: f32) {
    let y = pos.y + 0.18 + (t * 2.0).sin() * 0.05;
    let yaw = t * 1.2;
    let r = V3::right_of_yaw(yaw);
    let f = V3::from_yaw(yaw);
    let c = v3(pos.x, y, pos.z);
    g.obox(c, [r, V3::UP, f], [0.22, 0.13, 0.12], [0.32, 0.35, 0.2, 1.0]);
    g.obox(c + V3::UP * 0.135, [r, V3::UP, f], [0.08, 0.01, 0.125], [0.85, 0.8, 0.55, 1.0]);
}

pub fn grenade(g: &mut Geo, pos: V3, spin: f32, allied: bool) {
    let a = V3::from_yaw(spin);
    let r = V3::right_of_yaw(spin);
    if allied {
        g.obox(pos, [r, V3::UP, a], [0.055, 0.07, 0.055], [0.25, 0.3, 0.18, 1.0]);
    } else {
        // Granada de mango alemana
        g.obox(pos, [r, a, V3::UP], [0.03, 0.17, 0.03], [0.45, 0.33, 0.2, 1.0]);
        g.obox(pos + a * 0.19, [r, a, V3::UP], [0.06, 0.05, 0.06], [0.3, 0.32, 0.28, 1.0]);
    }
}

pub struct ViewParams {
    /// Índice en sim::GUNS: 0 Garand, 1 Thompson, 2 Kar98k, 3 MP40.
    pub gun: u8,
    pub ads: f32,
    pub bob: f32,
    pub moving: f32,
    pub recoil: f32,
    pub lowered: f32,
    pub sprint: bool,
    pub muzzle: f32,
    pub flash_seed: f32,
}

/// Arma en primera persona, construida en el espacio de la cámara.
pub fn viewmodel(g: &mut Geo, eye: V3, fwd: V3, right: V3, up: V3, v: &ViewParams) {
    let ads = v.ads;
    let hip = v3(0.2, -0.2, 0.42);
    let aim = v3(0.0, -0.053, 0.31);
    let mut off = hip.lerp(aim, ads);
    let b = 1.0 - ads * 0.85;
    off.x += v.bob.sin() * 0.012 * v.moving * b;
    off.y += -(v.bob.cos().abs()) * 0.01 * v.moving * b;
    off.z -= v.recoil * 0.05;
    off.y += v.recoil * 0.008;
    off.y -= v.lowered * 0.28;
    if v.sprint {
        off.x -= 0.06;
        off.y -= 0.07;
    }
    // Ejes del arma: leve cabeceo por retroceso y giro al correr
    let mut f = (fwd + up * (v.recoil * 0.06) + if v.sprint { -right * 0.6 } else { V3::ZERO }).norm();
    let mut r = f.cross(up).norm();
    let mut u = r.cross(f).norm();
    if !f.len().is_finite() || r.len() < 0.5 {
        f = fwd;
        r = right;
        u = up;
    }
    let base = eye + right * off.x + up * off.y + fwd * off.z;
    let ax = [r, u, f];
    let at = |x: f32, y: f32, z: f32| base + r * x + u * y + f * z;

    let wood = [0.45, 0.29, 0.15, 1.0];
    let metal = [0.13, 0.13, 0.14, 1.0];
    let sleeve = if v.gun >= 2 { [0.36, 0.38, 0.32, 1.0] } else { [0.42, 0.38, 0.25, 1.0] };
    let skin = [0.78, 0.6, 0.47, 1.0];

    let tip;
    match v.gun {
        0 | 2 => {
            g.obox(at(0.0, -0.037, -0.12), ax, [0.022, 0.035, 0.2], wood);
            g.obox(at(0.0, -0.065, -0.27), ax, [0.022, 0.05, 0.06], wood);
            g.obox(at(0.0, -0.02, 0.1), ax, [0.02, 0.022, 0.08], metal);
            g.obox(at(0.0, -0.032, 0.32), ax, [0.022, 0.024, 0.16], wood);
            g.obox(at(0.0, -0.01, 0.42), ax, [0.009, 0.009, 0.24], metal);
            // Miras: alza de apertura (dos postes) y punto de mira
            g.obox(at(-0.008, -0.004, 0.03), ax, [0.003, 0.012, 0.005], metal);
            g.obox(at(0.008, -0.004, 0.03), ax, [0.003, 0.012, 0.005], metal);
            g.obox(at(0.0, -0.008, 0.635), ax, [0.0035, 0.009, 0.005], metal);
            if v.gun == 2 {
                // Manilla del cerrojo del Kar98k
                g.obox(at(0.035, -0.015, 0.06), ax, [0.02, 0.006, 0.006], metal);
                g.obox(at(0.055, -0.02, 0.06), ax, [0.01, 0.01, 0.01], metal);
            }
            g.obox(at(-0.012, -0.065, 0.3), ax, [0.03, 0.028, 0.05], skin);
            tip = at(0.0, -0.01, 0.68);
        }
        3 => {
            // MP40: todo metal, cargador recto bajo el arma
            g.obox(at(0.0, -0.035, 0.06), ax, [0.022, 0.028, 0.16], metal);
            g.obox(at(0.0, -0.03, 0.3), ax, [0.012, 0.014, 0.09], metal);
            g.obox(at(0.0, -0.03, 0.4), ax, [0.016, 0.018, 0.015], metal);
            g.obox(at(0.0, -0.045, -0.16), ax, [0.006, 0.02, 0.1], metal);
            g.obox(at(0.0, -0.075, -0.26), ax, [0.02, 0.03, 0.01], metal);
            g.obox(at(0.0, -0.1, -0.02), ax, [0.017, 0.045, 0.022], [0.2, 0.15, 0.1, 1.0]);
            g.obox(at(0.0, -0.13, 0.14), ax, [0.014, 0.085, 0.02], metal);
            g.obox(at(0.0, -0.002, -0.02), ax, [0.009, 0.006, 0.008], metal);
            g.obox(at(0.0, -0.005, 0.38), ax, [0.0025, 0.009, 0.004], metal);
            g.obox(at(-0.01, -0.12, 0.14), ax, [0.03, 0.035, 0.035], skin);
            tip = at(0.0, -0.03, 0.42);
        }
        _ => {
            g.obox(at(0.0, -0.035, 0.08), ax, [0.025, 0.03, 0.14], metal);
            g.obox(at(0.0, -0.03, 0.3), ax, [0.014, 0.016, 0.1], metal);
            g.obox(at(0.0, -0.03, 0.42), ax, [0.018, 0.02, 0.025], metal);
            g.obox(at(0.0, -0.05, -0.15), ax, [0.02, 0.04, 0.12], wood);
            g.obox(at(0.0, -0.1, 0.2), ax, [0.017, 0.045, 0.02], wood);
            g.obox(at(0.0, -0.1, -0.01), ax, [0.017, 0.045, 0.022], wood);
            g.obox(at(0.0, -0.115, 0.08), ax, [0.015, 0.075, 0.02], metal);
            g.obox(at(0.0, -0.001, -0.02), ax, [0.01, 0.006, 0.01], metal);
            g.obox(at(0.0, -0.004, 0.39), ax, [0.0025, 0.009, 0.004], metal);
            g.obox(at(-0.01, -0.11, 0.2), ax, [0.03, 0.035, 0.035], skin);
            tip = at(0.0, -0.03, 0.46);
        }
    }
    // Mano derecha y antebrazos
    g.obox(at(0.005, -0.075, -0.03), ax, [0.03, 0.035, 0.045], skin);
    g.obox(at(0.05, -0.14, -0.13), ax, [0.035, 0.035, 0.1], sleeve);
    g.obox(at(-0.06, -0.12, 0.18), ax, [0.035, 0.035, 0.12], sleeve);

    if v.muzzle > 0.0 {
        let s = 0.05 + 0.04 * (v.flash_seed * 13.7).sin().abs();
        g.obox(tip + f * s, ax, [s * 0.7, s * 0.7, s * 1.3], [1.0, 0.85, 0.45, 0.9]);
    }
}

pub const FLAG_COLORS: [Col; 2] = [[0.30, 0.42, 0.22, 1.0], [0.62, 0.10, 0.08, 1.0]];

/// Bandera clavada en el suelo (ondea con el tiempo).
pub fn flag(g: &mut Geo, pos: V3, team: u8, t: f32, base: bool) {
    let c = FLAG_COLORS[team.min(1) as usize];
    if base {
        g.obox(pos + V3::UP * 0.05, IDENT, [0.9, 0.05, 0.9], [c[0] * 0.6, c[1] * 0.6, c[2] * 0.6, 1.0]);
    }
    g.obox(pos + V3::UP * 1.4, IDENT, [0.03, 1.4, 0.03], [0.45, 0.35, 0.22, 1.0]);
    let wave = (t * 3.0).sin() * 0.15;
    let f = V3::from_yaw(wave);
    let r = V3::right_of_yaw(wave);
    g.obox(pos + V3::UP * 2.45 + f * 0.45, [r, V3::UP, f], [0.015, 0.3, 0.42], c);
    g.obox(pos + V3::UP * 2.45 + f * 0.45 + r * 0.02, [r, V3::UP, f], [0.01, 0.08, 0.12], [0.95, 0.95, 0.9, 1.0]);
}

/// Objetivo de Buscar y destruir: cañón antitanque con cajas de munición.
pub fn bomb_site(g: &mut Geo, pos: V3) {
    let metal = [0.28, 0.30, 0.24, 1.0];
    let wood = [0.45, 0.33, 0.2, 1.0];
    g.obox(pos + v3(1.2, 0.45, 0.6), IDENT, [0.5, 0.45, 0.35], wood);
    g.obox(pos + v3(1.2, 1.05, 0.6), IDENT, [0.4, 0.15, 0.3], wood);
    g.obox(pos + v3(-1.0, 0.7, -0.8), IDENT, [0.9, 0.15, 0.5], metal);
    g.obox(pos + v3(-1.0, 0.95, -0.2), IDENT, [0.06, 0.06, 1.3], metal);
    g.obox(pos + v3(-1.0, 0.9, -1.0), IDENT, [0.55, 0.35, 0.05], metal);
    for sx in [-1.0f32, 1.0] {
        g.obox(pos + v3(-1.0 + sx * 0.9, 0.35, -0.8), IDENT, [0.05, 0.35, 0.35], [0.15, 0.15, 0.15, 1.0]);
    }
}

pub fn planted_bomb(g: &mut Geo, pos: V3, t: f32) {
    g.obox(pos + V3::UP * 0.12, IDENT, [0.22, 0.12, 0.15], [0.35, 0.28, 0.2, 1.0]);
    let blink = if (t * 6.0).sin() > 0.0 { [1.0, 0.1, 0.05, 1.0] } else { [0.3, 0.05, 0.05, 1.0] };
    g.obox(pos + V3::UP * 0.27, IDENT, [0.04, 0.03, 0.04], blink);
}

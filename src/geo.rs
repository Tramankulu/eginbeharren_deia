//! Geometría generada por código, con un estilo de ilustración pintada:
//! formas redondeadas, prados con hierba y flores, casas encaladas con
//! entramado y contraventanas, árboles frondosos, colinas y nubes.
//!
//! Es Rust puro. Cada vértice lleva su color base, su normal, el material
//! (que el sombreador usa para pintar detalle) y la luz precalculada:
//! cuánto le da el sol (`sun`) y cuánto cielo ve (`ao`).

use crate::math::*;
use crate::world::{Aabb, Mat, Prop, World};
use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

pub type Col = [f32; 4];

/// Identificadores de material (los interpreta el sombreador).
pub mod m {
    pub const PLAIN: f32 = 0.0;
    pub const GRASS: f32 = 1.0;
    pub const PLASTER: f32 = 2.0;
    pub const BRICK: f32 = 3.0;
    pub const STONE: f32 = 4.0;
    pub const ROOF: f32 = 5.0;
    pub const WOOD: f32 = 6.0;
    pub const LEAVES: f32 = 7.0;
    pub const CLOTH: f32 = 8.0;
    pub const SKIN: f32 = 9.0;
    pub const DIRT: f32 = 10.0;
    pub const METAL: f32 = 11.0;
    pub const HAY: f32 = 12.0;
    /// Briznas de hierba: se mecen con el viento.
    pub const BLADE: f32 = 13.0;
    /// Sin iluminación: cielo, nubes, destellos, humo, sombras de contacto.
    pub const SKY: f32 = 20.0;
    pub const CLOUD: f32 = 21.0;
    pub const GLOW: f32 = 22.0;
    pub const BLOB: f32 = 23.0;
    pub const SOFT: f32 = 24.0;
}

/// Dirección hacia el sol: tarde de verano, luz baja y cálida.
pub fn sun_dir() -> V3 {
    v3(-0.52, 0.5, 0.69).norm()
}

pub const IDENT: [V3; 3] = [V3::X, V3::UP, V3::Z];

#[derive(Clone, Copy, Debug)]
pub struct Vtx {
    pub p: V3,
    /// Normal (en los billboards: coordenadas locales y un parámetro).
    pub n: V3,
    pub c: Col,
    pub sun: f32,
    pub ao: f32,
    pub m: f32,
}

pub struct Geo {
    pub v: Vec<Vtx>,
    pub i: Vec<u32>,
    /// Material y luz solar que se aplican a lo que se añada a continuación.
    pub mat: f32,
    pub sun: f32,
}

impl Default for Geo {
    fn default() -> Self {
        Self::new()
    }
}

fn rgb(c: [f32; 3]) -> Col {
    [c[0], c[1], c[2], 1.0]
}

fn shade(c: Col, k: f32) -> Col {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

fn mix(a: Col, b: Col, t: f32) -> Col {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t), lerp(a[3], b[3], t)]
}

type Mesh3 = (Vec<V3>, Vec<[u32; 3]>);

fn subdivide(vs: &mut Vec<V3>, fs: &[[u32; 3]]) -> Vec<[u32; 3]> {
    let mut cache = std::collections::HashMap::new();
    let mut mid = |a: u32, b: u32, vs: &mut Vec<V3>| -> u32 {
        let key = (a.min(b), a.max(b));
        *cache.entry(key).or_insert_with(|| {
            vs.push(((vs[a as usize] + vs[b as usize]) * 0.5).norm());
            (vs.len() - 1) as u32
        })
    };
    let mut out = Vec::new();
    for f in fs {
        let a = mid(f[0], f[1], vs);
        let b = mid(f[1], f[2], vs);
        let c = mid(f[2], f[0], vs);
        out.push([f[0], a, c]);
        out.push([f[1], b, a]);
        out.push([f[2], c, b]);
        out.push([a, b, c]);
    }
    out
}

fn icosphere(level: usize) -> Mesh3 {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut vs: Vec<V3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ]
    .iter()
    .map(|&(x, y, z)| v3(x, y, z).norm())
    .collect();
    let mut fs: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..level {
        fs = subdivide(&mut vs, &fs);
    }
    (vs, fs)
}

/// Esfera unitaria: nivel 1 (80 triángulos) o 2 (320, más suave).
fn unit_sphere(level: usize) -> &'static Mesh3 {
    static S1: OnceLock<Mesh3> = OnceLock::new();
    static S2: OnceLock<Mesh3> = OnceLock::new();
    if level >= 2 {
        S2.get_or_init(|| icosphere(2))
    } else {
        S1.get_or_init(|| icosphere(1))
    }
}

/// Ruido de valor 3D suave en [0, 1).
pub fn noise3(p: V3) -> f32 {
    let (ix, iy, iz) = (p.x.floor(), p.y.floor(), p.z.floor());
    let (fx, fy, fz) = (p.x - ix, p.y - iy, p.z - iz);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy, sz) = (s(fx), s(fy), s(fz));
    let h = |dx: f32, dy: f32, dz: f32| hash3(v3(ix + dx, iy + dy, iz + dz) * 0.917);
    let a = lerp(lerp(h(0., 0., 0.), h(1., 0., 0.), sx), lerp(h(0., 1., 0.), h(1., 1., 0.), sx), sy);
    let b = lerp(lerp(h(0., 0., 1.), h(1., 0., 1.), sx), lerp(h(0., 1., 1.), h(1., 1., 1.), sx), sy);
    lerp(a, b, sz)
}

/// Desplazamiento suave que depende solo de la posición: aplicado a todos
/// los vértices, curva paredes y tejados sin abrir grietas entre caras.
pub fn wobble(p: V3, amp: f32) -> V3 {
    let q = p * 0.32;
    v3(noise3(q) - 0.5, noise3(q + v3(17.3, 5.1, 9.7)) - 0.5, noise3(q + v3(3.7, 23.9, 31.1)) - 0.5) * (2.0 * amp)
}

impl Geo {
    pub fn new() -> Geo {
        Geo { v: Vec::with_capacity(4096), i: Vec::with_capacity(8192), mat: m::PLAIN, sun: 1.0 }
    }
    pub fn clear(&mut self) {
        self.v.clear();
        self.i.clear();
        self.mat = m::PLAIN;
        self.sun = 1.0;
    }
    pub fn tris(&self) -> usize {
        self.i.len() / 3
    }
    pub fn set(&mut self, mat: f32) -> &mut Geo {
        self.mat = mat;
        self
    }
    fn vert(&mut self, p: V3, n: V3, c: Col) -> u32 {
        self.v.push(Vtx { p, n, c, sun: self.sun, ao: 1.0, m: self.mat });
        (self.v.len() - 1) as u32
    }

    pub fn tri_n(&mut self, a: V3, b: V3, c: V3, n: V3, col: Col) {
        let i0 = self.vert(a, n, col);
        let i1 = self.vert(b, n, col);
        let i2 = self.vert(c, n, col);
        self.i.extend_from_slice(&[i0, i1, i2]);
    }

    /// Triángulo con normal calculada; `up` orienta la normal hacia ese lado.
    pub fn tri(&mut self, a: V3, b: V3, c: V3, toward: V3, col: Col) {
        let mut n = (b - a).cross(c - a).norm();
        if n.dot(toward) < 0.0 {
            n = -n;
        }
        self.tri_n(a, b, c, n, col);
    }

    pub fn quad_n(&mut self, q: [V3; 4], n: V3, col: Col) {
        let b = self.v.len() as u32;
        for p in q {
            self.vert(p, n, col);
        }
        self.i.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }

    /// Cuadrilátero subdividido en `nu` x `nv` celdas (para que la luz
    /// precalculada tenga detalle en superficies grandes).
    #[allow(clippy::too_many_arguments)]
    pub fn quad_grid(&mut self, o: V3, eu: V3, ev: V3, n: V3, col: Col, nu: usize, nv: usize) {
        let b = self.v.len() as u32;
        for j in 0..=nv {
            for i in 0..=nu {
                let p = o + eu * (i as f32 / nu as f32) + ev * (j as f32 / nv as f32);
                self.vert(p, n, col);
            }
        }
        let w = (nu + 1) as u32;
        for j in 0..nv as u32 {
            for i in 0..nu as u32 {
                let a = b + j * w + i;
                self.i.extend_from_slice(&[a, a + 1, a + w + 1, a, a + w + 1, a + w]);
            }
        }
    }

    /// Caja orientada: centro, ejes (derecha, arriba, adelante) y semitamaños.
    pub fn obox(&mut self, c: V3, ax: [V3; 3], h: [f32; 3], col: Col) {
        self.obox_sub(c, ax, h, col, 0.0);
    }

    /// Caja orientada con caras subdivididas en una cuadrícula global de
    /// `cell` metros: las cajas vecinas comparten los cortes y, al curvarse,
    /// no se abren rendijas entre ellas.
    pub fn obox_sub(&mut self, c: V3, ax: [V3; 3], h: [f32; 3], col: Col, cell: f32) {
        self.obox_sub_ex(c, ax, h, col, cell, &[Vec::new(), Vec::new(), Vec::new()]);
    }

    /// Como `obox_sub`, cortando además por las coordenadas `extra` de cada
    /// eje (los bordes de las cajas vecinas, para no dejar uniones en T).
    pub fn obox_sub_ex(&mut self, c: V3, ax: [V3; 3], h: [f32; 3], col: Col, cell: f32, extra: &[Vec<f32>; 3]) {
        let cuts = |k: usize| -> Vec<f32> {
            let a = c.dot(ax[k]) - h[k];
            let b = c.dot(ax[k]) + h[k];
            let mut pts: Vec<f32> = Vec::new();
            if cell > 0.0 {
                let mut x = (a / cell).floor() * cell + cell;
                while x < b - 0.02 {
                    pts.push(x);
                    x += cell;
                }
            }
            pts.extend(extra[k].iter().copied());
            pts.retain(|&x| x > a + 0.02 && x < b - 0.02);
            pts.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
            pts.dedup_by(|p, q| (*p - *q).abs() < 0.02);
            let mut out = vec![0.0];
            out.extend(pts.iter().map(|&x| (x - a) / (b - a)));
            out.push(1.0);
            out
        };
        let all = [cuts(0), cuts(1), cuts(2)];
        for k in 0..3 {
            let (ui, vi) = ((k + 1) % 3, (k + 2) % 3);
            let n = ax[k];
            let eu = ax[ui] * (2.0 * h[ui]);
            let ev = ax[vi] * (2.0 * h[vi]);
            for s in [1.0f32, -1.0] {
                let o = c + n * (h[k] * s) - eu * 0.5 - ev * 0.5;
                self.quad_cuts(o, eu, ev, n * s, col, &all[ui], &all[vi]);
            }
        }
    }

    /// Caja alineada a los ejes con las caras cortadas en coordenadas
    /// absolutas: los vértices compartidos con las cajas vecinas salen
    /// bit a bit iguales, así que al curvarse no se abren rendijas.
    /// Las celdas de cara pegadas a una caja vecina (`touching`) no se
    /// generan: no se ven y competirían en profundidad con la fachada.
    pub fn aabb_sub(&mut self, b: &Aabb, col: Col, cell: f32, extra: &[Vec<f32>; 3], touching: &[Aabb]) {
        let cuts = |k: usize| -> Vec<f32> {
            let (a, z) = (b.min.get(k), b.max.get(k));
            let mut pts: Vec<f32> = Vec::new();
            if cell > 0.0 {
                let mut x = (a / cell).floor() * cell + cell;
                while x < z - 0.02 {
                    pts.push(x);
                    x += cell;
                }
            }
            pts.extend(extra[k].iter().copied());
            pts.retain(|&x| x > a + 0.02 && x < z - 0.02);
            pts.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
            pts.dedup_by(|p, q| (*p - *q).abs() < 0.02);
            let mut out = vec![a];
            out.extend(pts);
            out.push(z);
            out
        };
        let all = [cuts(0), cuts(1), cuts(2)];
        for k in 0..3 {
            let (ui, vi) = ((k + 1) % 3, (k + 2) % 3);
            for (fixed, sgn) in [(b.max.get(k), 1.0f32), (b.min.get(k), -1.0)] {
                let n = V3::axis(k) * sgn;
                let base = self.v.len() as u32;
                for &cv in &all[vi] {
                    for &cu in &all[ui] {
                        let mut p = V3::ZERO;
                        p.set(k, fixed);
                        p.set(ui, cu);
                        p.set(vi, cv);
                        self.vert(p, n, col);
                    }
                }
                let w = all[ui].len() as u32;
                for j in 0..(all[vi].len() - 1) as u32 {
                    for i in 0..(all[ui].len() - 1) as u32 {
                        let mut c = n * 0.01;
                        c.set(k, fixed + sgn * 0.01);
                        c.set(ui, (all[ui][i as usize] + all[ui][i as usize + 1]) * 0.5);
                        c.set(vi, (all[vi][j as usize] + all[vi][j as usize + 1]) * 0.5);
                        if touching.iter().any(|o| o.contains(c)) {
                            continue;
                        }
                        let a = base + j * w + i;
                        self.i.extend_from_slice(&[a, a + 1, a + w + 1, a, a + w + 1, a + w]);
                    }
                }
            }
        }
    }

    /// Cuadrilátero cortado en las fracciones dadas de cada lado.
    #[allow(clippy::too_many_arguments)]
    pub fn quad_cuts(&mut self, o: V3, eu: V3, ev: V3, n: V3, col: Col, us: &[f32], vs: &[f32]) {
        let b = self.v.len() as u32;
        for &fv in vs {
            for &fu in us {
                self.vert(o + eu * fu + ev * fv, n, col);
            }
        }
        let w = us.len() as u32;
        for j in 0..(vs.len() - 1) as u32 {
            for i in 0..(us.len() - 1) as u32 {
                let a = b + j * w + i;
                self.i.extend_from_slice(&[a, a + 1, a + w + 1, a, a + w + 1, a + w]);
            }
        }
    }

    pub fn aabb(&mut self, b: &Aabb, col: Col) {
        let s = b.size() * 0.5;
        self.obox(b.center(), IDENT, [s.x, s.y, s.z], col);
    }

    pub fn cube(&mut self, c: V3, size: f32, col: Col) {
        let h = size * 0.5;
        self.obox(c, IDENT, [h, h, h], col);
    }

    /// Esfera (o elipsoide) de normales suaves.
    pub fn sphere(&mut self, c: V3, r: V3, col: Col) {
        self.sphere_ex(c, r, col, 1, None);
    }

    /// Esfera más redonda (para personajes y primeros planos).
    pub fn sphere_hi(&mut self, c: V3, r: V3, col: Col) {
        self.sphere_ex(c, r, col, 2, None);
    }

    /// Esfera con nivel de detalle y, opcionalmente, normales "compartidas":
    /// se mezclan con la dirección desde `nc.0`, para que un grupo de bolas
    /// se ilumine como un solo volumen esponjoso (copas, setos).
    pub fn sphere_ex(&mut self, c: V3, r: V3, col: Col, level: usize, nc: Option<(V3, f32)>) {
        let (vs, fs) = unit_sphere(level);
        let b = self.v.len() as u32;
        for &u in vs {
            let p = c + v3(u.x * r.x, u.y * r.y, u.z * r.z);
            let mut n = v3(u.x / r.x, u.y / r.y, u.z / r.z).norm();
            if let Some((center, k)) = nc {
                n = (n * (1.0 - k) + (p - center).norm() * k).norm();
            }
            self.vert(p, n, col);
        }
        for f in fs {
            self.i.extend_from_slice(&[b + f[0], b + f[1], b + f[2]]);
        }
    }

    /// Cilindro (o tronco de cono) de `a` a `b`, con normales suaves.
    #[allow(clippy::too_many_arguments)]
    pub fn cylinder(&mut self, a: V3, b: V3, r0: f32, r1: f32, sides: usize, col: Col, caps: bool) {
        let axis = (b - a).norm();
        let helper = if axis.y.abs() < 0.9 { V3::UP } else { V3::X };
        let u = axis.cross(helper).norm();
        let w = axis.cross(u).norm();
        let base = self.v.len() as u32;
        let slope = (r0 - r1) / (b - a).len().max(1e-4);
        for k in 0..=sides {
            let ang = k as f32 / sides as f32 * TAU;
            let d = u * ang.cos() + w * ang.sin();
            let n = (d + axis * slope).norm();
            self.vert(a + d * r0, n, col);
            self.vert(b + d * r1, n, col);
        }
        for k in 0..sides as u32 {
            let i = base + k * 2;
            self.i.extend_from_slice(&[i, i + 2, i + 3, i, i + 3, i + 1]);
        }
        if caps {
            for (p, r, n) in [(a, r0, -axis), (b, r1, axis)] {
                let c = self.vert(p, n, col);
                let first = self.v.len() as u32;
                for k in 0..=sides {
                    let ang = k as f32 / sides as f32 * TAU;
                    self.vert(p + (u * ang.cos() + w * ang.sin()) * r, n, col);
                }
                for k in 0..sides as u32 {
                    self.i.extend_from_slice(&[c, first + k, first + k + 1]);
                }
            }
        }
    }

    /// Pirámide de base rectangular (en el plano horizontal) y vértice `top`.
    pub fn pyramid(&mut self, base: &Aabb, top: V3, col: Col) {
        let y = base.min.y;
        let c = [
            v3(base.min.x, y, base.min.z),
            v3(base.max.x, y, base.min.z),
            v3(base.max.x, y, base.max.z),
            v3(base.min.x, y, base.max.z),
        ];
        let mid = (c[0] + c[2]) * 0.5;
        for k in 0..4 {
            let (a, b) = (c[k], c[(k + 1) % 4]);
            let out = ((a + b) * 0.5 - mid).norm();
            self.tri(a, b, top, out + V3::UP * 0.3, col);
        }
    }

    /// Cuadrado orientado a la cámara. En la normal guarda las coordenadas
    /// locales (-1..1) y un parámetro libre para el sombreador.
    pub fn billboard(&mut self, c: V3, right: V3, up: V3, size: f32, col: Col, param: f32) {
        let b = self.v.len() as u32;
        for (u, w) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = c + right * (u * size) + up * (w * size);
            self.vert(p, v3(u, w, param), col);
        }
        self.i.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }

    /// Mancha de sombra suave en el suelo, bajo un personaje u objeto.
    pub fn blob_shadow(&mut self, pos: V3, r: f32, alpha: f32) {
        let old = self.mat;
        self.mat = m::BLOB;
        let b = self.v.len() as u32;
        for (u, w) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = v3(pos.x + u * r, pos.y + 0.025, pos.z + w * r);
            self.vert(p, v3(u, w, 0.0), [0.08, 0.10, 0.22, alpha]);
        }
        self.i.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        self.mat = old;
    }

    pub fn append(&mut self, o: &Geo) {
        let b = self.v.len() as u32;
        self.v.extend_from_slice(&o.v);
        self.i.extend(o.i.iter().map(|i| i + b));
    }
}

// ----------------------------------------------------------------------
// Ruido determinista
// ----------------------------------------------------------------------

fn hash2(x: i32, z: i32) -> f32 {
    hash3(v3(x as f32 * 1.37, 0.73, z as f32 * 0.91))
}

/// Ruido de valor 2D suave en [0, 1).
pub fn noise2(x: f32, z: f32) -> f32 {
    let (ix, iz) = (x.floor() as i32, z.floor() as i32);
    let (fx, fz) = (x - x.floor(), z - z.floor());
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let a = lerp(hash2(ix, iz), hash2(ix + 1, iz), sx);
    let b = lerp(hash2(ix, iz + 1), hash2(ix + 1, iz + 1), sx);
    lerp(a, b, sz)
}

/// Altura del terreno: llano en el pueblo, colinas suaves alrededor.
pub fn ground_height(x: f32, z: f32) -> f32 {
    let r = x.abs().max(z.abs());
    if r <= 72.0 {
        return 0.0;
    }
    let k = ((r - 72.0) / 60.0).clamp(0.0, 1.0);
    let k = k * k * (3.0 - 2.0 * k);
    let hills = 7.0 * (x * 0.021 + 1.3).sin() * (z * 0.017).cos()
        + 4.5 * (x * 0.043 + z * 0.031).sin()
        + 6.0 * noise2(x * 0.03, z * 0.03);
    let far = ((r - 170.0) / 150.0).clamp(0.0, 1.0);
    k * (5.0 + hills) + far * far * 38.0
}

// ----------------------------------------------------------------------
// Luz precalculada: sombras del sol y oclusión ambiental
// ----------------------------------------------------------------------

/// Ocultadores para los rayos de sombra, con una rejilla 2D que acelera.
pub struct Occluders {
    boxes: Vec<Aabb>,
    spheres: Vec<(V3, f32)>,
    cells: Vec<Vec<u32>>,
    stamp: Vec<u32>,
    ray: u32,
}

const GRID_MIN: f32 = -84.0;
const GRID_CELL: f32 = 4.0;
const GRID_N: usize = 42;

impl Occluders {
    pub fn new(boxes: Vec<Aabb>, spheres: Vec<(V3, f32)>) -> Occluders {
        let mut cells = vec![Vec::new(); GRID_N * GRID_N];
        let n = boxes.len();
        let cell_of = |x: f32| (((x - GRID_MIN) / GRID_CELL).floor() as i32).clamp(0, GRID_N as i32 - 1) as usize;
        let mut put = |id: u32, min: V3, max: V3| {
            for ix in cell_of(min.x)..=cell_of(max.x) {
                for iz in cell_of(min.z)..=cell_of(max.z) {
                    cells[iz * GRID_N + ix].push(id);
                }
            }
        };
        for (i, b) in boxes.iter().enumerate() {
            put(i as u32, b.min, b.max);
        }
        for (i, &(c, r)) in spheres.iter().enumerate() {
            put((n + i) as u32, c - v3(r, r, r), c + v3(r, r, r));
        }
        let total = boxes.len() + spheres.len();
        Occluders { boxes, spheres, cells, stamp: vec![0; total], ray: 0 }
    }

    fn test(&self, id: u32, o: V3, d: V3, max_t: f32) -> bool {
        let id = id as usize;
        if id < self.boxes.len() {
            return self.boxes[id].ray(o, d, max_t).is_some();
        }
        let (c, r) = self.spheres[id - self.boxes.len()];
        let oc = o - c;
        let b = oc.dot(d);
        let cc = oc.dot(oc) - r * r;
        let disc = b * b - cc;
        if disc < 0.0 {
            return false;
        }
        let t = -b - disc.sqrt();
        let t2 = -b + disc.sqrt();
        (t > 0.0 && t < max_t) || (t <= 0.0 && t2 > 0.0)
    }

    /// ¿Hay algo entre `o` y `o + d * max_t`? (`d` normalizado)
    pub fn hit(&mut self, o: V3, d: V3, max_t: f32) -> bool {
        if d.y < -1e-4 && o.y > 0.0 && -o.y / d.y < max_t {
            return true;
        }
        self.ray = self.ray.wrapping_add(1);
        let ray = self.ray;
        let size = GRID_CELL * GRID_N as f32;
        // Recortar el rayo a la rejilla
        let (mut t0, mut t1) = (0.0f32, max_t);
        for (oa, da) in [(o.x, d.x), (o.z, d.z)] {
            if da.abs() < 1e-8 {
                if oa < GRID_MIN || oa > GRID_MIN + size {
                    return false;
                }
            } else {
                let (mut a, mut b) = ((GRID_MIN - oa) / da, (GRID_MIN + size - oa) / da);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
            }
        }
        if t0 > t1 {
            return false;
        }
        let p = o + d * t0;
        let mut ix = (((p.x - GRID_MIN) / GRID_CELL).floor() as i32).clamp(0, GRID_N as i32 - 1);
        let mut iz = (((p.z - GRID_MIN) / GRID_CELL).floor() as i32).clamp(0, GRID_N as i32 - 1);
        let step = |dv: f32| if dv > 0.0 { 1 } else { -1 };
        let (sx, sz) = (step(d.x), step(d.z));
        let next = |i: i32, s: i32, ov: f32, dv: f32| {
            if dv.abs() < 1e-8 {
                f32::INFINITY
            } else {
                let edge = GRID_MIN + (i + if s > 0 { 1 } else { 0 }) as f32 * GRID_CELL;
                (edge - ov) / dv
            }
        };
        let mut tx = next(ix, sx, o.x, d.x);
        let mut tz = next(iz, sz, o.z, d.z);
        let dtx = if d.x.abs() < 1e-8 { f32::INFINITY } else { GRID_CELL / d.x.abs() };
        let dtz = if d.z.abs() < 1e-8 { f32::INFINITY } else { GRID_CELL / d.z.abs() };
        loop {
            let cell = iz as usize * GRID_N + ix as usize;
            for k in 0..self.cells[cell].len() {
                let id = self.cells[cell][k];
                if self.stamp[id as usize] == ray {
                    continue;
                }
                self.stamp[id as usize] = ray;
                if self.test(id, o, d, max_t) {
                    return true;
                }
            }
            let t = tx.min(tz);
            if t > t1 || (d.y > 0.0 && o.y + d.y * t > 30.0) {
                return false;
            }
            if tx < tz {
                ix += sx;
                tx += dtx;
            } else {
                iz += sz;
                tz += dtz;
            }
            if ix < 0 || iz < 0 || ix >= GRID_N as i32 || iz >= GRID_N as i32 {
                return false;
            }
        }
    }

    /// Fracción de luz solar que llega a un punto (penumbra suave con 3 rayos).
    pub fn sun_at(&mut self, p: V3, n: V3) -> f32 {
        let s = sun_dir();
        if n.dot(s) <= 0.0 {
            return 0.0;
        }
        let r = V3::right_of_yaw(s.yaw()) * 0.035;
        let o = p + n * 0.06;
        let mut lit = 0.0;
        for d in [s, (s + r).norm(), (s - r + V3::UP * 0.03).norm()] {
            if !self.hit(o, d, 70.0) {
                lit += 1.0;
            }
        }
        lit / 3.0
    }

    /// Cuánto cielo ve un punto (1 = todo, 0 = encerrado).
    pub fn ao_at(&mut self, p: V3, n: V3) -> f32 {
        let t = if n.y.abs() < 0.9 { V3::UP } else { V3::X };
        let u = n.cross(t).norm();
        let w = n.cross(u);
        let o = p + n * 0.05;
        let mut open = 0.0;
        let dirs = 7;
        for k in 0..dirs {
            let a = k as f32 / dirs as f32 * TAU + 0.4;
            let el = if k % 2 == 0 { 0.35 } else { 0.75 };
            let d = (n * el + (u * a.cos() + w * a.sin()) * (1.0 - el * el).sqrt()).norm();
            if !self.hit(o, d, 2.6) {
                open += 1.0;
            }
        }
        // Mirar también hacia arriba para captar techos y aleros
        if !self.hit(o, (n + V3::UP * 2.0).norm(), 6.0) {
            open += 1.0;
        }
        open / (dirs + 1) as f32
    }
}

/// Precalcula sol y oclusión de los vértices `from..` de `g`.
pub fn bake(g: &mut Geo, occ: &mut Occluders, from: usize) {
    for k in from..g.v.len() {
        let vx = g.v[k];
        if vx.m >= m::SKY {
            continue;
        }
        let sun = occ.sun_at(vx.p, vx.n);
        let ao = occ.ao_at(vx.p, vx.n);
        g.v[k].sun = sun;
        g.v[k].ao = ao.max(0.3);
    }
}

// ----------------------------------------------------------------------
// Paleta
// ----------------------------------------------------------------------

const MEADOW: [f32; 3] = [0.40, 0.57, 0.29];
const ROAD: [f32; 3] = [0.70, 0.60, 0.43];
const TIMBER: [f32; 3] = [0.33, 0.22, 0.15];
const FRAME: [f32; 3] = [0.93, 0.91, 0.84];
const SHUTTERS: [[f32; 3]; 4] = [[0.24, 0.52, 0.55], [0.40, 0.58, 0.36], [0.30, 0.44, 0.70], [0.66, 0.30, 0.25]];
const FLOWERS: [[f32; 3]; 5] = [[0.95, 0.30, 0.30], [0.98, 0.84, 0.30], [0.96, 0.60, 0.72], [0.95, 0.95, 0.90], [0.62, 0.70, 0.98]];
const ROOFS: [[f32; 3]; 3] = [[0.74, 0.37, 0.25], [0.62, 0.30, 0.22], [0.36, 0.40, 0.50]];

fn mat_of(mt: Mat) -> (f32, [f32; 3]) {
    match mt {
        Mat::Ground => (m::GRASS, MEADOW),
        Mat::Stone => (m::STONE, [0.66, 0.63, 0.56]),
        Mat::Plaster => (m::PLASTER, [0.95, 0.91, 0.82]),
        Mat::Brick => (m::BRICK, [0.70, 0.42, 0.32]),
        Mat::Wood => (m::WOOD, [0.58, 0.42, 0.26]),
        Mat::Sandbag => (m::CLOTH, [0.72, 0.65, 0.48]),
        Mat::Hedge => (m::LEAVES, [0.24, 0.42, 0.20]),
        Mat::Metal => (m::METAL, [0.36, 0.40, 0.34]),
        Mat::Roof => (m::ROOF, ROOFS[0]),
        Mat::Bark => (m::WOOD, [0.40, 0.29, 0.20]),
        Mat::Leaves => (m::LEAVES, [0.30, 0.52, 0.22]),
        Mat::Hay => (m::HAY, [0.86, 0.74, 0.44]),
    }
}

// ----------------------------------------------------------------------
// Construcción del mundo estático
// ----------------------------------------------------------------------

pub struct WorldGeo {
    /// Trozos del escenario con su caja envolvente (para descartar los no visibles).
    pub chunks: Vec<(Aabb, Geo)>,
    /// Hierba y flores: solo se dibujan cerca de la cámara.
    pub grass: Vec<(Aabb, Geo)>,
}

/// Construye y precalcula la geometría estática del mapa (una sola vez).
pub fn build_world(w: &World) -> WorldGeo {
    build_world_skip(w, &[])
}

/// Nombre del rol del catálogo de modelos que puede sustituir a un objeto.
pub fn prop_role(p: &Prop) -> Option<&'static str> {
    Some(match p {
        Prop::Tree { .. } => "arbol",
        Prop::Tank(_) => "tanque",
        Prop::Cart(_) => "carro",
        Prop::Barrel(_) => "barril",
        Prop::Hay(_) => "paja",
        _ => return None,
    })
}

/// Como `build_world`, sin dibujar los objetos cuyos roles estén en `skip`
/// (porque los sustituye un modelo de artista).
pub fn build_world_skip(w: &World, skip: &[&str]) -> WorldGeo {
    let mut g = Geo::new();
    let mut rng = Rng::new(1944);

    // Ocultadores: todos los bloques salvo las copas (que van como esferas)
    // Las cajas ocultas envuelven a su forma visible y la dejarían a oscuras:
    // se sustituyen por ocultadores más pequeños que caben dentro de ella.
    let mut boxes = Vec::new();
    let mut spheres = Vec::new();
    for b in &w.blocks {
        if !b.hidden {
            boxes.push(b.b);
        } else if matches!(b.mat, Mat::Sandbag | Mat::Hay | Mat::Metal) {
            let k = v3(0.14, 0.14, 0.14);
            boxes.push(Aabb { min: b.b.min + k, max: b.b.max - k });
        }
    }
    for p in &w.props {
        match p {
            Prop::Roof { x0, x1, z0, z1, y, .. } => {
                boxes.push(Aabb::new(v3(*x0, *y - 0.32, *z0), v3(*x1, *y - 0.2, *z1)));
            }
            Prop::Hedge(b) => {
                let k = 0.5;
                boxes.push(Aabb::new(b.min + v3(k, 0.0, k), b.max - v3(k, 0.8, k)));
            }
            _ => {}
        }
    }
    let mut tree_blobs: Vec<Vec<(V3, V3, Col)>> = Vec::new();
    for p in &w.props {
        if let Prop::Tree { pos, size } = p {
            let blobs = tree_canopy(*pos, *size, &mut rng);
            for (c, r, _) in &blobs {
                spheres.push((*c, r.x.max(r.y) * 0.85));
            }
            tree_blobs.push(blobs);
        }
    }
    // Las bolas de los setos también dan sombra (algo menores que las visibles)
    for p in &w.props {
        if let Prop::Hedge(b) = p {
            let along_x = b.size().x > b.size().z;
            let len = if along_x { b.size().x } else { b.size().z };
            let thick = if along_x { b.size().z } else { b.size().x };
            let count = (len / 1.4).ceil() as usize;
            for k in 0..count {
                let t = (k as f32 + 0.5) / count as f32;
                let c = if along_x { v3(lerp(b.min.x, b.max.x, t), b.max.y - 0.4, b.center().z) } else { v3(b.center().x, b.max.y - 0.4, lerp(b.min.z, b.max.z, t)) };
                spheres.push((c, thick * 0.42));
            }
        }
    }
    let mut occ = Occluders::new(boxes, spheres);

    // Suelo del pueblo: rejilla fina con vértices compartidos
    g.set(m::GRASS);
    let step = 2.0;
    let n = 72;
    let base = g.v.len() as u32;
    for j in 0..=n {
        for i in 0..=n {
            let (x, z) = (-72.0 + i as f32 * step, -72.0 + j as f32 * step);
            let k = 0.88 + 0.22 * noise2(x * 0.07, z * 0.07) + 0.08 * hash2(i, j);
            let yellow = ((noise2(x * 0.025 + 7.0, z * 0.025) - 0.55) * 3.0).clamp(0.0, 1.0);
            let c = mix(rgb(MEADOW), rgb([0.62, 0.68, 0.32]), yellow * 0.6);
            g.vert(v3(x, 0.0, z), V3::UP, shade(c, k));
        }
    }
    let wn = (n + 1) as u32;
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let a = base + j * wn + i;
            g.i.extend_from_slice(&[a, a + wn + 1, a + 1, a, a + wn, a + wn + 1]);
        }
    }

    // Caminos de tierra con bordes irregulares
    g.set(m::DIRT);
    let road = |g: &mut Geo, along_x: bool, c0: f32, c1: f32, from: f32, to: f32, y: f32| {
        let mut s = from;
        while s < to {
            let e = (s + 1.0).min(to);
            let edge = |t: f32, side: f32| {
                let wob = (noise2(t * 0.35, side * 9.0 + c0) - 0.5) * 0.9;
                if side < 0.0 {
                    c0 + wob
                } else {
                    c1 + wob
                }
            };
            let pt = |t: f32, q: f32| if along_x { v3(t, y, q) } else { v3(q, y, t) };
            let col = shade(rgb(ROAD), 0.94 + 0.12 * noise2(s * 0.2, c0));
            g.quad_n([pt(s, edge(s, -1.0)), pt(e, edge(e, -1.0)), pt(e, edge(e, 1.0)), pt(s, edge(s, 1.0))], V3::UP, col);
            s = e;
        }
    };
    road(&mut g, false, -3.4, 3.4, -63.0, 63.0, 0.02);
    road(&mut g, true, 2.1, 7.9, -63.0, 63.0, 0.035);

    // Bloques visibles (muros, cajas, tanque...)
    for b in &w.blocks {
        if b.hidden {
            continue;
        }
        let (mt, col) = mat_of(b.mat);
        let tint = if b.mat == Mat::Plaster { 0.96 + 0.06 * (b.tint - 0.88) } else { b.tint };
        g.set(mt);
        let s = b.b.size() * 0.5;
        let start = g.v.len();
        let cell = if matches!(b.mat, Mat::Stone | Mat::Plaster | Mat::Brick) { 0.7 } else { 1.6 };
        // Bordes de las cajas que tocan a esta: también se corta por ellos
        let mut extra = [Vec::new(), Vec::new(), Vec::new()];
        let mut touching = Vec::new();
        let grown = Aabb { min: b.b.min - v3(0.02, 0.02, 0.02), max: b.b.max + v3(0.02, 0.02, 0.02) };
        for o in &w.blocks {
            if o.hidden || std::ptr::eq(o, b) || !grown.overlaps(&o.b) {
                continue;
            }
            for (k, e) in extra.iter_mut().enumerate() {
                e.push(o.b.min.get(k));
                e.push(o.b.max.get(k));
            }
            touching.push(o.b);
        }
        let _ = s;
        g.aabb_sub(&b.b, shade(rgb(col), tint), cell, &extra, &touching);
        if b.mat == Mat::Stone && b.b.min.y < 0.1 && (b.ruin || b.b.size().y < 1.5) {
            // Borde superior roto en las ruinas e irregular en los muretes
            let tall = b.ruin;
            for vx in &mut g.v[start..] {
                if vx.p.y > b.b.max.y - 0.01 {
                    let k = if tall { 0.15 + 0.75 * noise2(vx.p.x * 1.1 + 3.0, vx.p.z * 1.1) } else { 0.12 * noise2(vx.p.x * 2.0, vx.p.z * 2.0) };
                    vx.p.y -= k.min(b.b.size().y - 0.3);
                }
            }
        }
        if b.mat == Mat::Wood && (s.x - 0.45).abs() < 0.01 && (s.y - 0.45).abs() < 0.01 {
            crate_frame(&mut g, &b.b, shade(rgb(col), tint * 0.72));
        }
    }

    // Elementos con forma propia
    let mut trees = tree_blobs.into_iter();
    for p in &w.props {
        if prop_role(p).map(|r| skip.contains(&r)).unwrap_or(false) {
            if matches!(p, Prop::Tree { .. }) {
                trees.next();
            }
            continue;
        }
        match p {
            Prop::Roof { x0, x1, z0, z1, y, rise, ridge_x, color } => {
                roof(&mut g, *x0, *x1, *z0, *z1, *y, *rise, *ridge_x, rgb(ROOFS[*color as usize % 3]))
            }
            Prop::Wall { along_x, a0, a1, face, n, h, ops, timber, style } => {
                wall_decor(&mut g, *along_x, *a0, *a1, *face, *n, *h, ops, *timber, *style, &mut rng)
            }
            Prop::Tree { pos, size } => {
                g.set(m::WOOD);
                let bark = rgb([0.42, 0.31, 0.22]);
                // Tronco algo torcido, con raíces que se abren
                let mid = *pos + v3(0.15, 2.4, -0.1);
                g.cylinder(*pos, mid, 0.36, 0.26, 9, bark, false);
                g.cylinder(mid, *pos + V3::UP * 4.8, 0.26, 0.18, 9, bark, false);
                g.cylinder(*pos + V3::UP * 3.0, *pos + v3(1.0, 4.4, 0.5), 0.13, 0.07, 6, bark, false);
                g.cylinder(*pos + V3::UP * 3.4, *pos + v3(-0.9, 4.6, -0.4), 0.11, 0.06, 6, bark, false);
                for k in 0..4 {
                    let a = k as f32 * 1.7 + 0.4;
                    let d = v3(a.cos(), 0.0, a.sin());
                    g.cylinder(*pos + V3::UP * 0.5 + d * 0.1, *pos + d * 0.75 - V3::UP * 0.05, 0.13, 0.05, 6, bark, false);
                }
                g.set(m::LEAVES);
                let center = *pos + V3::UP * (5.6 * size);
                if let Some(blobs) = trees.next() {
                    for (c, r, col) in blobs {
                        g.sphere_ex(c, r, col, 2, Some((center, 0.7)));
                    }
                }
            }
            Prop::Hedge(b) => hedge(&mut g, b, &mut rng),
            Prop::Sandbags(b) => sandbags(&mut g, b, &mut rng),
            Prop::Hay(pos) => {
                g.set(m::HAY);
                let a = *pos + v3(-0.6, 0.62, 0.0);
                g.cylinder(a, a + V3::X * 1.2, 0.62, 0.62, 12, rgb([0.86, 0.74, 0.44]), true);
            }
            Prop::Tank(c) => tank(&mut g, *c),
            Prop::Glass { a, b, n } => {
                // Vidriera de colores a ras de fachada (por fuera y por dentro),
                // con marco de piedra clara y parteluz
                let glass = [[0.36, 0.52, 0.80], [0.86, 0.62, 0.30], [0.72, 0.30, 0.32], [0.40, 0.66, 0.52], [0.92, 0.84, 0.52]];
                let half_t = 0.25;
                let rows = 6;
                g.set(m::GLOW);
                for side in [1.0f32, -1.0] {
                    let off = *n * ((half_t + 0.012) * side);
                    for k in 0..rows {
                        for c2 in 0..2 {
                            let (y0, y1) = (lerp(a.y, b.y, k as f32 / rows as f32), lerp(a.y, b.y, (k + 1) as f32 / rows as f32));
                            let (z0, z1) = (lerp(a.z, b.z, c2 as f32 / 2.0), lerp(a.z, b.z, (c2 + 1) as f32 / 2.0));
                            let col = glass[(k * 2 + c2 + (a.z * 3.0) as usize) % glass.len()];
                            let col = [col[0] * 0.85, col[1] * 0.85, col[2] * 0.85, 1.0];
                            g.quad_n([v3(a.x, y0, z0) + off, v3(a.x, y0, z1) + off, v3(a.x, y1, z1) + off, v3(a.x, y1, z0) + off], *n * side, col);
                        }
                    }
                }
                g.set(m::STONE);
                let fr = rgb([0.86, 0.82, 0.72]);
                let cz = (a.z + b.z) * 0.5;
                let hz = (b.z - a.z) * 0.5;
                let cy = (a.y + b.y) * 0.5;
                let hy = (b.y - a.y) * 0.5;
                let lead = rgb([0.25, 0.24, 0.24]);
                for side in [1.0f32, -1.0] {
                    let x = a.x + n.x * (half_t + 0.03) * side;
                    g.obox(v3(x, cy, cz), IDENT, [0.03, hy, 0.025], lead);
                    for sz in [-1.0f32, 1.0] {
                        g.obox(v3(x, cy, cz + sz * (hz + 0.1)), IDENT, [0.05, hy + 0.12, 0.1], fr);
                    }
                    g.obox(v3(x, b.y + 0.12, cz), IDENT, [0.05, 0.12, hz + 0.2], fr);
                    g.obox(v3(x + n.x * 0.04 * side, a.y - 0.07, cz), IDENT, [0.09, 0.07, hz + 0.22], fr);
                    for k in 1..rows {
                        let y = lerp(a.y, b.y, k as f32 / rows as f32);
                        g.obox(v3(x, y, cz), IDENT, [0.02, 0.012, hz], lead);
                    }
                }
            }
            Prop::Cart(c) => {
                let wood = rgb([0.55, 0.38, 0.23]);
                g.set(m::WOOD);
                g.obox(*c + v3(0.0, 0.62, 0.0), IDENT, [1.1, 0.06, 0.62], wood);
                for sz in [-1.0f32, 1.0] {
                    g.obox(*c + v3(0.0, 0.82, sz * 0.6), IDENT, [1.1, 0.18, 0.03], shade(wood, 0.9));
                    for sx in [-0.6f32, 0.6] {
                        let a = *c + v3(sx, 0.42, sz * 0.72);
                        g.cylinder(a - V3::Z * 0.05 * sz, a + V3::Z * 0.05 * sz, 0.42, 0.42, 12, shade(wood, 0.8), true);
                    }
                }
                g.cylinder(*c + v3(-1.1, 0.6, -0.25), *c + v3(-2.2, 0.35, -0.25), 0.04, 0.04, 6, wood, false);
                g.cylinder(*c + v3(-1.1, 0.6, 0.25), *c + v3(-2.2, 0.35, 0.25), 0.04, 0.04, 6, wood, false);
                g.set(m::HAY);
                g.sphere(*c + v3(0.0, 0.95, 0.0), v3(1.0, 0.38, 0.55), rgb([0.86, 0.74, 0.44]));
            }
            Prop::Barrel(pos) => {
                g.set(m::METAL);
                let col = rgb([0.34, 0.42, 0.36]);
                g.cylinder(*pos, *pos + V3::UP * 0.9, 0.3, 0.3, 10, col, true);
                for y in [0.15, 0.75] {
                    g.cylinder(*pos + V3::UP * y, *pos + V3::UP * (y + 0.05), 0.315, 0.315, 10, shade(col, 0.8), false);
                }
            }
            Prop::Spire { base, top } => {
                g.set(m::ROOF);
                let c = base.center();
                g.pyramid(base, v3(c.x, *top, c.z), rgb([0.30, 0.52, 0.50]));
                g.set(m::METAL);
                g.cylinder(v3(c.x, *top - 0.2, c.z), v3(c.x, *top + 1.2, c.z), 0.04, 0.03, 5, rgb([0.25, 0.25, 0.25]), false);
                // Veleta: un gallo esquemático
                g.obox(v3(c.x + 0.1, *top + 1.0, c.z), IDENT, [0.28, 0.12, 0.02], rgb([0.25, 0.25, 0.25]));
            }
        }
    }

    // Pozo con tejadillo
    g.set(m::ROOF);
    let wc = v3(-6.0, 0.0, 24.0);
    roof(&mut g, wc.x - 1.2, wc.x + 1.2, wc.z - 0.6, wc.z + 0.6, 2.55, 0.6, true, rgb(ROOFS[1]));

    // Hiedra, matorrales y flores al pie de los muros
    greenery(&mut g, w, &mut rng);

    let static_end = g.v.len();
    bake(&mut g, &mut occ, 0);
    // Nada es del todo recto: se curvan muros, tejados y objetos (la luz ya
    // está calculada, así que las superficies no se tapan a sí mismas)
    for vx in &mut g.v[..static_end] {
        let k = (vx.p.y / 0.6).clamp(0.0, 1.0);
        if k > 0.0 {
            let amp = if vx.m == m::ROOF {
                0.1
            } else if vx.m == m::STONE || vx.m == m::BRICK {
                0.04
            } else {
                0.055
            };
            let mut d = wobble(vx.p, amp) * k;
            d.y *= 0.6;
            vx.p += d;
        }
    }

    // Campos y colinas lejanas (sin sombras precalculadas)
    let far_start = g.v.len();
    countryside(&mut g, &mut rng);
    for k in far_start..g.v.len() {
        let vx = g.v[k];
        g.v[k].ao = (0.75 + 0.25 * vx.n.y).clamp(0.0, 1.0);
        g.v[k].sun = if vx.n.dot(sun_dir()) > 0.0 { 1.0 } else { 0.0 };
    }

    if std::env::var("DEIA_CAPTURA").is_ok() {
        println!("escenario: {} vértices, {} triángulos", g.v.len(), g.tris());
    }
    let chunks = split_tiles(&g, 16.0);

    // Hierba y flores
    let mut grass = Geo::new();
    meadow(&mut grass, w, &mut occ, &mut rng);
    if std::env::var("DEIA_CAPTURA").is_ok() {
        println!("hierba: {} vértices, {} triángulos", grass.v.len(), grass.tris());
    }
    let grass = split_tiles(&grass, 16.0);
    WorldGeo { chunks, grass }
}

/// Divide la geometría en losetas por la posición de cada triángulo.
pub fn split_tiles(g: &Geo, tile: f32) -> Vec<(Aabb, Geo)> {
    use std::collections::HashMap;
    let mut map: HashMap<(i32, i32), (Geo, Vec<u32>)> = HashMap::new();
    for t in g.i.chunks(3) {
        let c = (g.v[t[0] as usize].p + g.v[t[1] as usize].p + g.v[t[2] as usize].p) / 3.0;
        let big = c.x.abs().max(c.z.abs()) > 80.0;
        let size = if big { tile * 5.0 } else { tile };
        let key = ((c.x / size).floor() as i32 + if big { 10_000 } else { 0 }, (c.z / size).floor() as i32);
        let entry = map.entry(key).or_insert_with(|| (Geo::new(), vec![u32::MAX; g.v.len()]));
        let (geo, remap) = entry;
        for &i in t {
            if remap[i as usize] == u32::MAX {
                geo.v.push(g.v[i as usize]);
                remap[i as usize] = (geo.v.len() - 1) as u32;
            }
            geo.i.push(remap[i as usize]);
        }
    }
    let mut out: Vec<(Aabb, Geo)> = map
        .into_values()
        .map(|(geo, _)| {
            let mut min = v3(f32::MAX, f32::MAX, f32::MAX);
            let mut max = v3(f32::MIN, f32::MIN, f32::MIN);
            for vx in &geo.v {
                min = v3(min.x.min(vx.p.x), min.y.min(vx.p.y), min.z.min(vx.p.z));
                max = v3(max.x.max(vx.p.x), max.y.max(vx.p.y), max.z.max(vx.p.z));
            }
            (Aabb { min, max }, geo)
        })
        .collect();
    out.sort_by(|a, b| a.0.min.x.partial_cmp(&b.0.min.x).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn tree_canopy(pos: V3, size: f32, rng: &mut Rng) -> Vec<(V3, V3, Col)> {
    // Copa como una nube de hojas: núcleo grande y racimos alrededor, más
    // claros arriba (donde da el sol) y más oscuros abajo
    let mut blobs = Vec::new();
    let center = pos + V3::UP * (5.6 * size);
    let dark = [0.24, 0.44, 0.20];
    let mid = [0.32, 0.55, 0.24];
    let light = [0.50, 0.68, 0.30];
    blobs.push((center, v3(2.4, 1.9, 2.4) * size, rgb(mid)));
    for k in 0..16 {
        let a = k as f32 * 2.399 + rng.range(-0.2, 0.2);
        let y = rng.range(-0.8, 1.0);
        let ring = (1.0 - y * y).max(0.2).sqrt();
        let d = rng.range(1.7, 2.3) * size * ring;
        let c = center + v3(a.cos() * d, y * 1.7 * size, a.sin() * d);
        let r = rng.range(0.9, 1.35) * size;
        let t = ((y + 0.8) / 1.8).clamp(0.0, 1.0);
        let col = if t > 0.6 { mix(rgb(mid), rgb(light), (t - 0.6) * 2.5) } else { mix(rgb(dark), rgb(mid), t / 0.6) };
        blobs.push((c, v3(r, r * 0.85, r), shade(col, rng.range(0.94, 1.06))));
    }
    for _ in 0..4 {
        let c = center + v3(rng.range(-0.8, 0.8), 1.9, rng.range(-0.8, 0.8)) * size;
        let r = rng.range(0.8, 1.1) * size;
        blobs.push((c, v3(r, r * 0.8, r), rgb(light)));
    }
    blobs
}

/// Tejado a dos aguas con grosor, alero y piñones encalados.
#[allow(clippy::too_many_arguments)]
fn roof(g: &mut Geo, x0: f32, x1: f32, z0: f32, z1: f32, y: f32, rise: f32, ridge_x: bool, col: Col) {
    let th = 0.16;
    // Coordenadas en el sistema (a lo largo de la cumbrera, a lo ancho)
    let (l0, l1, w0, w1) = if ridge_x { (x0, x1, z0, z1) } else { (z0, z1, x0, x1) };
    let wc = (w0 + w1) * 0.5;
    let p = |l: f32, w: f32, h: f32| if ridge_x { v3(l, h, w) } else { v3(w, h, l) };
    let top = y + rise;
    let roof_start = g.v.len();
    g.set(m::ROOF);
    for (we, sgn) in [(w0, -1.0f32), (w1, 1.0)] {
        let slope_n = {
            let run = (wc - we).abs();
            let nn = if ridge_x { v3(0.0, run, sgn * rise) } else { v3(sgn * rise, run, 0.0) };
            nn.norm()
        };
        let segs = ((l1 - l0) / 0.8).ceil() as usize;
        let (o, eu, ev) = (p(l0, we, y), p(l1, we, y) - p(l0, we, y), p(l0, wc, top) - p(l0, we, y));
        g.quad_grid(o, eu, ev, slope_n, col, segs.max(1), 4);
        // Cara inferior y canto del alero (subdivididos como la de arriba,
        // para que se comben igual y no asomen por encima)
        let under = shade(col, 0.6);
        let ou = p(l0, we, y - th);
        g.quad_grid(ou, p(l1, we, y - th) - ou, p(l0, wc, top - th) - ou, -slope_n, under, segs.max(1), 4);
        g.quad_grid(ou, p(l1, we, y - th) - ou, p(l0, we, y) - ou, p(0.0, sgn, 0.0) - p(0.0, 0.0, 0.0), under, segs.max(1), 1);
    }
    // Remates laterales del tejado
    for (le, sgn) in [(l0, -1.0f32), (l1, 1.0)] {
        let n = (p(sgn, 0.0, 0.0) - p(0.0, 0.0, 0.0)).norm();
        g.quad_n([p(le, w0, y - th), p(le, w0, y), p(le, wc, top), p(le, wc, top - th)], n, shade(col, 0.8));
        g.quad_n([p(le, w1, y - th), p(le, w1, y), p(le, wc, top), p(le, wc, top - th)], n, shade(col, 0.8));
    }
    // Cumbrera (en tramos, para que siga el combado)
    g.set(m::ROOF);
    let ridge = shade(col, 0.85);
    let rs = ((l1 - l0) / 0.8).ceil() as usize;
    for k in 0..rs {
        let (la, lb) = (lerp(l0, l1, k as f32 / rs as f32), lerp(l0, l1, (k + 1) as f32 / rs as f32));
        g.cylinder(p(la, wc, top + 0.05), p(lb, wc, top + 0.05), 0.13, 0.13, 6, ridge, false);
    }
    // El tejado se comba un poco en el centro, como en una casa vieja
    let sag = 0.06 * (l1 - l0).min(12.0);
    for vx in &mut g.v[roof_start..] {
        let l = if ridge_x { vx.p.x } else { vx.p.z };
        let t = ((l - l0) / (l1 - l0)).clamp(0.0, 1.0);
        let hgt = ((vx.p.y - y) / rise).clamp(0.0, 1.0);
        vx.p.y -= sag * (PI * t).sin() * (0.4 + 0.6 * hgt);
    }
    // Piñones (triángulos de pared bajo el tejado), metidos 0.4 m (el alero)
    g.set(m::PLASTER);
    let gable = rgb([0.95, 0.91, 0.82]);
    for (le, sgn) in [(l0 + 0.4, -1.0f32), (l1 - 0.4, 1.0)] {
        let n = (p(sgn, 0.0, 0.0) - p(0.0, 0.0, 0.0)).norm();
        let inset = 0.4 * rise / (wc - w0).abs().max(0.1);
        g.tri(p(le, w0 + 0.4, y + 0.0), p(le, w1 - 0.4, y + 0.0), p(le, wc, top - inset * 0.0 - 0.05), n, gable);
    }
}

/// Ventanas con marco y contraventanas, puertas, jardineras y entramado.
#[allow(clippy::too_many_arguments)]
fn wall_decor(g: &mut Geo, along_x: bool, a0: f32, a1: f32, face: f32, n: V3, h: f32, ops: &[(f32, f32, f32, f32)], timber: bool, style: u8, rng: &mut Rng) {
    // Sistema local: `t` a lo largo del muro, `y` altura, `o` hacia fuera
    let at = |t: f32, y: f32, o: f32| -> V3 {
        let c = face + o * if along_x { n.z } else { n.x };
        if along_x {
            v3(t, y, c)
        } else {
            v3(c, y, t)
        }
    };
    let along = if along_x { V3::X } else { V3::Z };
    let ax = [along, V3::UP, n];
    let shutter = rgb(SHUTTERS[style as usize % SHUTTERS.len()]);
    let len = a1 - a0;

    // Entramado de madera (colombage normando)
    if timber {
        g.set(m::WOOD);
        let tim = rgb(TIMBER);
        let in_op = |t: f32, y0: f32, y1: f32| {
            ops.iter().any(|&(oc, ow, oy0, oy1)| {
                let c = a0 + oc;
                (t - c).abs() < ow / 2.0 + 0.12 && y1 > oy0 - 0.12 && y0 < oy1 + 0.12
            })
        };
        // Vigas horizontales
        for (y, hh) in [(0.12, 0.12), (2.45, 0.09), (h - 0.1, 0.1)] {
            let mut t = a0;
            let mut start = a0;
            while t <= a1 + 1e-3 {
                let blocked = in_op(t, y - hh, y + hh);
                if blocked || t >= a1 - 1e-3 {
                    let end = if blocked { t - 0.05 } else { a1 };
                    if end - start > 0.2 {
                        g.obox(at((start + end) * 0.5, y, 0.04), ax, [(end - start) * 0.5, hh, 0.04], tim);
                    }
                    start = t + 0.05;
                }
                t += 0.1;
            }
        }
        // Postes y cruces de San Andrés
        let posts = (len / 1.3).round().max(2.0) as i32;
        let dt = len / posts as f32;
        for k in 0..=posts {
            let t = a0 + k as f32 * dt;
            for (y0, y1) in [(0.24, 2.36), (2.54, h - 0.2)] {
                if !in_op(t, y0, y1) && y1 > y0 + 0.2 {
                    g.obox(at(t, (y0 + y1) * 0.5, 0.04), ax, [0.08, (y1 - y0) * 0.5, 0.04], tim);
                }
            }
            if k < posts && (k % 2 == 0) {
                let tc = t + dt * 0.5;
                if !in_op(tc, 0.24, 2.36) {
                    let dy = 2.1f32;
                    let ang = (dy / dt).atan();
                    let lenb = (dt * dt + dy * dy).sqrt() * 0.5;
                    let dir = along * ang.cos() + V3::UP * ang.sin();
                    let up = n.cross(dir).norm();
                    let up = if up.dot(V3::UP) < 0.0 { -up } else { up };
                    g.obox(at(tc, 1.3, 0.035), [dir, up, n], [lenb, 0.06, 0.035], tim);
                }
            }
        }
    }

    for &(oc, ow, y0, y1) in ops {
        let c = a0 + oc;
        if y1 > 50.0 {
            continue;
        }
        if y0 < 0.05 {
            // Puerta: marco de madera y hoja abierta contra la pared
            g.set(m::WOOD);
            let wood = rgb([0.50, 0.33, 0.20]);
            for s in [-1.0f32, 1.0] {
                g.obox(at(c + s * (ow / 2.0 + 0.07), y1 / 2.0, 0.06), ax, [0.07, y1 / 2.0, 0.06], wood);
            }
            g.obox(at(c, y1 + 0.08, 0.06), ax, [ow / 2.0 + 0.14, 0.08, 0.06], wood);
            g.obox(at(c - ow / 2.0 - 0.14 - ow / 2.0, y1 / 2.0 - 0.05, 0.08), ax, [ow / 2.0, y1 / 2.0 - 0.05, 0.03], shutter);
            // Escalón de piedra
            g.set(m::STONE);
            g.obox(at(c, 0.06, 0.3), ax, [ow / 2.0 + 0.2, 0.06, 0.3], rgb([0.70, 0.67, 0.60]));
            continue;
        }
        // Ventana: marco blanco, alféizar y contraventanas abiertas
        g.set(m::WOOD);
        let fr = rgb(FRAME);
        for s in [-1.0f32, 1.0] {
            g.obox(at(c + s * (ow / 2.0 + 0.05), (y0 + y1) / 2.0, 0.04), ax, [0.05, (y1 - y0) / 2.0 + 0.05, 0.04], fr);
        }
        g.obox(at(c, y1 + 0.05, 0.04), ax, [ow / 2.0 + 0.1, 0.05, 0.04], fr);
        g.obox(at(c, y0 - 0.04, 0.09), ax, [ow / 2.0 + 0.14, 0.04, 0.09], fr);
        for s in [-1.0f32, 1.0] {
            let sc = c + s * (ow / 2.0 + 0.1 + ow * 0.27);
            g.obox(at(sc, (y0 + y1) / 2.0, 0.05), ax, [ow * 0.27, (y1 - y0) / 2.0, 0.025], shutter);
            for k in 0..4 {
                let y = y0 + 0.15 + k as f32 * (y1 - y0 - 0.2) / 4.0;
                g.obox(at(sc, y, 0.08), ax, [ow * 0.25, 0.015, 0.01], shade(shutter, 0.8));
            }
        }
        // Jardinera con flores
        if rng.chance(0.75) {
            g.set(m::WOOD);
            g.obox(at(c, y0 - 0.2, 0.22), ax, [ow / 2.0, 0.11, 0.12], rgb([0.55, 0.36, 0.22]));
            g.set(m::LEAVES);
            g.obox(at(c, y0 - 0.06, 0.22), ax, [ow / 2.0 - 0.03, 0.06, 0.1], rgb([0.30, 0.52, 0.22]));
            g.set(m::PLAIN);
            let fc = FLOWERS[rng.index(3)];
            for k in 0..7 {
                let t = c - ow / 2.0 + 0.1 + k as f32 * (ow - 0.2) / 6.0;
                let y = y0 + rng.range(0.0, 0.12);
                g.sphere(at(t, y, 0.22 + rng.range(-0.06, 0.06)), v3(0.07, 0.06, 0.07), rgb(fc));
            }
        }
    }
}

/// Listones oscuros en las aristas de una caja de munición.
fn crate_frame(g: &mut Geo, b: &Aabb, col: Col) {
    let c = b.center();
    let s = b.size() * 0.5;
    let t = 0.05;
    for (dx, dz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        g.obox(c + v3(dx * (s.x - t + 0.01), 0.0, dz * (s.z - t + 0.01)), IDENT, [t, s.y + 0.005, t], col);
    }
    for y in [-1.0f32, 1.0] {
        for dz in [-1.0f32, 1.0] {
            g.obox(c + v3(0.0, y * (s.y - t + 0.01), dz * (s.z - t + 0.01)), IDENT, [s.x + 0.005, t, t], col);
        }
        for dx in [-1.0f32, 1.0] {
            g.obox(c + v3(dx * (s.x - t + 0.01), y * (s.y - t + 0.01), 0.0), IDENT, [t, t, s.z + 0.005], col);
        }
    }
}

/// Tanque destruido: orugas con ruedas, casco, torreta redondeada y cañón.
fn tank(g: &mut Geo, c: V3) {
    let olive = rgb([0.44, 0.46, 0.33]);
    let dark = rgb([0.24, 0.24, 0.22]);
    g.set(m::METAL);
    for sx in [-1.0f32, 1.0] {
        let x = c.x + sx * 1.3;
        g.obox(v3(x, 0.55, c.z), IDENT, [0.3, 0.5, 3.35], dark);
        for k in 0..6 {
            let z = c.z - 2.7 + k as f32 * 1.08;
            g.cylinder(v3(x - 0.33, 0.42, z), v3(x + 0.33, 0.42, z), 0.36, 0.36, 10, shade(olive, 0.75), true);
        }
        g.obox(v3(x, 1.12, c.z), IDENT, [0.36, 0.05, 3.4], shade(olive, 0.9));
    }
    g.obox(v3(c.x, 0.95, c.z + 0.3), IDENT, [1.05, 0.6, 2.7], olive);
    // Plancha frontal inclinada
    let ang: f32 = 0.6;
    let dir = v3(0.0, ang.sin(), -ang.cos());
    let up = V3::X.cross(dir).norm();
    let up = if up.y < 0.0 { -up } else { up };
    g.obox(v3(c.x, 0.95, c.z - 2.75), [V3::X, up, dir], [1.05, 0.08, 0.7], olive);
    // Torreta y cañón (girado, como abandonado)
    let tc = v3(c.x, 1.55, c.z + 0.1);
    g.cylinder(tc, tc + V3::UP * 0.7, 0.98, 0.82, 14, olive, true);
    g.sphere(tc + V3::UP * 0.7, v3(0.8, 0.22, 0.8), olive);
    g.obox(tc + v3(0.0, 0.42, -0.95), IDENT, [0.32, 0.28, 0.18], shade(olive, 0.9));
    g.cylinder(tc + v3(0.0, 0.42, -1.0), tc + v3(0.25, 0.32, -4.9), 0.1, 0.085, 8, shade(olive, 0.85), true);
    g.cylinder(tc + v3(0.0, 0.7, 0.3), tc + v3(0.0, 0.85, 0.3), 0.3, 0.3, 10, dark, true);
    g.set(m::PLAIN);
}

fn hedge(g: &mut Geo, b: &Aabb, rng: &mut Rng) {
    g.set(m::LEAVES);
    let dark = rgb([0.22, 0.40, 0.19]);
    let s = b.size() * 0.5;
    g.obox(b.center() - V3::UP * 0.1, IDENT, [s.x - 0.12, s.y - 0.1, s.z - 0.12], dark);
    let along_x = s.x > s.z;
    let len = if along_x { b.size().x } else { b.size().z };
    let thick = if along_x { b.size().z } else { b.size().x };
    let count = (len / 1.4).ceil() as usize;
    let greens = [[0.25, 0.45, 0.20], [0.30, 0.52, 0.23], [0.22, 0.42, 0.20]];
    let axis = |t: f32, y: f32| if along_x { v3(lerp(b.min.x, b.max.x, t), y, b.center().z) } else { v3(b.center().x, y, lerp(b.min.z, b.max.z, t)) };
    for k in 0..count {
        let t = (k as f32 + 0.5) / count as f32;
        let c = axis(t, b.max.y - 0.4);
        // Las normales apuntan desde el eje del seto: se ilumina como un volumen
        let nc = Some((axis(t, b.min.y + b.size().y * 0.35), 0.6));
        let r = thick * 0.5 + rng.range(0.1, 0.4);
        let col = rgb(greens[rng.index(greens.len())]);
        g.sphere_ex(c + V3::UP * rng.range(-0.1, 0.25), v3(r, r * 0.8, r), mix(col, rgb([0.42, 0.62, 0.28]), 0.35), 1, nc);
        if b.size().y > 3.0 {
            g.sphere_ex(c - V3::UP * 1.6, v3(r * 0.95, r, r * 0.95), shade(col, 0.95), 1, nc);
            g.sphere_ex(c - V3::UP * 2.7 + V3::UP * rng.range(-0.1, 0.1), v3(r * 0.9, r * 0.8, r * 0.9), shade(col, 0.88), 1, nc);
        } else {
            g.sphere_ex(c - V3::UP * 1.2, v3(r * 0.95, r * 0.9, r * 0.95), shade(col, 0.92), 1, nc);
        }
        // Florecillas silvestres en el seto
        if rng.chance(0.35) {
            g.set(m::PLAIN);
            let fc = rgb(if rng.chance(0.5) { [0.97, 0.96, 0.90] } else { [0.96, 0.66, 0.76] });
            for _ in 0..6 {
                let d = rng.dir();
                let d = v3(d.x, d.y.abs() * 0.8 + 0.2, d.z).norm();
                g.sphere(c + v3(d.x * r, d.y * r * 0.8, d.z * r) * 0.98, v3(0.05, 0.05, 0.05), fc);
            }
            g.set(m::LEAVES);
        }
    }
}

/// Hiedra que trepa por las fachadas y matorrales con flores al pie de
/// los muros de las casas y de las ruinas.
fn greenery(g: &mut Geo, w: &World, rng: &mut Rng) {
    let leaf = [[0.22, 0.42, 0.19], [0.28, 0.50, 0.22], [0.34, 0.56, 0.24], [0.20, 0.38, 0.20]];
    let bush = |g: &mut Geo, base: V3, out: V3, rng: &mut Rng, flowers: bool| {
        g.set(m::LEAVES);
        let nc = Some((base + V3::UP * 0.1, 0.55));
        for _ in 0..3 {
            let off = v3(rng.range(-0.35, 0.35), rng.range(0.2, 0.45), rng.range(-0.35, 0.35)) + out * 0.15;
            let r = rng.range(0.32, 0.55);
            g.sphere_ex(base + off, v3(r, r * 0.8, r), rgb(leaf[rng.index(leaf.len())]), 1, nc);
        }
        if flowers {
            g.set(m::PLAIN);
            let fc = rgb(FLOWERS[rng.index(FLOWERS.len())]);
            for _ in 0..8 {
                let d = rng.dir();
                let d = v3(d.x, d.y.abs() * 0.7 + 0.3, d.z).norm();
                g.sphere(base + V3::UP * 0.4 + d * 0.5, v3(0.05, 0.05, 0.05), fc);
            }
        }
    };
    let ivy = |g: &mut Geo, at: &dyn Fn(f32, f32, f32) -> V3, t0: f32, top: f32, rng: &mut Rng| {
        g.set(m::LEAVES);
        let mut t = t0;
        let n = (top * 9.0) as usize;
        for k in 0..n {
            let y = k as f32 / n as f32 * top;
            t += rng.range(-0.12, 0.12);
            let spread = 0.25 + 0.35 * (1.0 - y / top);
            let p = at(t + rng.range(-spread, spread), y + rng.range(-0.1, 0.1), 0.1);
            let r = rng.range(0.16, 0.28);
            let o = at(0.0, 0.0, 1.0) - at(0.0, 0.0, 0.0);
            let rr = v3(
                if o.x.abs() > 0.5 { 0.09 } else { r },
                r * 0.9,
                if o.z.abs() > 0.5 { 0.09 } else { r },
            );
            g.sphere(p, rr, rgb(leaf[rng.index(leaf.len())]));
        }
    };
    for p in &w.props {
        if let Prop::Wall { along_x, a0, a1, face, n, h, ops, .. } = p {
            let at = |t: f32, y: f32, o: f32| -> V3 {
                let c = *face + o * if *along_x { n.z } else { n.x };
                if *along_x {
                    v3(t, y, c)
                } else {
                    v3(c, y, t)
                }
            };
            let near_door = |t: f32| ops.iter().any(|&(oc, ow, y0, _)| y0 < 0.05 && (t - (a0 + oc)).abs() < ow / 2.0 + 0.6);
            let in_window = |t: f32, y: f32| ops.iter().any(|&(oc, ow, y0, y1)| (t - (a0 + oc)).abs() < ow / 2.0 + 0.35 && y > y0 - 0.35 && y < y1 + 0.3);
            let mut t = a0 + 0.4;
            while t < a1 - 0.4 {
                if !near_door(t) && rng.chance(0.5) {
                    let fl = rng.chance(0.4);
                    bush(g, at(t, 0.0, 0.45), *n, rng, fl);
                }
                t += rng.range(0.9, 1.6);
            }
            if rng.chance(0.4) {
                let t0 = rng.range(a0 + 0.6, a1 - 0.6);
                if !near_door(t0) {
                    let top = rng.range(2.0, h - 0.3);
                    let at2 = |tt: f32, y: f32, o: f32| at(tt, y, o);
                    let mut tmp = Geo::new();
                    ivy(&mut tmp, &at2, t0, top, rng);
                    // Quitar las hojas que taparían ventanas
                    keep_outside(&mut tmp, |p| {
                        let t = if *along_x { p.x } else { p.z };
                        !in_window(t, p.y)
                    });
                    g.append(&tmp);
                }
            }
        }
    }
    // Ruinas: mucha hiedra y matorral
    for b in &w.blocks {
        if b.hidden || !b.ruin || b.b.min.y > 0.1 {
            continue;
        }
        let along_x = b.b.size().x >= b.b.size().z;
        let len = if along_x { b.b.size().x } else { b.b.size().z };
        if len < 1.2 {
            continue;
        }
        for side in [-1.0f32, 1.0] {
            let n = if along_x { v3(0.0, 0.0, side) } else { v3(side, 0.0, 0.0) };
            let face = if along_x { b.b.center().z + side * b.b.size().z * 0.5 } else { b.b.center().x + side * b.b.size().x * 0.5 };
            let (t0, t1) = if along_x { (b.b.min.x, b.b.max.x) } else { (b.b.min.z, b.b.max.z) };
            let at = |t: f32, y: f32, o: f32| -> V3 {
                let c = face + o * side;
                if along_x {
                    v3(t, y, c)
                } else {
                    v3(c, y, t)
                }
            };
            if rng.chance(0.6) {
                let top = rng.range(1.2, (b.b.size().y - 0.6).max(1.3));
                ivy(g, &at, rng.range(t0 + 0.3, (t1 - 0.3).max(t0 + 0.31)), top, rng);
            }
            if rng.chance(0.6) {
                let (pt, fl) = (at(rng.range(t0, t1), 0.0, 0.45), rng.chance(0.3));
                bush(g, pt, n, rng, fl);
            }
        }
    }
}

/// Deja en `g` solo los triángulos cuyo centro cumple `keep`.
fn keep_outside(g: &mut Geo, keep: impl Fn(V3) -> bool) {
    let mut out = Vec::with_capacity(g.i.len());
    for t in g.i.chunks(3) {
        let c = (g.v[t[0] as usize].p + g.v[t[1] as usize].p + g.v[t[2] as usize].p) / 3.0;
        if keep(c) {
            out.extend_from_slice(t);
        }
    }
    g.i = out;
}

fn sandbags(g: &mut Geo, b: &Aabb, rng: &mut Rng) {
    g.set(m::CLOTH);
    let along_x = b.size().x >= b.size().z;
    let len = if along_x { b.size().x } else { b.size().z };
    let thick = if along_x { b.size().z } else { b.size().x };
    let layers = (b.size().y / 0.26).round().max(1.0) as usize;
    let lh = b.size().y / layers as f32;
    let bag = 0.62;
    for l in 0..layers {
        let off = if l % 2 == 1 { bag * 0.5 } else { 0.0 };
        let mut t = -off;
        while t < len {
            let t0 = t.max(0.0);
            let t1 = (t + bag).min(len);
            if t1 - t0 > 0.12 {
                let tc = (t0 + t1) * 0.5;
                let y = b.min.y + (l as f32 + 0.5) * lh;
                let c = if along_x { v3(b.min.x + tc, y, b.center().z) } else { v3(b.center().x, y, b.min.z + tc) };
                // Cada saco, un almohadón: elipsoide algo aplastado y blando
                let half = (t1 - t0) * 0.5 + 0.03;
                let he = v3(if along_x { half } else { thick * 0.5 + 0.02 }, lh * 0.62, if along_x { thick * 0.5 + 0.02 } else { half });
                let col = shade(rgb([0.76, 0.69, 0.52]), rng.range(0.9, 1.06));
                g.sphere_ex(c + V3::UP * 0.01, he, col, 1, None);
            }
            t += bag;
        }
    }
}

/// Hierba, flores y matojos dentro del pueblo.
fn meadow(g: &mut Geo, w: &World, occ: &mut Occluders, rng: &mut Rng) {
    let blocked = |x: f32, z: f32| -> bool {
        if x.abs() < 4.0 || (z > 1.6 && z < 8.4) {
            return true;
        }
        w.blocks.iter().any(|b| b.b.min.y < 0.5 && x > b.b.min.x - 0.25 && x < b.b.max.x + 0.25 && z > b.b.min.z - 0.25 && z < b.b.max.z + 0.25)
            || w.bomb_sites.iter().chain(w.flag_bases.iter()).any(|p| p.dist_flat(v3(x, 0.0, z)) < 2.5)
    };
    g.set(m::BLADE);
    let mut placed = 0;
    let mut tries = 0;
    while placed < 9000 && tries < 40_000 {
        tries += 1;
        let (x, z) = (rng.range(-62.5, 62.5), rng.range(-62.5, 62.5));
        // Más densa en zonas tranquilas, más rala en el centro del pueblo
        let dens = 0.35 + 0.65 * noise2(x * 0.09, z * 0.09);
        if rng.f() > dens || blocked(x, z) {
            continue;
        }
        placed += 1;
        let base = v3(x, 0.0, z);
        g.sun = occ.sun_at(base + V3::UP * 0.25, V3::UP);
        let tall = rng.range(0.25, 0.55) * (0.7 + 0.6 * noise2(x * 0.2, z * 0.2));
        let k = rng.range(0.85, 1.1);
        let dark = shade(rgb([0.30, 0.50, 0.22]), k);
        let tip = shade(rgb([0.62, 0.78, 0.38]), k);
        let blades = 4;
        for bl in 0..blades {
            let a = rng.range(0.0, TAU);
            let side = v3(a.cos(), 0.0, a.sin()) * 0.045;
            let lean = v3((a + 1.3).cos(), 0.0, (a + 1.3).sin()) * rng.range(0.03, 0.14);
            let b0 = base + v3(rng.range(-0.08, 0.08), 0.0, rng.range(-0.08, 0.08)) + side * (bl as f32 * 0.3);
            let t = b0 + V3::UP * tall * rng.range(0.7, 1.15) + lean;
            let i0 = g.vert(b0 - side, V3::UP, dark);
            let i1 = g.vert(b0 + side, V3::UP, dark);
            let i2 = g.vert(t, V3::UP, tip);
            g.i.extend_from_slice(&[i0, i1, i2]);
        }
    }
    // Flores en grupos
    g.set(m::PLAIN);
    for _ in 0..140 {
        let (cx, cz) = (rng.range(-60.0, 60.0), rng.range(-60.0, 60.0));
        let col = rgb(FLOWERS[rng.index(FLOWERS.len())]);
        for _ in 0..rng.range(6.0, 16.0) as usize {
            let (x, z) = (cx + rng.range(-1.6, 1.6), cz + rng.range(-1.6, 1.6));
            if blocked(x, z) {
                continue;
            }
            let p = v3(x, rng.range(0.22, 0.42), z);
            g.sun = occ.sun_at(p, V3::UP);
            let s = rng.range(0.05, 0.08);
            let a = rng.range(0.0, PI);
            let (u, wv) = (v3(a.cos(), 0.0, a.sin()) * s, v3(-a.sin(), 0.0, a.cos()) * s);
            g.quad_n([p - u, p - wv, p + u, p + wv], V3::UP, col);
            let (cu, cw) = (u * 0.35, wv * 0.35);
            let q = p + V3::UP * 0.006;
            g.quad_n([q - cu - cw, q + cu - cw, q + cu + cw, q - cu + cw], V3::UP, rgb([0.98, 0.80, 0.25]));
            let i0 = g.vert(v3(x - 0.01, 0.0, z), V3::UP, rgb([0.30, 0.50, 0.22]));
            let i1 = g.vert(v3(x + 0.01, 0.0, z), V3::UP, rgb([0.30, 0.50, 0.22]));
            let i2 = g.vert(p, V3::UP, rgb([0.38, 0.60, 0.26]));
            g.i.extend_from_slice(&[i0, i1, i2]);
        }
    }
    g.sun = 1.0;
}

/// Campos de cultivo y colinas con bosquecillos alrededor del pueblo.
fn countryside(g: &mut Geo, rng: &mut Rng) {
    let cell = 8.0;
    let lim = 320.0;
    let n = (2.0 * lim / cell) as i32;
    let fields = [[0.46, 0.66, 0.31], [0.55, 0.70, 0.33], [0.40, 0.60, 0.29], [0.78, 0.72, 0.42], [0.66, 0.70, 0.36], [0.50, 0.66, 0.40]];
    g.set(m::GRASS);
    for j in 0..n {
        for i in 0..n {
            let (x0, z0) = (-lim + i as f32 * cell, -lim + j as f32 * cell);
            let (x1, z1) = (x0 + cell, z0 + cell);
            if x0 >= -72.0 && x1 <= 72.0 && z0 >= -72.0 && z1 <= 72.0 {
                continue;
            }
            let field_col = |x: f32, z: f32| -> Col {
                let t = noise2(x * 0.012 + 3.0, z * 0.012) * (fields.len() - 1) as f32;
                let (i0, f) = (t.floor() as usize, t - t.floor());
                let f = ((f - 0.5) * 3.0 + 0.5).clamp(0.0, 1.0);
                let a = rgb(fields[i0.min(fields.len() - 1)]);
                let b = rgb(fields[(i0 + 1).min(fields.len() - 1)]);
                shade(mix(a, b, f), 0.95 + 0.1 * noise2(x * 0.08, z * 0.08))
            };
            let _ = (i, j);
            // Normales suaves sacadas de la pendiente: colinas redondeadas
            let base = g.v.len() as u32;
            for (x, z) in [(x0, z0), (x1, z0), (x1, z1), (x0, z1)] {
                let e = 1.5;
                let nx = ground_height(x - e, z) - ground_height(x + e, z);
                let nz = ground_height(x, z - e) - ground_height(x, z + e);
                let n = v3(nx, 2.0 * e, nz).norm();
                g.vert(v3(x, ground_height(x, z), z), n, field_col(x, z));
            }
            g.i.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    // Arboledas en las colinas
    for k in 0..150 {
        let ang = k as f32 / 150.0 * TAU + rng.range(-0.02, 0.02);
        let d = rng.range(84.0, 200.0);
        let (x, z) = (ang.cos() * d, ang.sin() * d);
        if x.abs().max(z.abs()) < 76.0 {
            continue;
        }
        let pos = v3(x, ground_height(x, z) - 0.3, z);
        let s = rng.range(1.0, 1.7);
        g.set(m::WOOD);
        g.cylinder(pos, pos + V3::UP * 4.0 * s, 0.35 * s, 0.25 * s, 5, rgb([0.42, 0.31, 0.22]), false);
        g.set(m::LEAVES);
        let greens = [[0.28, 0.50, 0.22], [0.34, 0.56, 0.24], [0.24, 0.44, 0.22], [0.44, 0.62, 0.28]];
        let center = pos + V3::UP * (5.0 * s);
        for b in 0..6 {
            let off = v3(rng.range(-1.8, 1.8), rng.range(-0.6, 1.4), rng.range(-1.8, 1.8)) * s;
            let r = rng.range(1.6, 2.4) * s;
            let col = if off.y > 0.5 * s { rgb(greens[3]) } else { rgb(greens[rng.index(3)]) };
            g.sphere_ex(center + off * if b == 0 { 0.0 } else { 1.0 }, v3(r, r * 0.9, r), col, 1, Some((center, 0.7)));
        }
    }
}

// ----------------------------------------------------------------------
// Cielo
// ----------------------------------------------------------------------

/// Cúpula del cielo centrada en la cámara (el color lo pinta el sombreador).
pub fn sky_dome(g: &mut Geo, eye: V3) {
    g.set(m::SKY);
    g.sphere(eye, v3(900.0, 900.0, 900.0), [1.0, 1.0, 1.0, 1.0]);
}

/// Grandes cúmulos formados por bolas suaves orientadas a la cámara.
pub fn clouds(g: &mut Geo, eye: V3, right: V3, up: V3, time: f32) {
    static CLOUDS: OnceLock<Vec<Vec<(V3, f32)>>> = OnceLock::new();
    let set = CLOUDS.get_or_init(|| {
        let mut rng = Rng::new(77);
        let mut out = Vec::new();
        for k in 0..16 {
            let ang = k as f32 / 16.0 * TAU + rng.range(-0.15, 0.15);
            let d = rng.range(420.0, 640.0);
            let base = v3(ang.cos() * d, rng.range(110.0, 230.0), ang.sin() * d);
            let size = rng.range(0.7, 1.4);
            let mut blobs = Vec::new();
            // Base plana y ancha, torres redondeadas encima
            for _ in 0..14 {
                let off = v3(rng.range(-60.0, 60.0), rng.range(0.0, 10.0), rng.range(-25.0, 25.0)) * size;
                blobs.push((base + off, rng.range(18.0, 28.0) * size));
            }
            for _ in 0..10 {
                let off = v3(rng.range(-35.0, 35.0), rng.range(15.0, 45.0), rng.range(-18.0, 18.0)) * size;
                blobs.push((base + off, rng.range(16.0, 30.0) * size));
            }
            for _ in 0..4 {
                let off = v3(rng.range(-15.0, 15.0), rng.range(45.0, 70.0), rng.range(-10.0, 10.0)) * size;
                blobs.push((base + off, rng.range(14.0, 22.0) * size));
            }
            out.push(blobs);
        }
        out
    });
    g.set(m::CLOUD);
    let drift = v3(time * 1.2, 0.0, time * 0.4);
    let mut order: Vec<(f32, &Vec<(V3, f32)>)> = set.iter().map(|c| (c[0].0.flat().dist(eye.flat()), c)).collect();
    order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, cloud) in order {
        let base_y = cloud.iter().map(|b| b.0.y).fold(f32::MAX, f32::min);
        let top_y = cloud.iter().map(|b| b.0.y).fold(f32::MIN, f32::max);
        let mut blobs: Vec<&(V3, f32)> = cloud.iter().collect();
        blobs.sort_by(|a, b| b.0.dist(eye).partial_cmp(&a.0.dist(eye)).unwrap_or(std::cmp::Ordering::Equal));
        for (c, r) in blobs {
            let p = v3(eye.x, 0.0, eye.z) + *c + drift;
            let h = ((c.y - base_y) / (top_y - base_y + 1.0)).clamp(0.0, 1.0);
            g.billboard(p, right, up, *r, [1.0, 1.0, 1.0, 1.0], h);
        }
    }
}

// ----------------------------------------------------------------------
// Personajes y objetos
// ----------------------------------------------------------------------

pub struct SoldierLook {
    pub side: u8,
    /// Nación (uniforme y casco); ver `sim::nation`
    pub nation: u8,
    /// Arma en la mano (índice de `GUNS`)
    pub gun: u8,
    pub smg: bool,
    pub crouch: f32,
    pub pitch: f32,
    pub flash: f32,
    pub has_flag: Option<Col>,
}

/// Soldado de formas redondeadas y amables, al estilo de una ilustración.
#[allow(clippy::too_many_arguments)]
pub fn soldier(g: &mut Geo, pos: V3, yaw: f32, death_t: Option<f32>, walk: f32, moving: f32, look: &SoldierLook) {
    let fwd0 = V3::from_yaw(yaw);
    let r = V3::right_of_yaw(yaw);
    let dead = death_t.is_some();
    let (theta, lift) = match death_t {
        Some(t) => {
            let k = (t * 2.2).min(1.0);
            let k = k * k * (3.0 - 2.0 * k);
            (k * PI * 0.5, k * 0.16)
        }
        None => (0.0, 0.0),
    };
    let u = V3::UP * theta.cos() - fwd0 * theta.sin();
    let f = fwd0 * theta.cos() + V3::UP * theta.sin();
    let crouch = if dead { 0.0 } else { look.crouch };
    let drop = crouch * 0.42;
    let origin = pos + V3::UP * lift;
    let at = |x: f32, y: f32, z: f32| origin + r * x + u * y + f * z;
    let body = |y: f32| y - drop;
    let fl = (look.flash / 0.12).clamp(0.0, 1.0);
    let tint = |c: Col| mix(c, [1.0, 0.4, 0.35, 1.0], fl * 0.7);
    use crate::sim::nation as na;
    let nat = look.nation;
    let zombie = nat == na::ZOMBIE;
    let allied = nat != na::DE && !zombie;
    let (uni, trs, bts, hel) = match nat {
        na::UK => ([0.56, 0.46, 0.31, 1.0], [0.50, 0.41, 0.28, 1.0], [0.30, 0.22, 0.15, 1.0], [0.40, 0.40, 0.27, 1.0]),
        na::RU => ([0.60, 0.55, 0.37, 1.0], [0.48, 0.45, 0.31, 1.0], [0.15, 0.14, 0.13, 1.0], [0.32, 0.40, 0.25, 1.0]),
        na::DE => ([0.46, 0.50, 0.46, 1.0], [0.40, 0.44, 0.41, 1.0], [0.17, 0.16, 0.15, 1.0], [0.36, 0.40, 0.37, 1.0]),
        na::ZOMBIE => ([0.36, 0.38, 0.33, 1.0], [0.30, 0.31, 0.27, 1.0], [0.16, 0.14, 0.12, 1.0], [0.30, 0.32, 0.29, 1.0]),
        _ => ([0.52, 0.48, 0.32, 1.0], [0.46, 0.40, 0.29, 1.0], [0.42, 0.30, 0.19, 1.0], [0.40, 0.42, 0.28, 1.0]),
    };
    let uniform = tint(uni);
    let trousers = tint(trs);
    let boots = bts;
    let skin = tint(if zombie { [0.64, 0.72, 0.56, 1.0] } else { [0.96, 0.78, 0.64, 1.0] });
    let helmet = hel;
    let gk = crate::sim::gun(look.gun).kind;
    let wood = [0.52, 0.35, 0.21, 1.0];
    let metal = [0.20, 0.21, 0.22, 1.0];
    let belt = match nat {
        na::UK => [0.72, 0.68, 0.54, 1.0],
        na::RU => [0.40, 0.30, 0.20, 1.0],
        _ if allied => [0.64, 0.58, 0.42, 1.0],
        _ => [0.24, 0.20, 0.16, 1.0],
    };
    let pack = if allied { [0.58, 0.54, 0.38, 1.0] } else { [0.44, 0.37, 0.28, 1.0] };
    let dir = |a: f32| (-u * a.cos() + f * a.sin()).norm();

    // Piernas: muslo y espinilla con rodilla, botas
    let mv = moving.min(1.0);
    for (sx, phase) in [(-1.0f32, 0.0f32), (1.0, PI)] {
        let s = if dead { 0.0 } else { (walk + phase).sin() * 0.5 * mv };
        let thigh_a = s + crouch * 1.25;
        let bend = if dead { 0.05 } else { 0.35 * mv * (walk + phase).cos().max(0.0) + crouch * 2.0 };
        let hip = at(sx * 0.1, body(0.92), 0.0);
        let knee = hip + dir(thigh_a) * 0.44;
        let ankle = knee + dir(thigh_a - bend) * 0.42;
        g.set(m::CLOTH);
        g.cylinder(hip, knee, 0.088, 0.075, 10, trousers, false);
        g.sphere(knee, v3(0.075, 0.075, 0.075), trousers);
        g.cylinder(knee, ankle, 0.073, 0.064, 8, if nat == na::US { [0.62, 0.58, 0.44, 1.0] } else if nat == na::UK { [0.66, 0.62, 0.48, 1.0] } else { trousers }, false);
        g.set(m::PLAIN);
        g.obox(ankle + f * 0.04 + u * 0.01, [r, u, f], [0.075, 0.065, 0.14], boots);
    }

    // Cadera, guerrera, cinturón con cartucheras, mochila y manta
    g.set(m::CLOTH);
    g.sphere_hi(at(0.0, body(0.95), 0.0), v3(0.19, 0.12, 0.13), trousers);
    g.cylinder(at(0.0, body(0.96), 0.0), at(0.0, body(1.14), 0.0), 0.2, 0.2, 10, uniform, false);
    g.sphere_hi(at(0.0, body(1.26), 0.0), v3(0.225, 0.3, 0.155), uniform);
    g.set(m::PLAIN);
    g.cylinder(at(0.0, body(0.99), 0.0), at(0.0, body(1.06), 0.0), 0.212, 0.212, 10, belt, false);
    for sx in [-1.0f32, 1.0] {
        g.obox(at(sx * 0.1, body(1.02), 0.19), [r, u, f], [0.05, 0.045, 0.03], shade(belt, 0.9));
    }
    g.obox(at(0.0, body(1.27), -0.19), [r, u, f], [0.15, 0.17, 0.07], pack);
    g.set(m::CLOTH);
    g.cylinder(at(-0.18, body(1.47), -0.17), at(0.18, body(1.47), -0.17), 0.06, 0.06, 8, if allied { [0.50, 0.46, 0.34, 1.0] } else { [0.38, 0.40, 0.44, 1.0] }, true);
    let sh_r = at(0.21, body(1.44), 0.0);
    let sh_l = at(-0.21, body(1.44), 0.0);
    g.sphere_hi(sh_r, v3(0.085, 0.08, 0.085), uniform);
    g.sphere_hi(sh_l, v3(0.085, 0.08, 0.085), uniform);

    // Cuello, cabeza, cara y casco
    g.set(m::SKIN);
    g.cylinder(at(0.0, body(1.5), 0.0), at(0.0, body(1.58), 0.01), 0.05, 0.05, 6, skin, false);
    let head = at(0.0, body(1.67), 0.02);
    g.sphere_hi(head, v3(0.115, 0.128, 0.12), skin);
    g.set(if zombie { m::GLOW } else { m::PLAIN });
    for sx in [-1.0f32, 1.0] {
        let eye_c = if zombie { [0.95, 0.75, 0.3, 1.0] } else { [0.14, 0.11, 0.11, 1.0] };
        g.sphere(head + r * (sx * 0.042) + u * 0.008 + f * 0.108, v3(0.013, 0.018, 0.01), eye_c);
        if !zombie {
            g.sphere(head + r * (sx * 0.068) - u * 0.04 + f * 0.088, v3(0.024, 0.015, 0.01), [0.98, 0.62, 0.56, 1.0]);
        }
    }
    g.set(m::PLAIN);
    g.sphere(head - u * 0.015 + f * 0.118, v3(0.016, 0.02, 0.016), shade(skin, 0.95));
    g.set(m::METAL);
    if zombie {
        // Casco abollado y torcido (o sin casco)
        if (look.gun as u32 + (pos.x * 7.0) as i32 as u32) % 2 == 0 {
            g.sphere_hi(head + u * 0.06 + r * 0.03, v3(0.15, 0.1, 0.16), helmet);
        }
    } else if nat == na::UK {
        // Casco Brodie: plato de ala ancha
        g.sphere_hi(head + u * 0.07, v3(0.13, 0.075, 0.14), helmet);
        g.cylinder(head + u * 0.045, head + u * 0.06, 0.24, 0.24, 14, shade(helmet, 0.92), true);
    } else if nat == na::RU {
        // SSh-40: cúpula alta sin ala
        g.sphere_hi(head + u * 0.055, v3(0.155, 0.125, 0.165), helmet);
        g.cylinder(head + u * 0.0, head + u * 0.05, 0.17, 0.16, 12, shade(helmet, 0.9), false);
    } else if allied {
        g.sphere_hi(head + u * 0.05, v3(0.158, 0.11, 0.168), helmet);
        g.cylinder(head + u * 0.03, head + u * 0.048, 0.2, 0.2, 12, shade(helmet, 0.9), true);
    } else {
        g.sphere_hi(head + u * 0.055 - f * 0.01, v3(0.15, 0.12, 0.16), helmet);
        g.cylinder(head - u * 0.025 - f * 0.012, head + u * 0.06 - f * 0.01, 0.175, 0.155, 12, shade(helmet, 0.9), false);
    }
    if let Some(fc) = look.has_flag {
        g.set(m::WOOD);
        g.cylinder(at(-0.1, body(0.9), -0.24), at(-0.1, body(2.45), -0.24), 0.02, 0.02, 5, [0.45, 0.35, 0.22, 1.0], false);
        g.set(m::CLOTH);
        g.obox(at(-0.1, body(2.15), -0.5), [r, u, f], [0.015, 0.22, 0.26], fc);
    }

    // Brazos con codo: del hombro a la mano, que sujetan el arma
    let pitch = if dead { 0.0 } else { look.pitch.clamp(-1.0, 1.0) };
    let pf = (f * pitch.cos() + u * pitch.sin()).norm();
    let pu = (u * pitch.cos() - f * pitch.sin()).norm();
    let chest = at(0.0, body(1.36), 0.0);
    let aim = |x: f32, y: f32, z: f32| chest + r * x + pu * y + pf * z;
    let (hand_r, hand_l) = if dead {
        (at(0.32, body(0.95), 0.1), at(-0.32, body(0.95), 0.1))
    } else if zombie {
        // Brazos hacia delante, tambaleándose
        let sw = (walk * 0.5).sin() * 0.06;
        (at(0.2, body(1.42) + sw, 0.62), at(-0.2, body(1.42) - sw, 0.6))
    } else if gk == crate::sim::GunKind::Pistol {
        (aim(0.06, -0.04, 0.42), aim(0.02, -0.06, 0.38))
    } else if look.smg {
        (aim(0.12, -0.12, 0.2), aim(0.1, -0.12, 0.48))
    } else {
        (aim(0.12, -0.08, 0.2), aim(0.11, -0.07, 0.55))
    };
    let arm = |g: &mut Geo, sh: V3, hand: V3, side: f32| {
        let d = hand - sh;
        let len = d.len().clamp(0.1, 0.58);
        let dn = d.norm();
        let hand = sh + dn * len;
        let mid = sh + dn * (len * 0.5);
        let hgt = (0.3f32 * 0.3 - (len * 0.5) * (len * 0.5)).max(0.0).sqrt();
        let want = -u + r * (side * 0.7);
        let bend = (want - dn * want.dot(dn)).norm();
        let elbow = mid + bend * hgt;
        g.set(m::CLOTH);
        g.cylinder(sh, elbow, 0.07, 0.062, 10, uniform, false);
        g.sphere(elbow, v3(0.062, 0.062, 0.062), uniform);
        g.cylinder(elbow, hand, 0.06, 0.052, 10, uniform, false);
        g.set(m::SKIN);
        g.sphere(hand, v3(0.05, 0.05, 0.055), skin);
    };
    arm(g, sh_r, hand_r, 1.0);
    arm(g, sh_l, hand_l, -1.0);

    // Arma a lo largo de la línea de tiro
    if death_t.map(|t| t < 0.3).unwrap_or(true) && !zombie {
        let ax = [r, pu, pf];
        use crate::sim::GunKind as K;
        if gk == K::Pistol {
            g.set(m::METAL);
            g.obox(aim(0.05, -0.0, 0.5), ax, [0.018, 0.03, 0.09], metal);
            g.obox(aim(0.05, -0.05, 0.45), ax, [0.016, 0.04, 0.02], metal);
        } else if look.smg {
            g.set(m::METAL);
            g.obox(aim(0.12, -0.07, 0.32), ax, [0.035, 0.05, 0.3], metal);
            g.obox(aim(0.12, -0.2, 0.36), ax, [0.02, 0.09, 0.03], metal);
            g.set(m::WOOD);
            g.obox(aim(0.12, -0.08, -0.02), ax, [0.03, 0.05, 0.1], wood);
        } else {
            g.set(m::WOOD);
            g.obox(aim(0.12, -0.06, 0.32), ax, [0.03, 0.04, 0.5], wood);
            g.obox(aim(0.12, -0.1, -0.08), ax, [0.03, 0.06, 0.1], wood);
            g.set(m::METAL);
            g.obox(aim(0.12, -0.02, 0.92), ax, [0.014, 0.014, 0.14], metal);
            g.obox(aim(0.12, -0.02, 0.32), ax, [0.02, 0.02, 0.1], metal);
            if gk == K::Sniper {
                // Mira telescópica
                g.cylinder(aim(0.12, 0.04, 0.2), aim(0.12, 0.04, 0.5), 0.022, 0.022, 8, metal, true);
            }
            if gk == K::Support {
                // Cargador y bípode
                g.obox(aim(0.12, 0.05, 0.36), ax, [0.02, 0.05, 0.06], metal);
                g.cylinder(aim(0.12, -0.04, 0.8), aim(0.07, -0.3, 0.86), 0.008, 0.008, 4, metal, false);
                g.cylinder(aim(0.12, -0.04, 0.8), aim(0.17, -0.3, 0.86), 0.008, 0.008, 4, metal, false);
            }
        }
    }
    g.set(m::PLAIN);
}

pub fn grenade(g: &mut Geo, pos: V3, spin: f32, allied: bool) {
    let a = V3::from_yaw(spin);
    let r = V3::right_of_yaw(spin);
    g.set(m::METAL);
    if allied {
        g.sphere(pos, v3(0.055, 0.07, 0.055), [0.30, 0.36, 0.22, 1.0]);
    } else {
        g.set(m::WOOD);
        g.cylinder(pos - a * 0.17, pos + a * 0.14, 0.03, 0.03, 6, [0.56, 0.42, 0.26, 1.0], true);
        g.set(m::METAL);
        g.cylinder(pos + a * 0.14, pos + a * 0.25, 0.06, 0.06, 8, [0.32, 0.36, 0.32, 1.0], true);
        let _ = r;
    }
    g.set(m::PLAIN);
}

pub struct ViewParams {
    /// Índice en sim::GUNS
    pub gun: u8,
    /// Nación (color de las mangas)
    pub nation: u8,
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

    let wood = [0.54, 0.35, 0.20, 1.0];
    let metal = [0.22, 0.23, 0.25, 1.0];
    use crate::sim::nation as na;
    let sleeve = match v.nation {
        na::DE => [0.47, 0.51, 0.47, 1.0],
        na::UK => [0.56, 0.46, 0.31, 1.0],
        na::RU => [0.60, 0.55, 0.37, 1.0],
        _ => [0.52, 0.48, 0.32, 1.0],
    };
    let skin = [0.96, 0.78, 0.64, 1.0];
    let def = crate::sim::gun(v.gun);
    use crate::sim::GunKind as K;
    let name = def.name;

    let tip;
    let left;
    match def.kind {
        K::Rifle | K::Sniper | K::Semi => {
            let carbine = name == "M1 Carbine";
            let k = if carbine { 0.82 } else { 1.0 };
            g.set(m::WOOD);
            g.obox(at(0.0, -0.037, -0.12), ax, [0.022, 0.035, 0.2], wood);
            g.obox(at(0.0, -0.065, -0.27), ax, [0.022, 0.05, 0.06], wood);
            g.obox(at(0.0, -0.032, 0.32 * k), ax, [0.022, 0.024, 0.16 * k], wood);
            g.set(m::METAL);
            g.obox(at(0.0, -0.02, 0.1), ax, [0.02, 0.022, 0.08], metal);
            g.obox(at(0.0, -0.01, 0.42 * k), ax, [0.009, 0.009, 0.24 * k], metal);
            g.obox(at(-0.008, -0.004, 0.03), ax, [0.003, 0.012, 0.005], metal);
            g.obox(at(0.008, -0.004, 0.03), ax, [0.003, 0.012, 0.005], metal);
            g.obox(at(0.0, -0.008, 0.635 * k), ax, [0.0035, 0.009, 0.005], metal);
            if def.kind != K::Semi {
                // Palanca del cerrojo
                g.obox(at(0.035, -0.015, 0.06), ax, [0.02, 0.006, 0.006], metal);
                g.obox(at(0.055, -0.02, 0.06), ax, [0.01, 0.01, 0.01], metal);
            }
            if name.starts_with("Lee-Enfield") || carbine || name == "Gewehr 43" || name == "SVT-40" {
                // Cargador extraíble bajo el cajón
                g.obox(at(0.0, -0.075, 0.12), ax, [0.014, 0.03, 0.03], metal);
            }
            if def.kind == K::Sniper {
                g.cylinder(at(0.0, 0.035, -0.04), at(0.0, 0.035, 0.22), 0.016, 0.016, 10, metal, true);
                g.cylinder(at(0.0, 0.035, 0.2), at(0.0, 0.035, 0.25), 0.022, 0.022, 10, metal, true);
                g.cylinder(at(0.0, 0.035, -0.07), at(0.0, 0.035, -0.03), 0.02, 0.02, 10, metal, true);
            }
            tip = at(0.0, -0.01, 0.68 * k);
            left = at(-0.012, -0.072, 0.3 * k);
        }
        K::Smg if name == "MP40" => {
            g.set(m::METAL);
            g.obox(at(0.0, -0.035, 0.06), ax, [0.022, 0.028, 0.16], metal);
            g.obox(at(0.0, -0.03, 0.3), ax, [0.012, 0.014, 0.09], metal);
            g.obox(at(0.0, -0.03, 0.4), ax, [0.016, 0.018, 0.015], metal);
            g.obox(at(0.0, -0.045, -0.16), ax, [0.006, 0.02, 0.1], metal);
            g.obox(at(0.0, -0.075, -0.26), ax, [0.02, 0.03, 0.01], metal);
            g.obox(at(0.0, -0.13, 0.14), ax, [0.014, 0.085, 0.02], metal);
            g.obox(at(0.0, -0.002, -0.02), ax, [0.009, 0.006, 0.008], metal);
            g.obox(at(0.0, -0.005, 0.38), ax, [0.0025, 0.009, 0.004], metal);
            g.set(m::WOOD);
            g.obox(at(0.0, -0.1, -0.02), ax, [0.017, 0.045, 0.022], [0.30, 0.22, 0.15, 1.0]);
            tip = at(0.0, -0.03, 0.42);
            left = at(-0.01, -0.125, 0.14);
        }
        K::Smg if name == "Sten" => {
            g.set(m::METAL);
            g.cylinder(at(0.0, -0.03, -0.05), at(0.0, -0.03, 0.4), 0.018, 0.018, 9, metal, true);
            // Cargador lateral, a la izquierda
            g.obox(at(-0.07, -0.035, 0.12), ax, [0.06, 0.012, 0.018], metal);
            // Culata de tubo
            g.obox(at(0.0, -0.05, -0.18), ax, [0.005, 0.03, 0.12], metal);
            g.obox(at(0.0, -0.085, -0.29), ax, [0.02, 0.03, 0.008], metal);
            g.obox(at(0.0, -0.08, 0.0), ax, [0.012, 0.04, 0.02], metal);
            g.obox(at(0.0, -0.004, 0.38), ax, [0.0025, 0.009, 0.004], metal);
            tip = at(0.0, -0.03, 0.42);
            left = at(-0.04, -0.045, 0.12);
        }
        K::Smg if name == "PPSh-41" => {
            g.set(m::WOOD);
            g.obox(at(0.0, -0.05, -0.14), ax, [0.022, 0.045, 0.16], wood);
            g.set(m::METAL);
            g.obox(at(0.0, -0.03, 0.12), ax, [0.022, 0.026, 0.12], metal);
            g.cylinder(at(0.0, -0.025, 0.22), at(0.0, -0.025, 0.42), 0.02, 0.02, 10, metal, true);
            // Tambor
            g.cylinder(at(-0.02, -0.11, 0.1), at(0.02, -0.11, 0.1), 0.065, 0.065, 14, metal, true);
            g.obox(at(0.0, -0.004, 0.36), ax, [0.0025, 0.009, 0.004], metal);
            tip = at(0.0, -0.025, 0.44);
            left = at(-0.01, -0.06, 0.28);
        }
        K::Smg => {
            g.set(m::METAL);
            g.obox(at(0.0, -0.035, 0.08), ax, [0.025, 0.03, 0.14], metal);
            g.obox(at(0.0, -0.03, 0.3), ax, [0.014, 0.016, 0.1], metal);
            g.obox(at(0.0, -0.03, 0.42), ax, [0.018, 0.02, 0.025], metal);
            g.obox(at(0.0, -0.115, 0.08), ax, [0.015, 0.075, 0.02], metal);
            g.obox(at(0.0, -0.001, -0.02), ax, [0.01, 0.006, 0.01], metal);
            g.obox(at(0.0, -0.004, 0.39), ax, [0.0025, 0.009, 0.004], metal);
            g.set(m::WOOD);
            g.obox(at(0.0, -0.05, -0.15), ax, [0.02, 0.04, 0.12], wood);
            g.obox(at(0.0, -0.1, 0.2), ax, [0.017, 0.045, 0.02], wood);
            g.obox(at(0.0, -0.1, -0.01), ax, [0.017, 0.045, 0.022], wood);
            tip = at(0.0, -0.03, 0.46);
            left = at(-0.01, -0.115, 0.2);
        }
        K::Support => {
            let stg = name == "StG 44";
            g.set(if stg { m::METAL } else { m::WOOD });
            g.obox(at(0.0, -0.045, -0.16), ax, [0.022, 0.045, 0.16], if stg { [0.30, 0.22, 0.15, 1.0] } else { wood });
            g.set(m::METAL);
            g.obox(at(0.0, -0.025, 0.1), ax, [0.024, 0.03, 0.14], metal);
            g.cylinder(at(0.0, -0.02, 0.24), at(0.0, -0.02, if stg { 0.5 } else { 0.66 }), 0.012, 0.012, 8, metal, true);
            match name {
                "Bren" => {
                    // Cargador curvo por arriba
                    g.obox(at(0.0, 0.05, 0.1), ax, [0.012, 0.05, 0.03], metal);
                    g.obox(at(0.0, 0.1, 0.12), ax, [0.012, 0.025, 0.035], metal);
                }
                "DP-28" => {
                    // Plato encima
                    g.cylinder(at(0.0, 0.015, 0.08), at(0.0, 0.035, 0.08), 0.09, 0.09, 16, metal, true);
                }
                _ => {
                    // Cargador por debajo (curvo en el StG)
                    g.obox(at(0.0, -0.085, 0.14), ax, [0.014, 0.05, 0.022], metal);
                    if stg {
                        g.obox(at(0.0, -0.13, 0.16), ax, [0.014, 0.025, 0.022], metal);
                    }
                }
            }
            if !stg {
                // Bípode plegado
                g.obox(at(0.0, -0.045, 0.52), ax, [0.006, 0.006, 0.08], metal);
            }
            g.obox(at(0.0, -0.004, if stg { 0.46 } else { 0.6 }), ax, [0.0025, 0.009, 0.004], metal);
            tip = at(0.0, -0.02, if stg { 0.52 } else { 0.68 });
            left = at(-0.012, -0.06, 0.3);
        }
        K::Pistol | K::Melee => {
            g.set(m::METAL);
            let len = if name == "Webley" { 0.2 } else { 0.15 };
            g.obox(at(0.0, -0.012, 0.18), ax, [0.012, 0.016, len * 0.5], metal);
            if name == "Webley" {
                g.cylinder(at(-0.018, -0.03, 0.12), at(0.018, -0.03, 0.12), 0.025, 0.025, 8, metal, true);
            }
            g.set(m::WOOD);
            g.obox(at(0.0, -0.06, 0.12), ax, [0.011, 0.045, 0.018], [0.30, 0.22, 0.15, 1.0]);
            g.set(m::METAL);
            g.obox(at(0.0, 0.006, 0.17 + len * 0.45), ax, [0.002, 0.006, 0.003], metal);
            tip = at(0.0, -0.012, 0.18 + len * 0.5 + 0.01);
            left = at(-0.02, -0.075, 0.12);
        }
    }
    // Manos y antebrazos
    let right_hand = at(0.006, -0.08, -0.03);
    g.set(m::SKIN);
    g.sphere(right_hand, v3(0.036, 0.042, 0.055) , skin);
    g.sphere(left, v3(0.04, 0.036, 0.05), skin);
    g.set(m::CLOTH);
    g.cylinder(right_hand - f * 0.03 - u * 0.01, at(0.1, -0.27, -0.32), 0.04, 0.055, 9, sleeve, false);
    g.cylinder(left - f * 0.02 - u * 0.02, at(-0.15, -0.27, -0.02), 0.04, 0.055, 9, sleeve, false);
    g.set(m::CLOTH);
    g.cylinder(right_hand - f * 0.055 - u * 0.015, right_hand - f * 0.07 - u * 0.02, 0.046, 0.046, 9, shade(sleeve, 0.85), false);
    g.cylinder(left - f * 0.04 - u * 0.03, left - f * 0.055 - u * 0.035, 0.046, 0.046, 9, shade(sleeve, 0.85), false);

    if v.muzzle > 0.0 {
        g.set(m::GLOW);
        let s = 0.05 + 0.04 * (v.flash_seed * 13.7).sin().abs();
        g.obox(tip + f * s, ax, [s * 0.7, s * 0.7, s * 1.3], [1.0, 0.85, 0.45, 0.9]);
    }
    g.set(m::PLAIN);
}

pub const FLAG_COLORS: [Col; 2] = [[0.34, 0.50, 0.26, 1.0], [0.72, 0.16, 0.14, 1.0]];

/// Bandera clavada en el suelo (ondea con el tiempo).
pub fn flag(g: &mut Geo, pos: V3, team: u8, t: f32, base: bool) {
    let c = FLAG_COLORS[team.min(1) as usize];
    if base {
        g.set(m::STONE);
        g.cylinder(pos, pos + V3::UP * 0.12, 1.0, 0.9, 14, [0.72, 0.68, 0.60, 1.0], true);
    }
    g.set(m::WOOD);
    g.cylinder(pos, pos + V3::UP * 2.8, 0.035, 0.03, 6, [0.50, 0.38, 0.24, 1.0], false);
    g.set(m::CLOTH);
    let wave = (t * 3.0).sin() * 0.15;
    let f = V3::from_yaw(wave);
    let r = V3::right_of_yaw(wave);
    g.obox(pos + V3::UP * 2.45 + f * 0.45, [r, V3::UP, f], [0.015, 0.3, 0.42], c);
    g.obox(pos + V3::UP * 2.45 + f * 0.45 + r * 0.02, [r, V3::UP, f], [0.01, 0.08, 0.12], [0.95, 0.95, 0.9, 1.0]);
    g.set(m::PLAIN);
}

/// Objetivo de Buscar y destruir: cañón antitanque con cajas de munición.
pub fn bomb_site(g: &mut Geo, pos: V3) {
    let metal = [0.38, 0.42, 0.34, 1.0];
    let wood = [0.58, 0.42, 0.26, 1.0];
    g.set(m::WOOD);
    g.obox(pos + v3(1.2, 0.45, 0.6), IDENT, [0.5, 0.45, 0.35], wood);
    g.obox(pos + v3(1.2, 1.05, 0.6), IDENT, [0.4, 0.15, 0.3], wood);
    g.set(m::METAL);
    g.obox(pos + v3(-1.0, 0.7, -0.8), IDENT, [0.9, 0.15, 0.5], metal);
    g.cylinder(pos + v3(-1.0, 0.95, -0.7), pos + v3(-1.0, 0.95, 0.6), 0.07, 0.06, 8, metal, true);
    g.obox(pos + v3(-1.0, 0.9, -1.0), IDENT, [0.55, 0.35, 0.05], metal);
    for sx in [-1.0f32, 1.0] {
        g.cylinder(pos + v3(-1.0 + sx * 0.95, 0.35, -0.8), pos + v3(-1.0 + sx * 0.85, 0.35, -0.8), 0.35, 0.35, 12, [0.18, 0.18, 0.18, 1.0], true);
    }
    g.set(m::PLAIN);
}

pub fn planted_bomb(g: &mut Geo, pos: V3, t: f32) {
    g.set(m::WOOD);
    g.obox(pos + V3::UP * 0.12, IDENT, [0.22, 0.12, 0.15], [0.45, 0.36, 0.26, 1.0]);
    g.set(m::GLOW);
    let blink = if (t * 6.0).sin() > 0.0 { [1.0, 0.15, 0.08, 1.0] } else { [0.35, 0.06, 0.05, 1.0] };
    g.obox(pos + V3::UP * 0.27, IDENT, [0.04, 0.03, 0.04], blink);
    g.set(m::PLAIN);
}

/// Cuartel General: radio de campaña sobre cajas, con antena y un banderín
/// del color del equipo que lo tiene (o blanco si está libre).
pub fn hq_radio(g: &mut Geo, pos: V3, owner: Option<u8>, t: f32) {
    let wood = [0.55, 0.42, 0.27, 1.0];
    let olive = [0.36, 0.40, 0.28, 1.0];
    g.set(m::WOOD);
    g.obox(pos + v3(0.0, 0.3, 0.0), IDENT, [0.55, 0.3, 0.4], wood);
    g.obox(pos + v3(0.7, 0.22, 0.3), IDENT, [0.3, 0.22, 0.25], shade(wood, 0.9));
    g.set(m::METAL);
    g.obox(pos + v3(0.0, 0.82, 0.0), IDENT, [0.32, 0.22, 0.2], olive);
    g.obox(pos + v3(0.0, 0.85, 0.205), IDENT, [0.24, 0.12, 0.01], [0.15, 0.15, 0.14, 1.0]);
    g.cylinder(pos + v3(0.22, 1.04, 0.0), pos + v3(0.25, 3.2, 0.0), 0.012, 0.006, 5, [0.2, 0.2, 0.2, 1.0], false);
    g.set(m::GLOW);
    let on = (t * 3.0).sin() > 0.0;
    g.obox(pos + v3(-0.15, 0.95, 0.21), IDENT, [0.025, 0.025, 0.01], if on { [1.0, 0.75, 0.2, 1.0] } else { [0.4, 0.3, 0.1, 1.0] });
    g.set(m::CLOTH);
    let c = match owner {
        Some(t) => FLAG_COLORS[t.min(1) as usize],
        None => [0.92, 0.9, 0.84, 1.0],
    };
    let wave = (t * 2.5).sin() * 0.2;
    let f = V3::from_yaw(wave);
    let r = V3::right_of_yaw(wave);
    g.obox(pos + v3(0.25, 2.95, 0.0) + f * 0.3, [r, V3::UP, f], [0.01, 0.16, 0.28], c);
    g.set(m::PLAIN);
}

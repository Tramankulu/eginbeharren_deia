//! El mapa: un pueblo normando en ruinas hecho de cajas alineadas a los ejes,
//! con colisiones, trazado de rayos y línea de visión.

use crate::math::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

impl Aabb {
    pub fn new(a: V3, b: V3) -> Aabb {
        Aabb {
            min: v3(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)),
            max: v3(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)),
        }
    }
    pub fn cs(center: V3, size: V3) -> Aabb {
        let h = size * 0.5;
        Aabb {
            min: center - h,
            max: center + h,
        }
    }
    pub fn center(&self) -> V3 {
        (self.min + self.max) * 0.5
    }
    pub fn size(&self) -> V3 {
        self.max - self.min
    }
    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.min.x < o.max.x
            && self.max.x > o.min.x
            && self.min.y < o.max.y
            && self.max.y > o.min.y
            && self.min.z < o.max.z
            && self.max.z > o.min.z
    }
    pub fn contains(&self, p: V3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }
    /// Intersección rayo-caja (método de las losas). Devuelve distancia y normal de entrada.
    pub fn ray(&self, o: V3, d: V3, max_t: f32) -> Option<(f32, V3)> {
        let mut tmin = 0.0f32;
        let mut tmax = max_t;
        let mut n = V3::ZERO;
        for axis in 0..3 {
            let oa = o.get(axis);
            let da = d.get(axis);
            let mn = self.min.get(axis);
            let mx = self.max.get(axis);
            if da.abs() < 1e-9 {
                if oa < mn || oa > mx {
                    return None;
                }
            } else {
                let inv = 1.0 / da;
                let mut t1 = (mn - oa) * inv;
                let mut t2 = (mx - oa) * inv;
                let mut sign = -1.0;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                    sign = 1.0;
                }
                if t1 > tmin {
                    tmin = t1;
                    n = V3::axis(axis) * sign;
                }
                tmax = tmax.min(t2);
                if tmin > tmax {
                    return None;
                }
            }
        }
        Some((tmin, n))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mat {
    Ground,
    Stone,
    Plaster,
    Brick,
    Wood,
    Sandbag,
    Hedge,
    Metal,
    Roof,
    Bark,
    Leaves,
    Hay,
}

impl Mat {
    pub fn color(self) -> [f32; 3] {
        match self {
            Mat::Ground => [0.36, 0.38, 0.25],
            Mat::Stone => [0.56, 0.53, 0.47],
            Mat::Plaster => [0.72, 0.67, 0.56],
            Mat::Brick => [0.55, 0.33, 0.25],
            Mat::Wood => [0.47, 0.34, 0.20],
            Mat::Sandbag => [0.60, 0.54, 0.38],
            Mat::Hedge => [0.22, 0.33, 0.16],
            Mat::Metal => [0.27, 0.30, 0.24],
            Mat::Roof => [0.42, 0.23, 0.18],
            Mat::Bark => [0.30, 0.23, 0.16],
            Mat::Leaves => [0.24, 0.36, 0.17],
            Mat::Hay => [0.74, 0.64, 0.36],
        }
    }
}

pub struct Block {
    pub b: Aabb,
    pub mat: Mat,
    pub tint: f32,
}

/// Elementos solo visuales (caminos), sin colisión.
pub struct Decal {
    pub b: Aabb,
    pub color: [f32; 3],
}

pub struct RayHit {
    pub t: f32,
    pub n: V3,
    pub mat: Mat,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    S,
    N,
    W,
    E,
}

pub struct World {
    pub blocks: Vec<Block>,
    pub decals: Vec<Decal>,
    pub half: f32,
    /// Reaparición del bando aliado (sur) y del Eje (norte).
    pub allied_spawns: Vec<V3>,
    pub axis_spawns: Vec<V3>,
    /// Bases de bandera: [Aliados, Eje].
    pub flag_bases: [V3; 2],
    /// Objetivos de Buscar y destruir: [A, B].
    pub bomb_sites: [V3; 2],
    /// Puntos de paso: sirven de patrulla y de "navegación" básica para la IA.
    pub waypoints: Vec<V3>,
    /// Grafo de puntos de paso: vecinos alcanzables en línea recta y su distancia.
    pub links: Vec<Vec<(usize, f32)>>,
}

const ROAD: [f32; 3] = [0.47, 0.42, 0.32];

impl World {
    pub fn body_box(pos: V3, r: f32, h: f32) -> Aabb {
        Aabb {
            min: v3(pos.x - r, pos.y, pos.z - r),
            max: v3(pos.x + r, pos.y + h, pos.z + r),
        }
    }

    pub fn body_free(&self, pos: V3, r: f32, h: f32) -> bool {
        let bb = Self::body_box(pos, r, h);
        pos.x.abs() < self.half - r
            && pos.z.abs() < self.half - r
            && !self.blocks.iter().any(|b| b.b.overlaps(&bb))
    }

    /// Rayo contra el mapa (incluido el suelo). `d` debe estar normalizado.
    pub fn raycast(&self, o: V3, d: V3, max_t: f32) -> Option<RayHit> {
        let mut lim = max_t;
        let mut best: Option<RayHit> = None;
        if d.y < -1e-6 && o.y >= 0.0 {
            let t = -o.y / d.y;
            if t < lim {
                lim = t;
                best = Some(RayHit {
                    t,
                    n: V3::UP,
                    mat: Mat::Ground,
                });
            }
        }
        for b in &self.blocks {
            if let Some((t, n)) = b.b.ray(o, d, lim) {
                if t < lim {
                    lim = t;
                    best = Some(RayHit { t, n, mat: b.mat });
                }
            }
        }
        best
    }

    pub fn los(&self, a: V3, b: V3) -> bool {
        let d = b - a;
        let len = d.len();
        if len < 1e-4 {
            return true;
        }
        self.raycast(a, d / len, len - 0.05).is_none()
    }

    /// Se puede caminar en línea recta (a la altura de rodillas y pecho).
    pub fn walk_clear(&self, a: V3, b: V3) -> bool {
        self.los(v3(a.x, 0.5, a.z), v3(b.x, 0.5, b.z))
            && self.los(v3(a.x, 1.3, a.z), v3(b.x, 1.3, b.z))
    }

    /// Mueve un cuerpo cilíndrico (aprox. caja) deslizando contra las paredes.
    /// Devuelve (posición nueva, en_suelo, bloqueado_horizontalmente).
    pub fn move_body(&self, pos: V3, r: f32, h: f32, delta: V3) -> (V3, bool, bool) {
        let steps = ((delta.len() / 0.2).ceil() as i32).clamp(1, 16);
        let d = delta / steps as f32;
        let mut p = pos;
        let mut grounded = false;
        let mut blocked = false;
        for _ in 0..steps {
            for axis in [0usize, 2, 1] {
                let da = d.get(axis);
                if da == 0.0 {
                    continue;
                }
                p.set(axis, p.get(axis) + da);
                let bb = Self::body_box(p, r, h);
                for b in &self.blocks {
                    if !b.b.overlaps(&bb) {
                        continue;
                    }
                    match axis {
                        1 => {
                            if da > 0.0 {
                                p.y = p.y.min(b.b.min.y - h - 1e-3);
                            } else {
                                p.y = p.y.max(b.b.max.y + 1e-3);
                                grounded = true;
                            }
                        }
                        _ => {
                            let v = if da > 0.0 {
                                (b.b.min.get(axis) - r - 1e-3).min(p.get(axis))
                            } else {
                                (b.b.max.get(axis) + r + 1e-3).max(p.get(axis))
                            };
                            p.set(axis, v);
                            blocked = true;
                        }
                    }
                }
            }
            if p.y < 0.0 {
                p.y = 0.0;
                grounded = true;
            }
            let lim = self.half - r - 0.05;
            if p.x.abs() > lim || p.z.abs() > lim {
                p.x = p.x.clamp(-lim, lim);
                p.z = p.z.clamp(-lim, lim);
                blocked = true;
            }
        }
        if !grounded {
            let mut probe = Self::body_box(p, r * 0.9, h);
            probe.min.y -= 0.03;
            probe.max.y = probe.min.y + 0.05;
            grounded = p.y <= 0.02 || self.blocks.iter().any(|b| b.b.overlaps(&probe));
        }
        (p, grounded, blocked)
    }

    // ------------------------------------------------------------------
    // Construcción del mapa
    // ------------------------------------------------------------------

    fn add(&mut self, a: V3, b: V3, mat: Mat) {
        let bb = Aabb::new(a, b);
        let tint = 0.88 + 0.24 * hash3(bb.min);
        self.blocks.push(Block { b: bb, mat, tint });
    }

    fn add_cs(&mut self, c: V3, s: V3, mat: Mat) {
        let bb = Aabb::cs(c, s);
        self.add(bb.min, bb.max, mat);
    }

    fn decal(&mut self, a: V3, b: V3, color: [f32; 3]) {
        self.decals.push(Decal {
            b: Aabb::new(a, b),
            color,
        });
    }

    /// Muro recto con huecos (puertas, ventanas, brechas).
    /// Cada hueco: (centro desde el inicio del muro, ancho, y_inferior, y_superior).
    #[allow(clippy::too_many_arguments)]
    fn wall(
        &mut self,
        along_x: bool,
        a0: f32,
        a1: f32,
        c: f32,
        t: f32,
        h: f32,
        mut ops: Vec<(f32, f32, f32, f32)>,
        mat: Mat,
    ) {
        ops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let seg = |s: &mut World, p0: f32, p1: f32, y0: f32, y1: f32| {
            if p1 - p0 < 0.01 || y1 - y0 < 0.01 {
                return;
            }
            if along_x {
                s.add(v3(p0, y0, c - t / 2.0), v3(p1, y1, c + t / 2.0), mat);
            } else {
                s.add(v3(c - t / 2.0, y0, p0), v3(c + t / 2.0, y1, p1), mat);
            }
        };
        let mut cur = a0;
        for &(oc, ow, y0, y1) in ops.iter() {
            let s = (a0 + oc - ow / 2.0).max(cur);
            let e = (a0 + oc + ow / 2.0).min(a1);
            seg(self, cur, s, 0.0, h);
            seg(self, s, e, 0.0, y0.min(h));
            if y1 < h {
                seg(self, s, e, y1, h);
            }
            cur = e.max(cur);
        }
        seg(self, cur, a1, 0.0, h);
    }

    #[allow(clippy::too_many_arguments)]
    fn house(
        &mut self,
        rng: &mut Rng,
        cx: f32,
        cz: f32,
        w: f32,
        d: f32,
        door: Side,
        ruined: bool,
    ) {
        let t = 0.4;
        let (x0, x1, z0, z1) = (cx - w / 2.0, cx + w / 2.0, cz - d / 2.0, cz + d / 2.0);
        let mat = if ruined {
            Mat::Stone
        } else if hash3(v3(cx, 1.0, cz)) < 0.5 {
            Mat::Plaster
        } else {
            Mat::Brick
        };
        let base_h = 4.2;
        for side in [Side::S, Side::N, Side::W, Side::E] {
            let (a0, a1) = match side {
                Side::S | Side::N => (x0, x1),
                _ => (z0 + t, z1 - t),
            };
            let len = a1 - a0;
            let mut ops = Vec::new();
            if side == door {
                ops.push((len / 2.0, 1.5, 0.0, 2.3));
                if len > 8.0 {
                    ops.push((len * 0.16, 1.1, 1.0, 2.1));
                    ops.push((len * 0.84, 1.1, 1.0, 2.1));
                }
            } else if len > 5.0 {
                ops.push((len * 0.28, 1.1, 1.0, 2.1));
                ops.push((len * 0.72, 1.1, 1.0, 2.1));
            } else {
                ops.push((len * 0.5, 1.1, 1.0, 2.1));
            }
            let mut hh = base_h;
            if ruined {
                hh = rng.range(2.0, 3.8);
                if side != door && rng.chance(0.45) {
                    ops = vec![(len * rng.range(0.35, 0.65), 2.2, rng.range(0.0, 0.6), 99.0)];
                }
            }
            let c = match side {
                Side::S => z0 + t / 2.0,
                Side::N => z1 - t / 2.0,
                Side::W => x0 + t / 2.0,
                Side::E => x1 - t / 2.0,
            };
            self.wall(
                matches!(side, Side::S | Side::N),
                a0,
                a1,
                c,
                t,
                hh,
                ops,
                mat,
            );
        }
        if ruined {
            // Escombros en el interior
            for _ in 0..5 {
                let c = v3(
                    rng.range(x0 + 1.4, x1 - 1.4),
                    0.0,
                    rng.range(z0 + 1.4, z1 - 1.4),
                );
                let s = v3(rng.range(0.6, 1.6), rng.range(0.3, 0.9), rng.range(0.6, 1.6));
                self.add(v3(c.x - s.x / 2.0, 0.0, c.z - s.z / 2.0), v3(c.x + s.x / 2.0, s.y, c.z + s.z / 2.0), Mat::Stone);
            }
            let bz = rng.range(z0 + 1.5, z1 - 1.5);
            self.add(v3(cx - 1.5, 0.0, bz - 0.13), v3(cx + 1.5, 0.26, bz + 0.13), Mat::Wood);
        } else {
            self.roof(x0, x1, z0, z1, base_h);
            self.add(v3(x0 + 0.8, base_h, cz - 0.35), v3(x0 + 1.5, base_h + 2.6, cz + 0.35), Mat::Brick);
        }
    }

    fn roof(&mut self, x0: f32, x1: f32, z0: f32, z1: f32, top: f32) {
        let o = 0.4;
        self.add(v3(x0 - o, top, z0 - o), v3(x1 + o, top + 0.25, z1 + o), Mat::Roof);
        let ridge_x = (x1 - x0) >= (z1 - z0);
        let (cx, cz) = ((x0 + x1) / 2.0, (z0 + z1) / 2.0);
        for k in 1..4 {
            let f = 1.0 - k as f32 / 4.0;
            let y = top + 0.25 + (k - 1) as f32 * 0.5;
            if ridge_x {
                let hd = ((z1 - z0) / 2.0 + o) * f;
                self.add(v3(x0 - o, y, cz - hd), v3(x1 + o, y + 0.5, cz + hd), Mat::Roof);
            } else {
                let hw = ((x1 - x0) / 2.0 + o) * f;
                self.add(v3(cx - hw, y, z0 - o), v3(cx + hw, y + 0.5, z1 + o), Mat::Roof);
            }
        }
    }

    fn crate_at(&mut self, x: f32, z: f32, level: f32) {
        let y = level * 0.9;
        self.add(v3(x - 0.45, y, z - 0.45), v3(x + 0.45, y + 0.9, z + 0.45), Mat::Wood);
    }

    fn tree(&mut self, x: f32, z: f32) {
        self.add(v3(x - 0.25, 0.0, z - 0.25), v3(x + 0.25, 4.0, z + 0.25), Mat::Bark);
        self.add_cs(v3(x, 4.7, z), v3(3.4, 2.2, 3.4), Mat::Leaves);
        self.add_cs(v3(x + 0.2, 6.3, z - 0.1), v3(2.3, 1.4, 2.3), Mat::Leaves);
    }

    /// Saint-Aubin: el pueblo del juego.
    pub fn village() -> World {
        let mut w = World {
            blocks: Vec::new(),
            decals: Vec::new(),
            half: 64.0,
            allied_spawns: Vec::new(),
            axis_spawns: Vec::new(),
            flag_bases: [v3(0.0, 0.0, -50.0), v3(0.0, 0.0, 53.0)],
            bomb_sites: [v3(-9.5, 0.0, -24.0), v3(31.0, 0.0, -40.0)],
            waypoints: Vec::new(),
            links: Vec::new(),
        };
        let h = w.half;
        let mut rng = Rng::new(1944);

        // Setos del borde (bocage)
        w.add(v3(-h, 0.0, h - 1.5), v3(h, 3.6, h), Mat::Hedge);
        w.add(v3(-h, 0.0, -h), v3(h, 3.6, -h + 1.5), Mat::Hedge);
        w.add(v3(-h, 0.0, -h + 1.5), v3(-h + 1.5, 3.6, h - 1.5), Mat::Hedge);
        w.add(v3(h - 1.5, 0.0, -h + 1.5), v3(h, 3.6, h - 1.5), Mat::Hedge);

        // Caminos
        w.decal(v3(-3.5, 0.0, -h), v3(3.5, 0.02, h), ROAD);
        w.decal(v3(-h, 0.0, 2.0), v3(h, 0.02, 8.0), ROAD);

        // Casas
        let houses = [
            (-13.0, -34.0, 10.0, 8.0, Side::E, false),
            (-13.0, -16.0, 10.0, 8.0, Side::E, true),
            (-13.0, 16.0, 10.0, 8.0, Side::E, false),
            (-13.0, 34.0, 10.0, 9.0, Side::E, true),
            (13.0, -30.0, 10.0, 8.0, Side::W, true),
            (13.0, -13.0, 10.0, 9.0, Side::W, false),
            (13.0, 20.0, 10.0, 8.0, Side::W, false),
            (13.0, 40.0, 10.0, 8.0, Side::W, false),
            (-36.0, -6.0, 12.0, 8.0, Side::S, true),
            (-38.0, 28.0, 9.0, 9.0, Side::E, false),
            (38.0, -14.0, 10.0, 8.0, Side::N, false),
            (36.0, 48.0, 9.0, 8.0, Side::S, true),
            (-34.0, -38.0, 9.0, 7.0, Side::N, true),
        ];
        for &(cx, cz, ww, dd, door, ruined) in houses.iter() {
            w.house(&mut rng, cx, cz, ww, dd, door, ruined);
        }

        // Iglesia
        {
            let (x0, x1, z0, z1, hh, t) = (27.0, 37.0, 14.0, 30.0, 7.5, 0.5);
            w.wall(true, x0, x1, z0 + t / 2.0, t, hh, vec![(5.0, 2.2, 0.0, 3.4)], Mat::Stone);
            w.wall(true, x0, x1, z1 - t / 2.0, t, hh, vec![], Mat::Stone);
            let win = vec![(3.8, 0.8, 2.6, 5.4), (7.6, 0.8, 2.6, 5.4), (11.4, 0.8, 2.6, 5.4)];
            w.wall(false, z0 + t, z1 - t, x0 + t / 2.0, t, hh, win.clone(), Mat::Stone);
            w.wall(false, z0 + t, z1 - t, x1 - t / 2.0, t, hh, win, Mat::Stone);
            w.roof(x0, x1, z0, z1, hh);
            // Torre
            w.add(v3(29.5, 0.0, 30.0), v3(34.5, 17.0, 35.0), Mat::Stone);
            w.add_cs(v3(32.0, 18.25, 32.5), v3(3.4, 2.5, 3.4), Mat::Stone);
            w.add_cs(v3(32.0, 21.25, 32.5), v3(1.6, 3.5, 1.6), Mat::Roof);
            w.add_cs(v3(32.0, 24.0, 32.5), v3(0.5, 2.0, 0.5), Mat::Roof);
            // Bancos
            for k in 0..5 {
                let z = 17.0 + k as f32 * 2.4;
                w.add(v3(28.2, 0.0, z), v3(31.2, 0.5, z + 0.5), Mat::Wood);
                w.add(v3(32.8, 0.0, z), v3(35.8, 0.5, z + 0.5), Mat::Wood);
            }
        }

        // Tanque destruido junto al cruce
        w.add(v3(3.9, 0.0, -5.4), v3(4.5, 1.1, 1.4), Mat::Metal);
        w.add(v3(6.5, 0.0, -5.4), v3(7.1, 1.1, 1.4), Mat::Metal);
        w.add(v3(4.4, 0.35, -5.0), v3(6.6, 1.55, 1.0), Mat::Metal);
        w.add(v3(4.6, 1.55, -3.2), v3(6.4, 2.35, -0.6), Mat::Metal);
        w.add(v3(5.4, 1.85, -6.9), v3(5.6, 2.05, -3.2), Mat::Metal);

        // Sacos terreros
        let sb = 1.05;
        w.add(v3(-9.0, 0.0, -42.6), v3(-3.0, sb, -41.8), Mat::Sandbag);
        w.add(v3(3.0, 0.0, -42.6), v3(9.0, sb, -41.8), Mat::Sandbag);
        w.add(v3(-10.0, 0.0, 47.6), v3(-2.0, sb, 48.4), Mat::Sandbag);
        w.add(v3(2.0, 0.0, 47.6), v3(10.0, sb, 48.4), Mat::Sandbag);
        w.add(v3(-7.5, 0.0, 9.8), v3(-4.0, sb, 10.6), Mat::Sandbag);
        w.add(v3(-7.5, 0.0, 10.6), v3(-6.7, sb, 11.8), Mat::Sandbag);
        w.add(v3(18.0, 0.0, -3.0), v3(22.0, sb, -2.2), Mat::Sandbag);
        w.add(v3(22.0, 0.0, -3.0), v3(22.8, sb, 0.5), Mat::Sandbag);

        // Muretes de piedra entre campos
        let sw = 1.1;
        w.add(v3(-24.25, 0.0, -52.0), v3(-23.75, sw, -38.0), Mat::Stone);
        w.add(v3(-24.25, 0.0, -34.0), v3(-23.75, sw, -24.0), Mat::Stone);
        w.add(v3(-50.0, 0.0, -22.25), v3(-30.0, sw, -21.75), Mat::Stone);
        w.add(v3(23.75, 0.0, -52.0), v3(24.25, sw, -42.0), Mat::Stone);
        w.add(v3(23.75, 0.0, -38.0), v3(24.25, sw, -24.0), Mat::Stone);
        w.add(v3(-50.0, 0.0, 41.75), v3(-30.0, sw, 42.25), Mat::Stone);
        w.add(v3(45.75, 0.0, 12.0), v3(46.25, sw, 22.0), Mat::Stone);
        w.add(v3(45.75, 0.0, 28.0), v3(46.25, sw, 38.0), Mat::Stone);

        // Setos interiores
        let hg = 2.4;
        w.add(v3(-56.0, 0.0, 14.0), v3(-44.0, hg, 15.2), Mat::Hedge);
        w.add(v3(44.0, 0.0, -26.0), v3(56.0, hg, -24.8), Mat::Hedge);
        w.add(v3(-56.0, 0.0, -30.0), v3(-46.0, hg, -28.8), Mat::Hedge);

        // Cajas
        for &(x, z, l) in [
            (-5.0, -28.0, 0.0),
            (-5.0, -27.1, 0.0),
            (-5.0, -27.55, 1.0),
            (5.0, 14.0, 0.0),
            (5.9, 14.0, 0.0),
            (-5.0, 30.0, 0.0),
            (6.0, 32.0, 0.0),
            (6.0, 32.0, 1.0),
            (22.0, 10.0, 0.0),
            (22.9, 10.0, 0.0),
            (22.45, 10.0, 1.0),
            (-28.0, -30.0, 0.0),
            (28.0, -34.0, 0.0),
            (28.0, -33.1, 0.0),
            (18.0, -44.0, 0.0),
            (-45.0, -6.0, 0.0),
            (-1.5, 40.0, 0.0),
        ]
        .iter()
        {
            w.crate_at(x, z, l);
        }
        // Barriles
        for &(x, z) in [(-4.2, -12.0), (4.2, 22.0), (-26.0, 9.5), (26.0, 1.0)].iter() {
            w.add_cs(v3(x, 0.45, z), v3(0.6, 0.9, 0.6), Mat::Metal);
        }
        // Balas de heno y carro
        for &(x, z) in [(-40.0, -48.0), (-38.6, -48.0), (40.0, -48.0), (-46.0, 36.0), (50.0, 0.0)].iter() {
            w.add_cs(v3(x, 0.6, z), v3(1.2, 1.2, 1.2), Mat::Hay);
        }
        w.add_cs(v3(-25.0, 0.45, -46.0), v3(2.2, 0.9, 1.3), Mat::Wood);

        // Pozo
        w.add(v3(-6.9, 0.0, 23.1), v3(-5.1, 0.9, 24.9), Mat::Stone);
        w.add(v3(-6.8, 0.9, 23.9), v3(-6.6, 2.4, 24.1), Mat::Wood);
        w.add(v3(-5.4, 0.9, 23.9), v3(-5.2, 2.4, 24.1), Mat::Wood);
        w.add(v3(-7.0, 2.4, 23.85), v3(-5.0, 2.6, 24.15), Mat::Wood);

        // Árboles
        for &(x, z) in [
            (-30.0, -18.0),
            (-44.0, -34.0),
            (44.0, -36.0),
            (46.0, 8.0),
            (-46.0, 48.0),
            (-24.0, 46.0),
            (21.0, -46.0),
            (50.0, 32.0),
            (-50.0, 6.0),
            (-18.0, -52.0),
        ]
        .iter()
        {
            w.tree(x, z);
        }

        // Puntos de paso (navegación y patrulla)
        w.waypoints = vec![
            v3(0.0, 0.0, -48.0),
            v3(0.0, 0.0, -36.0),
            v3(0.0, 0.0, -24.0),
            v3(0.0, 0.0, -12.0),
            v3(0.0, 0.0, 5.0),
            v3(0.0, 0.0, 18.0),
            v3(0.0, 0.0, 30.0),
            v3(0.0, 0.0, 44.0),
            v3(0.0, 0.0, 55.0),
            v3(-50.0, 0.0, 5.0),
            v3(-38.0, 0.0, 5.0),
            v3(-24.0, 0.0, 5.0),
            v3(-12.0, 0.0, 5.0),
            v3(12.0, 0.0, 5.0),
            v3(24.0, 0.0, 5.0),
            v3(38.0, 0.0, 5.0),
            v3(54.0, 0.0, 5.0),
            v3(-24.0, 0.0, -12.0),
            v3(-24.0, 0.0, 18.0),
            v3(-24.0, 0.0, 32.0),
            v3(-48.0, 0.0, -20.0),
            v3(-48.0, 0.0, 30.0),
            v3(-50.0, 0.0, 50.0),
            v3(-48.0, 0.0, -45.0),
            v3(22.0, 0.0, -20.0),
            v3(22.0, 0.0, 30.0),
            v3(22.0, 0.0, 46.0),
            v3(40.0, 0.0, -30.0),
            v3(50.0, 0.0, -5.0),
            v3(52.0, 0.0, 25.0),
            v3(54.0, 0.0, 45.0),
            v3(-25.0, 0.0, 52.0),
            v3(-10.0, 0.0, 55.0),
            v3(15.0, 0.0, 53.0),
            v3(28.0, 0.0, 57.0),
            v3(-15.0, 0.0, -46.0),
            v3(15.0, 0.0, -46.0),
            v3(-32.0, 0.0, -52.0),
            v3(-28.0, 0.0, -44.0),
            v3(-24.0, 0.0, -36.0),
            v3(34.0, 0.0, -50.0),
            v3(-9.5, 0.0, -25.0),
            v3(9.5, 0.0, -21.5),
            v3(-8.0, 0.0, 25.0),
            v3(9.0, 0.0, 30.0),
        ];
        w.allied_spawns = vec![
            v3(0.0, 0.0, -58.0),
            v3(-6.0, 0.0, -56.0),
            v3(6.0, 0.0, -56.0),
            v3(-12.0, 0.0, -52.0),
            v3(12.0, 0.0, -52.0),
            v3(-9.0, 0.0, -59.0),
            v3(9.0, 0.0, -59.0),
            v3(-18.0, 0.0, -57.0),
            v3(18.0, 0.0, -57.0),
            v3(-3.0, 0.0, -48.0),
            v3(3.0, 0.0, -48.0),
            v3(25.0, 0.0, -56.0),
        ];
        w.axis_spawns = vec![
            v3(0.0, 0.0, 58.0),
            v3(-6.0, 0.0, 57.0),
            v3(6.0, 0.0, 57.0),
            v3(-14.0, 0.0, 55.0),
            v3(14.0, 0.0, 55.0),
            v3(-3.0, 0.0, 60.0),
            v3(3.0, 0.0, 60.0),
            v3(-20.0, 0.0, 57.0),
            v3(20.0, 0.0, 57.0),
            v3(-28.0, 0.0, 58.0),
            v3(28.0, 0.0, 58.0),
            v3(-40.0, 0.0, 56.0),
        ];
        w.build_links();
        w
    }

    /// Siguiente punto hacia el que caminar para llegar a `goal`
    /// (Dijkstra sobre el grafo de puntos de paso).
    pub fn nav_target(&self, from: V3, goal: V3) -> V3 {
        if self.walk_clear(from, goal) {
            return goal;
        }
        let wp = &self.waypoints;
        let n = wp.len();
        let mut dist = vec![f32::MAX; n];
        let mut prev = vec![usize::MAX; n];
        let mut done = vec![false; n];
        for i in 0..n {
            if self.walk_clear(from, wp[i]) {
                dist[i] = from.dist_flat(wp[i]);
            }
        }
        let ends: Vec<bool> = (0..n).map(|i| self.walk_clear(wp[i], goal)).collect();
        let mut best_end: Option<(usize, f32)> = None;
        loop {
            let mut u = usize::MAX;
            let mut ud = f32::MAX;
            for i in 0..n {
                if !done[i] && dist[i] < ud {
                    ud = dist[i];
                    u = i;
                }
            }
            if u == usize::MAX {
                break;
            }
            done[u] = true;
            if ends[u] {
                let total = ud + wp[u].dist_flat(goal);
                if best_end.map(|b| total < b.1).unwrap_or(true) {
                    best_end = Some((u, total));
                }
            }
            for &(v, w) in &self.links[u] {
                if ud + w < dist[v] {
                    dist[v] = ud + w;
                    prev[v] = u;
                }
            }
        }
        let Some((end, _)) = best_end else { return goal };
        let mut path = vec![end];
        while let Some(&last) = path.last() {
            if prev[last] == usize::MAX {
                break;
            }
            path.push(prev[last]);
        }
        path.reverse();
        path.into_iter().map(|i| wp[i]).find(|p| p.dist_flat(from) > 1.2).unwrap_or(goal)
    }

    fn build_links(&mut self) {
        let n = self.waypoints.len();
        let mut links = vec![Vec::new(); n];
        for i in 0..n {
            for j in (i + 1)..n {
                let (a, b) = (self.waypoints[i], self.waypoints[j]);
                if self.walk_clear(a, b) {
                    let d = a.dist_flat(b);
                    links[i].push((j, d));
                    links[j].push((i, d));
                }
            }
        }
        self.links = links;
    }
}

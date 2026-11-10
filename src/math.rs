//! Matemáticas mínimas (vector 3D y generador aleatorio) sin dependencias,
//! para que la lógica del juego se pueda compilar y probar sin motor gráfico.

use std::f32::consts::{PI, TAU};
use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub const ZERO: V3 = v3(0.0, 0.0, 0.0);
    pub const UP: V3 = v3(0.0, 1.0, 0.0);
    pub const X: V3 = v3(1.0, 0.0, 0.0);
    pub const Z: V3 = v3(0.0, 0.0, 1.0);

    pub fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn len_sq(self) -> f32 {
        self.dot(self)
    }
    pub fn len(self) -> f32 {
        self.len_sq().sqrt()
    }
    pub fn norm(self) -> V3 {
        let l = self.len();
        if l > 1e-6 {
            self / l
        } else {
            V3::ZERO
        }
    }
    pub fn flat(self) -> V3 {
        v3(self.x, 0.0, self.z)
    }
    pub fn dist(self, o: V3) -> f32 {
        (self - o).len()
    }
    pub fn dist_flat(self, o: V3) -> f32 {
        (self - o).flat().len()
    }
    pub fn lerp(self, o: V3, t: f32) -> V3 {
        self + (o - self) * t
    }
    pub fn get(self, axis: usize) -> f32 {
        match axis {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
    pub fn set(&mut self, axis: usize, v: f32) {
        match axis {
            0 => self.x = v,
            1 => self.y = v,
            _ => self.z = v,
        }
    }
    pub fn axis(axis: usize) -> V3 {
        match axis {
            0 => V3::X,
            1 => V3::UP,
            _ => V3::Z,
        }
    }
    /// Dirección horizontal a partir del ángulo de guiñada (0 = +X, PI/2 = +Z).
    pub fn from_yaw(yaw: f32) -> V3 {
        v3(yaw.cos(), 0.0, yaw.sin())
    }
    pub fn from_yaw_pitch(yaw: f32, pitch: f32) -> V3 {
        v3(pitch.cos() * yaw.cos(), pitch.sin(), pitch.cos() * yaw.sin())
    }
    /// Vector "derecha" correspondiente a una guiñada (sistema dextrógiro, Y arriba).
    pub fn right_of_yaw(yaw: f32) -> V3 {
        v3(-yaw.sin(), 0.0, yaw.cos())
    }
    pub fn yaw(self) -> f32 {
        self.z.atan2(self.x)
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, s: f32) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Div<f32> for V3 {
    type Output = V3;
    fn div(self, s: f32) -> V3 {
        v3(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}
impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl SubAssign for V3 {
    fn sub_assign(&mut self, o: V3) {
        *self = *self - o;
    }
}
impl MulAssign<f32> for V3 {
    fn mul_assign(&mut self, s: f32) {
        *self = *self * s;
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn wrap_angle(a: f32) -> f32 {
    if !a.is_finite() {
        return 0.0;
    }
    (a + PI).rem_euclid(TAU) - PI
}

pub fn approach(cur: f32, target: f32, step: f32) -> f32 {
    if cur < target {
        (cur + step).min(target)
    } else {
        (cur - step).max(target)
    }
}

pub fn approach_angle(cur: f32, target: f32, step: f32) -> f32 {
    let d = wrap_angle(target - cur);
    if d.abs() <= step {
        wrap_angle(target)
    } else {
        wrap_angle(cur + step * d.signum())
    }
}

/// Distancia mínima entre un punto y un segmento.
pub fn seg_point_dist(a: V3, b: V3, p: V3) -> f32 {
    let ab = b - a;
    let l2 = ab.len_sq();
    if l2 < 1e-8 {
        return a.dist(p);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (a + ab * t).dist(p)
}

/// Hash determinista de una posición, en [0, 1).
pub fn hash3(v: V3) -> f32 {
    let mut h = (v.x * 73.13).to_bits() as u64;
    h ^= ((v.y * 19.91).to_bits() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= ((v.z * 41.57).to_bits() as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 40) as f32 / 16_777_216.0
}

/// Xorshift64* — rápido, determinista y suficiente para un juego.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        r.next_u64();
        r
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Número en [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16_777_216.0
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
    pub fn index(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
    pub fn sign(&mut self) -> f32 {
        if self.chance(0.5) {
            1.0
        } else {
            -1.0
        }
    }
    /// Dirección aleatoria uniforme en la esfera.
    pub fn dir(&mut self) -> V3 {
        loop {
            let v = v3(
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
            );
            let l = v.len_sq();
            if l > 0.01 && l <= 1.0 {
                return v.norm();
            }
        }
    }
}

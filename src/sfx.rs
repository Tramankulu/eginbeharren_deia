//! Efectos de sonido sintetizados por código (sin archivos externos).
//! Cada función devuelve muestras mono en [-1, 1]; `to_wav` las empaqueta en WAV.

use crate::math::Rng;
use std::f32::consts::TAU;

pub const RATE: u32 = 44100;

pub fn to_wav(s: &[f32]) -> Vec<u8> {
    let data_len = (s.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for &x in s {
        let i = (x.clamp(-1.0, 1.0) * 32000.0) as i16;
        v.extend_from_slice(&i.to_le_bytes());
    }
    v
}

fn gen(dur: f32, mut f: impl FnMut(f32) -> f32) -> Vec<f32> {
    let n = (dur * RATE as f32) as usize;
    (0..n).map(|i| f(i as f32 / RATE as f32)).collect()
}

/// Filtro paso bajo de un polo.
struct Lp {
    y: f32,
    a: f32,
}
impl Lp {
    fn new(cut: f32) -> Lp {
        Lp { y: 0.0, a: 1.0 - (-TAU * cut / RATE as f32).exp() }
    }
    fn run(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

fn normalize(mut v: Vec<f32>, peak: f32) -> Vec<f32> {
    let m = v.iter().fold(0.0f32, |a, &x| a.max(x.abs()));
    if m > 1e-6 {
        let k = peak / m;
        for x in v.iter_mut() {
            *x *= k;
        }
    }
    v
}

/// Filtro paso alto de un polo.
struct Hp {
    lp: Lp,
}
impl Hp {
    fn new(cut: f32) -> Hp {
        Hp { lp: Lp::new(cut) }
    }
    fn run(&mut self, x: f32) -> f32 {
        x - self.lp.run(x)
    }
}

/// Reverberación de Schroeder (peines con amortiguación y pasatodos): la cola
/// que devuelven las casas y el campo abierto.
fn reverb(dry: &[f32], extra: f32, size: f32, damp: f32, wet: f32) -> Vec<f32> {
    let n = dry.len() + (extra * RATE as f32) as usize;
    let mut out = vec![0.0f32; n];
    let combs = [0.0297f32, 0.0371, 0.0411, 0.0437, 0.0533, 0.0617];
    for (k, &d) in combs.iter().enumerate() {
        let len = ((d * size) * RATE as f32) as usize;
        let mut buf = vec![0.0f32; len.max(1)];
        let mut idx = 0;
        let mut f = 0.0f32;
        let fb = 0.84 - 0.02 * k as f32;
        for i in 0..n {
            let x = dry.get(i).copied().unwrap_or(0.0);
            let y = buf[idx];
            f = y * (1.0 - damp) + f * damp;
            buf[idx] = x + f * fb;
            idx = (idx + 1) % buf.len();
            out[i] += y / combs.len() as f32;
        }
    }
    for &d in &[0.005f32, 0.0017] {
        let len = (d * RATE as f32) as usize;
        let mut buf = vec![0.0f32; len.max(1)];
        let mut idx = 0;
        for x in out.iter_mut() {
            let b = buf[idx];
            let y = -*x + b;
            buf[idx] = *x + b * 0.5;
            idx = (idx + 1) % buf.len();
            *x = y;
        }
    }
    for i in 0..n {
        out[i] = dry.get(i).copied().unwrap_or(0.0) + out[i] * wet;
    }
    out
}

/// Receta de un disparo.
struct Shot {
    seed: u64,
    /// Chasquido supersónico de la bala (0 en las balas subsónicas, como la .45)
    crack: f32,
    /// Grave del estampido en la boca del cañón (Hz) y su duración
    boom_hz: f32,
    boom_decay: f32,
    /// Cuerpo de ruido del estampido: brillo y duración
    body_cut: f32,
    body_decay: f32,
    /// Saturación (los tiros reales saturan micrófono y oído)
    drive: f32,
    /// Mecanismo: chasquido metálico del cierre al ciclar (0 = nada) y cuándo
    mech: f32,
    mech_at: f32,
    /// Ecos y cola de reverberación
    room: f32,
    tail: f32,
    /// Lejanía: 0 = al lado, 1 = muy lejos (sin agudos y casi todo eco)
    far: f32,
}

fn gunshot(s: Shot) -> Vec<f32> {
    let mut r = Rng::new(s.seed);
    let mut crack_hp = Hp::new(3000.0);
    let mut body_lp = Lp::new(s.body_cut);
    let mut body_lp2 = Lp::new(s.body_cut * 0.25);
    let mut mech_hp = Hp::new(1800.0);
    let dry_len = 0.35f32.max(s.mech_at + 0.08);
    let mut boom_phase = 0.0f32;
    let mut dry = gen(dry_len, |t| {
        let n = r.f() * 2.0 - 1.0;
        // Chasquido: un clic de banda ancha de un milisegundo
        let crack = crack_hp.run(n) * (-t / 0.0007).exp() * 3.0 * s.crack;
        // Estampido: ruido grave y medio con ataque instantáneo
        let atk = (t / 0.0004).min(1.0);
        let body = (body_lp.run(n) * 1.4 * (-t / s.body_decay).exp() + body_lp2.run(n) * 2.2 * (-t / (s.body_decay * 2.2)).exp()) * atk;
        // Golpe de presión: un seno que baja de tono
        let f = s.boom_hz * (1.0 + 1.5 * (-t / 0.01).exp());
        boom_phase += TAU * f / RATE as f32;
        let boom = boom_phase.sin() * (-t / s.boom_decay).exp() * 1.3 * atk;
        let mut x = crack + body + boom;
        x = (x * s.drive).tanh() / s.drive.tanh();
        // Mecanismo: dos golpecitos metálicos
        if s.mech > 0.0 {
            for (dt, g) in [(0.0f32, 1.0f32), (0.022, 0.6)] {
                let u = t - s.mech_at - dt;
                if u >= 0.0 {
                    let ring = ((TAU * 3100.0 * u).sin() + 0.6 * (TAU * 4700.0 * u).sin()) * (-u / 0.012).exp();
                    x += (mech_hp.run(n) * 0.6 + ring * 0.4) * (-u / 0.01).exp() * s.mech * g * 0.25;
                }
            }
        }
        x
    });
    // Lejanía: el aire se come los agudos
    if s.far > 0.0 {
        let mut lp = Lp::new(lerp_f(6000.0, 700.0, s.far));
        let mut lp2 = Lp::new(lerp_f(9000.0, 1100.0, s.far));
        for x in dry.iter_mut() {
            *x = lp2.run(lp.run(*x));
        }
    }
    // Primeros rebotes en las fachadas (cada vez más apagados)
    let mut v = dry.clone();
    v.resize(dry.len() + (0.5 * RATE as f32) as usize, 0.0);
    let echoes = [(0.043f32, 0.32f32), (0.071, 0.22), (0.118, 0.2), (0.19, 0.14), (0.27, 0.1), (0.38, 0.07)];
    for (k, &(d, g)) in echoes.iter().enumerate() {
        let o = (d * RATE as f32) as usize;
        let mut lp = Lp::new(3500.0 / (1.0 + k as f32 * 0.7));
        for i in 0..dry.len() {
            if i + o < v.len() {
                v[i + o] += lp.run(dry[i]) * g * s.room * (1.0 + s.far);
            }
        }
    }
    let wet = s.room * (0.5 + 1.5 * s.far);
    let mut out = reverb(&v, s.tail, 1.0 + 0.6 * s.far, 0.35 + 0.3 * s.far, wet);
    // Fundido final suave para que no corte
    let n = out.len();
    let fade = (0.15 * RATE as f32) as usize;
    for i in 0..fade.min(n) {
        out[n - 1 - i] *= i as f32 / fade as f32;
    }
    normalize(out, 0.95)
}

fn lerp_f(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Fusil semiautomático (M1 Garand): estampido pleno y el cierre que cicla.
pub fn rifle() -> Vec<f32> {
    gunshot(Shot { seed: 1, crack: 1.0, boom_hz: 75.0, boom_decay: 0.05, body_cut: 4200.0, body_decay: 0.045, drive: 3.0, mech: 1.0, mech_at: 0.045, room: 0.55, tail: 1.1, far: 0.0 })
}
/// Subfusil Thompson: bala subsónica (.45), golpe grave y corto, sin chasquido.
pub fn smg() -> Vec<f32> {
    gunshot(Shot { seed: 2, crack: 0.15, boom_hz: 105.0, boom_decay: 0.03, body_cut: 3200.0, body_decay: 0.028, drive: 2.5, mech: 0.6, mech_at: 0.03, room: 0.45, tail: 0.55, far: 0.0 })
}
/// Disparos lejanos: casi todo eco, sin agudos.
pub fn enemy_rifle() -> Vec<f32> {
    gunshot(Shot { seed: 3, crack: 0.6, boom_hz: 70.0, boom_decay: 0.06, body_cut: 3500.0, body_decay: 0.05, drive: 2.0, mech: 0.0, mech_at: 0.0, room: 0.7, tail: 1.6, far: 0.75 })
}
pub fn enemy_smg() -> Vec<f32> {
    gunshot(Shot { seed: 4, crack: 0.2, boom_hz: 100.0, boom_decay: 0.035, body_cut: 3000.0, body_decay: 0.03, drive: 2.0, mech: 0.0, mech_at: 0.0, room: 0.6, tail: 0.9, far: 0.7 })
}

pub fn explosion() -> Vec<f32> {
    let mut r = Rng::new(5);
    let mut lp = Lp::new(700.0);
    let mut lp2 = Lp::new(110.0);
    normalize(
        gen(2.2, |t| {
            let n = r.f() * 2.0 - 1.0;
            let crack = lp.run(n) * (-t / 0.35).exp();
            let rumble = lp2.run(n) * (-t / 0.9).exp() * 3.5;
            let th = (TAU * 42.0 * t).sin() * (-t / 0.25).exp() * 0.8;
            (crack + rumble + th) * (t / 0.004).min(1.0)
        }),
        1.0,
    )
}

/// El "ping" del peine del Garand al quedarse vacío.
pub fn ping() -> Vec<f32> {
    let mut r = Rng::new(6);
    normalize(
        gen(0.9, |t| {
            let a = (TAU * 2750.0 * t).sin() * (-t / 0.22).exp() * 0.5;
            let b = (TAU * 4180.0 * t).sin() * (-t / 0.1).exp() * 0.3;
            let c = (TAU * 1370.0 * t).sin() * (-t / 0.3).exp() * 0.15;
            let click = (r.f() * 2.0 - 1.0) * (-t / 0.003).exp() * 0.5;
            a + b + c + click
        }),
        0.55,
    )
}

fn clicks(seed: u64, times: &[f32], dur: f32, peak: f32) -> Vec<f32> {
    let mut r = Rng::new(seed);
    let mut lp = Lp::new(1500.0);
    let times = times.to_vec();
    normalize(
        gen(dur, |t| {
            let n = r.f() * 2.0 - 1.0;
            let hp = n - lp.run(n);
            let mut env = 0.0;
            for &t0 in &times {
                if t >= t0 {
                    env += (-(t - t0) / 0.012).exp();
                }
            }
            let tone = times
                .iter()
                .filter(|&&t0| t >= t0)
                .map(|&t0| (TAU * 900.0 * (t - t0)).sin() * (-(t - t0) / 0.02).exp() * 0.4)
                .sum::<f32>();
            hp * env + tone
        }),
        peak,
    )
}

pub fn reload() -> Vec<f32> {
    clicks(7, &[0.0, 0.08, 0.7, 1.5, 1.6], 1.8, 0.45)
}
pub fn dry() -> Vec<f32> {
    clicks(8, &[0.0], 0.1, 0.4)
}
pub fn switch_gun() -> Vec<f32> {
    clicks(9, &[0.0, 0.18], 0.35, 0.35)
}

pub fn hitmarker() -> Vec<f32> {
    normalize(
        gen(0.07, |t| (TAU * 1900.0 * t).sin() * (-t / 0.012).exp()),
        0.35,
    )
}

pub fn hurt() -> Vec<f32> {
    let mut r = Rng::new(10);
    let mut lp = Lp::new(280.0);
    normalize(
        gen(0.35, |t| {
            let n = r.f() * 2.0 - 1.0;
            lp.run(n) * (-t / 0.1).exp() * 2.0 + (TAU * 85.0 * t).sin() * (-t / 0.09).exp()
        }),
        0.8,
    )
}

pub fn pickup() -> Vec<f32> {
    normalize(
        gen(0.22, |t| {
            let f = if t < 0.09 { 660.0 } else { 990.0 };
            let lt = if t < 0.09 { t } else { t - 0.09 };
            (TAU * f * t).sin() * (-lt / 0.05).exp()
        }),
        0.35,
    )
}

/// Silbido de bala que pasa cerca.
pub fn whiz() -> Vec<f32> {
    let mut r = Rng::new(11);
    let mut lp = Lp::new(3000.0);
    let mut lp2 = Lp::new(900.0);
    normalize(
        gen(0.22, |t| {
            let n = r.f() * 2.0 - 1.0;
            let band = lp.run(n) - lp2.run(n);
            let env = (-((t - 0.06) / 0.035).powi(2)).exp();
            band * env
        }),
        0.5,
    )
}

pub fn bounce() -> Vec<f32> {
    normalize(
        gen(0.12, |t| {
            ((TAU * 1900.0 * t).sin() + (TAU * 3150.0 * t).sin() * 0.6) * (-t / 0.025).exp()
        }),
        0.3,
    )
}

pub fn throw() -> Vec<f32> {
    let mut r = Rng::new(12);
    let mut lp = Lp::new(1200.0);
    normalize(
        gen(0.35, |t| {
            let n = r.f() * 2.0 - 1.0;
            lp.run(n) * (t / 0.15).min(1.0) * (-(t - 0.15).max(0.0) / 0.08).exp()
        }),
        0.3,
    )
}

/// Silbato de oficial al empezar una oleada.
pub fn whistle() -> Vec<f32> {
    normalize(
        gen(1.0, |t| {
            let env = (t / 0.03).min(1.0) * if t > 0.8 { (1.0 - (t - 0.8) / 0.2).max(0.0) } else { 1.0 };
            // Frecuencia 2450 Hz con trino de ±140 Hz a 28 Hz (fase integrada)
            let phase = TAU * 2450.0 * t + (140.0 / 28.0) * (1.0 - (TAU * 28.0 * t).cos());
            phase.sin() * env
        }),
        0.45,
    )
}

/// Fusil de cerrojo (Kar98k, Mosin, Lee-Enfield): el estampido más seco y potente.
pub fn kar98k() -> Vec<f32> {
    gunshot(Shot { seed: 13, crack: 1.2, boom_hz: 68.0, boom_decay: 0.06, body_cut: 4600.0, body_decay: 0.05, drive: 3.5, mech: 0.0, mech_at: 0.0, room: 0.6, tail: 1.3, far: 0.0 })
}
/// Subfusil de 9 mm (MP40, Sten): más agudo y seco que la Thompson.
pub fn mp40() -> Vec<f32> {
    gunshot(Shot { seed: 14, crack: 0.45, boom_hz: 120.0, boom_decay: 0.025, body_cut: 4200.0, body_decay: 0.024, drive: 2.6, mech: 0.7, mech_at: 0.025, room: 0.45, tail: 0.5, far: 0.0 })
}
/// Cerrojo del Kar98k tras el disparo.
pub fn bolt() -> Vec<f32> {
    clicks(15, &[0.0, 0.25, 0.45], 0.6, 0.35)
}

pub fn pistol() -> Vec<f32> {
    gunshot(Shot { seed: 11, crack: 0.4, boom_hz: 140.0, boom_decay: 0.022, body_cut: 5000.0, body_decay: 0.02, drive: 2.4, mech: 0.8, mech_at: 0.02, room: 0.5, tail: 0.7, far: 0.0 })
}
/// Ametralladora ligera: más grave y con más cola que un subfusil.
pub fn lmg() -> Vec<f32> {
    gunshot(Shot { seed: 12, crack: 1.0, boom_hz: 72.0, boom_decay: 0.045, body_cut: 3800.0, body_decay: 0.04, drive: 3.2, mech: 0.7, mech_at: 0.035, room: 0.5, tail: 0.7, far: 0.0 })
}
/// Fusil de francotirador: un estampido largo que rebota en el valle.
pub fn sniper() -> Vec<f32> {
    gunshot(Shot { seed: 15, crack: 1.3, boom_hz: 64.0, boom_decay: 0.07, body_cut: 4800.0, body_decay: 0.055, drive: 3.6, mech: 0.0, mech_at: 0.0, room: 0.85, tail: 2.2, far: 0.1 })
}

/// Latido (cuando la salud está muy baja).
pub fn heartbeat() -> Vec<f32> {
    normalize(
        gen(0.9, |t| {
            let beat = |t0: f32, a: f32| {
                let u = t - t0;
                if u < 0.0 {
                    0.0
                } else {
                    (TAU * (55.0 - 20.0 * u) * u).sin() * (-u / 0.07).exp() * a
                }
            };
            beat(0.0, 1.0) + beat(0.22, 0.7)
        }),
        0.9,
    )
}

/// Gruñido de zombi.
pub fn growl() -> Vec<f32> {
    let mut r = Rng::new(21);
    let mut lp = Lp::new(500.0);
    normalize(
        gen(1.1, |t| {
            let f = 85.0 + 25.0 * (t * 7.0).sin();
            let voice = (TAU * f * t).sin().signum() * 0.4 + (TAU * f * 2.01 * t).sin() * 0.3;
            let rasp = lp.run(r.f() * 2.0 - 1.0) * 1.6;
            let env = (t / 0.1).min(1.0) * (1.0 - t / 1.1).max(0.0) * (0.7 + 0.3 * (t * 23.0).sin());
            (voice + rasp) * env
        }),
        0.8,
    )
}

/// Zarpazo o mordisco.
pub fn claw() -> Vec<f32> {
    let mut r = Rng::new(22);
    let mut lp = Lp::new(2500.0);
    normalize(
        gen(0.3, |t| {
            let n = lp.run(r.f() * 2.0 - 1.0);
            let thud = (TAU * 90.0 * t).sin() * (-t / 0.05).exp();
            (n * (-t / 0.08).exp() * 1.4 + thud) * (t / 0.004).min(1.0)
        }),
        0.85,
    )
}

/// Subida de rango.
pub fn rank_up() -> Vec<f32> {
    normalize(
        gen(1.0, |t| {
            let note = |t0: f32, f: f32| {
                let u = t - t0;
                if u < 0.0 {
                    0.0
                } else {
                    ((TAU * f * u).sin() + 0.4 * (TAU * f * 2.0 * u).sin()) * (-u / 0.35).exp() * (u / 0.01).min(1.0)
                }
            };
            note(0.0, 523.0) + note(0.12, 659.0) + note(0.24, 784.0) + note(0.36, 1047.0) * 0.8
        }),
        0.6,
    )
}

/// WAV estéreo a partir de un sonido mono, colocado a la izquierda (-1) o a
/// la derecha (1). El oído lejano lo recibe algo después y más apagado, como
/// en la realidad: así se distingue de dónde viene un disparo.
pub fn to_wav_stereo(s: &[f32], pan: f32) -> Vec<u8> {
    let pan = pan.clamp(-1.0, 1.0);
    let a = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    let (gl, gr) = (a.cos() * 1.2, a.sin() * 1.2);
    let delay = (pan.abs() * 0.0006 * RATE as f32) as usize;
    let mut far = Lp::new(3500.0 - 2000.0 * pan.abs());
    let n = s.len() + delay;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    for i in 0..n {
        let x = s.get(i).copied().unwrap_or(0.0);
        let late = if i >= delay { far.run(s.get(i - delay).copied().unwrap_or(0.0)) } else { 0.0 };
        if pan >= 0.0 {
            r[i] = x * gr;
            l[i] = late * gl;
        } else {
            l[i] = x * gl;
            r[i] = late * gr;
        }
    }
    let data_len = (n * 4) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        for x in [l[i], r[i]] {
            v.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32000.0) as i16).to_le_bytes());
        }
    }
    v
}

// ----------------------------------------------------------------------
// Pasos, saltos, impactos
// ----------------------------------------------------------------------

/// Paso sobre tierra y hierba (`k` cambia la pisada para que no suenen iguales).
pub fn footstep(k: u64) -> Vec<f32> {
    let mut r = Rng::new(100 + k);
    let mut lp = Lp::new(900.0 + 250.0 * k as f32);
    let mut lp2 = Lp::new(180.0);
    let crunch = 0.5 + 0.15 * k as f32;
    normalize(
        gen(0.16, |t| {
            let n = r.f() * 2.0 - 1.0;
            // Talón y luego la punta
            let heel = (-t / 0.018).exp();
            let toe = (-(t - 0.05).max(0.0) / 0.03).exp() * if t > 0.05 { 0.6 } else { 0.0 };
            let thump = lp2.run(n) * 3.0 * heel;
            (lp.run(n) * crunch * (heel + toe) + thump) * (t / 0.002).min(1.0)
        }),
        0.5,
    )
}

/// Salto: roce de la ropa y el equipo que tintinea.
pub fn jump() -> Vec<f32> {
    let mut r = Rng::new(130);
    let mut lp = Lp::new(1500.0);
    normalize(
        gen(0.3, |t| {
            let cloth = lp.run(r.f() * 2.0 - 1.0) * (t / 0.05).min(1.0) * (-t / 0.1).exp();
            let gear = ((TAU * 2300.0 * t).sin() + (TAU * 3400.0 * t).sin()) * (-(t - 0.04).max(0.0) / 0.02).exp() * if t > 0.04 { 0.15 } else { 0.0 };
            cloth + gear
        }),
        0.35,
    )
}

/// Caída al suelo tras un salto.
pub fn land() -> Vec<f32> {
    let mut r = Rng::new(131);
    let mut lp = Lp::new(500.0);
    let mut lp2 = Lp::new(2200.0);
    normalize(
        gen(0.28, |t| {
            let n = r.f() * 2.0 - 1.0;
            let thud = lp.run(n) * 3.0 * (-t / 0.05).exp();
            let gear = lp2.run(n) * (-(t - 0.02).max(0.0) / 0.04).exp() * 0.6;
            (thud + gear) * (t / 0.002).min(1.0)
        }),
        0.6,
    )
}

/// Bala que da en piedra o tierra cerca.
pub fn impact() -> Vec<f32> {
    let mut r = Rng::new(140);
    let mut lp = Lp::new(3500.0);
    normalize(
        gen(0.18, |t| {
            let n = lp.run(r.f() * 2.0 - 1.0);
            let ping = (TAU * (2600.0 - 3000.0 * t) * t).sin() * (-t / 0.03).exp() * 0.4;
            (n * (-t / 0.03).exp() + ping) * (t / 0.001).min(1.0)
        }),
        0.45,
    )
}

/// Seguro de la granada al quitarlo.
pub fn pin() -> Vec<f32> {
    normalize(
        gen(0.2, |t| {
            let a = (TAU * 3800.0 * t).sin() * (-t / 0.02).exp();
            let b = (TAU * 5100.0 * (t - 0.06).max(0.0)).sin() * (-(t - 0.06).max(0.0) / 0.03).exp() * if t > 0.06 { 0.7 } else { 0.0 };
            a + b
        }),
        0.3,
    )
}

// ----------------------------------------------------------------------
// Ambiente y música
// ----------------------------------------------------------------------

/// Bucle que empalma sin corte: los extremos se funden entre sí.
fn seamless(mut v: Vec<f32>, fade: usize) -> Vec<f32> {
    let n = v.len();
    let fade = fade.min(n / 2);
    let tail: Vec<f32> = v[n - fade..].to_vec();
    for i in 0..fade {
        let k = i as f32 / fade as f32;
        v[i] = v[i] * k + tail[i] * (1.0 - k);
    }
    v.truncate(n - fade);
    v
}

/// Ambiente de campo de batalla lejano: viento, pájaros, alguna ráfaga y
/// cañonazos a lo lejos. 40 s en bucle.
pub fn ambience() -> Vec<f32> {
    let dur = 42.0;
    let n = (dur * RATE as f32) as usize;
    let mut v = vec![0.0f32; n];
    let mut r = Rng::new(200);
    // Viento: ruido filtrado que sube y baja
    let mut lp = Lp::new(260.0);
    let mut lp2 = Lp::new(60.0);
    for (i, x) in v.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        let gust = 0.55 + 0.3 * (t * 0.21).sin() + 0.15 * (t * 0.57 + 1.0).sin();
        let w = lp.run(r.f() * 2.0 - 1.0) * 1.6 + lp2.run(r.f() * 2.0 - 1.0) * 2.5;
        *x = w * gust * 0.35;
    }
    let mut add = |at: f32, s: &[f32], g: f32| {
        let o = (at * RATE as f32) as usize;
        for (k, x) in s.iter().enumerate() {
            if o + k < n {
                v[o + k] += x * g;
            }
        }
    };
    // Pájaros: trinos cortos
    let mut t = 1.0;
    while t < dur - 2.0 {
        let base = r.range(2600.0, 4200.0);
        let notes = 2 + r.index(4);
        let mut chirp = Vec::new();
        for k in 0..notes {
            let f0 = base * r.range(0.85, 1.15);
            let len = r.range(0.05, 0.12);
            for i in 0..(len * RATE as f32) as usize {
                let u = i as f32 / RATE as f32;
                let f = f0 + 900.0 * (u / len) * if k % 2 == 0 { 1.0 } else { -1.0 };
                chirp.push((TAU * f * u).sin() * (u / len * std::f32::consts::PI).sin());
            }
            chirp.extend(std::iter::repeat(0.0).take((r.range(0.03, 0.09) * RATE as f32) as usize));
        }
        add(t, &chirp, r.range(0.04, 0.09));
        t += r.range(1.5, 5.0);
    }
    // Ráfagas y cañonazos muy lejanos
    let far_shot = {
        let mut rr = Rng::new(210);
        let mut f = Lp::new(500.0);
        gen(0.6, |t| f.run(rr.f() * 2.0 - 1.0) * (-t / 0.12).exp() * (t / 0.004).min(1.0))
    };
    let boom = {
        let mut rr = Rng::new(211);
        let mut f = Lp::new(90.0);
        gen(3.0, |t| f.run(rr.f() * 2.0 - 1.0) * 4.0 * (-t / 0.8).exp() * (t / 0.03).min(1.0))
    };
    let mut t = 3.0;
    while t < dur - 4.0 {
        if r.chance(0.35) {
            add(t, &boom, r.range(0.25, 0.45));
        } else {
            let shots = 3 + r.index(7);
            let gap = r.range(0.09, 0.2);
            let g = r.range(0.08, 0.18);
            for k in 0..shots {
                add(t + k as f32 * gap * r.range(0.8, 1.3), &far_shot, g);
            }
        }
        t += r.range(2.5, 7.0);
    }
    normalize(seamless(v, (2.0 * RATE as f32) as usize), 0.55)
}

/// Música tranquila para los menús: acordes suaves de cuerda y una melodía
/// lenta de flauta, en modo menor. 48 s en bucle.
pub fn menu_music() -> Vec<f32> {
    let bpm = 66.0;
    let beat = 60.0 / bpm;
    // La menor · Fa · Do · Sol, dos compases cada uno, dos veces con variación
    let chords: [[f32; 3]; 4] = [[57.0, 60.0, 64.0], [53.0, 57.0, 60.0], [48.0, 52.0, 55.0], [55.0, 59.0, 62.0]];
    let melody: [(f32, f32); 26] = [
        (76.0, 2.0), (74.0, 1.0), (72.0, 1.0), (71.0, 2.0), (72.0, 2.0),
        (69.0, 3.0), (67.0, 1.0), (69.0, 4.0),
        (72.0, 2.0), (71.0, 1.0), (69.0, 1.0), (67.0, 2.0), (64.0, 2.0),
        (67.0, 3.0), (69.0, 1.0), (71.0, 4.0),
        (76.0, 2.0), (77.0, 1.0), (76.0, 1.0), (74.0, 2.0), (72.0, 2.0),
        (71.0, 2.0), (72.0, 1.0), (74.0, 1.0), (69.0, 4.0), (0.0, 4.0),
    ];
    let midi = |m: f32| 440.0 * 2f32.powf((m - 69.0) / 12.0);
    let bars = 16.0;
    let dur = bars * 4.0 * beat;
    let n = (dur * RATE as f32) as usize;
    let mut v = vec![0.0f32; n];
    // Acordes: cuerdas con vibrato lento y ataque suave
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let bar = (t / (4.0 * beat)) as usize;
        let c = chords[(bar / 2) % 4];
        let local = t % (8.0 * beat);
        let env = (local / 1.5).min(1.0) * (1.0 - ((local - 8.0 * beat + 1.2) / 1.2).max(0.0)).max(0.0);
        let mut s = 0.0;
        for (k, &m) in c.iter().enumerate() {
            let f = midi(m - 12.0) * (1.0 + 0.003 * (t * 5.0 + k as f32).sin());
            s += (TAU * f * t).sin() * 0.5 + (TAU * f * 2.0 * t).sin() * 0.15 + (TAU * f * 3.0 * t).sin() * 0.05;
        }
        // Bajo
        let fb = midi(c[0] - 24.0);
        s += (TAU * fb * t).sin() * 0.5;
        v[i] = s * env * 0.18;
    }
    // Melodía de flauta (empieza en el compás 3)
    let mut t0 = 8.0 * beat;
    for &(m, len) in melody.iter() {
        let d = len * beat;
        if m > 0.0 {
            let f = midi(m);
            let mut r = Rng::new(m as u64);
            let mut breath = Lp::new(2500.0);
            for i in 0..(d * RATE as f32) as usize {
                let u = i as f32 / RATE as f32;
                let at = ((t0 + u) * RATE as f32) as usize;
                if at >= n {
                    break;
                }
                let env = (u / 0.12).min(1.0) * (1.0 - (u / d).powf(3.0)).max(0.0);
                let vib = 1.0 + 0.004 * (u * 5.5).sin() * (u / 0.4).min(1.0);
                let tone = (TAU * f * vib * u).sin() + 0.25 * (TAU * f * 2.0 * vib * u).sin();
                let air = breath.run(r.f() * 2.0 - 1.0) * 0.08;
                v[at] += (tone + air) * env * 0.22;
            }
        }
        t0 += d;
        if t0 >= dur {
            break;
        }
    }
    // Reverberación sencilla (ecos)
    for (delay, g) in [(0.13f32, 0.25f32), (0.29, 0.18), (0.47, 0.12)] {
        let d = (delay * RATE as f32) as usize;
        for i in (d..n).rev() {
            v[i] += v[i - d] * g;
        }
    }
    normalize(seamless(v, (1.5 * RATE as f32) as usize), 0.5)
}

/// Barrido del culatazo.
pub fn swing() -> Vec<f32> {
    let mut r = Rng::new(150);
    let mut lp = Lp::new(900.0);
    let mut hp = Hp::new(300.0);
    normalize(
        gen(0.28, |t| {
            let env = (-((t - 0.1) / 0.05).powi(2)).exp();
            hp.run(lp.run(r.f() * 2.0 - 1.0)) * env
        }),
        0.35,
    )
}

/// Golpe seco del culatazo al dar.
pub fn bash() -> Vec<f32> {
    let mut r = Rng::new(151);
    let mut lp = Lp::new(700.0);
    normalize(
        gen(0.3, |t| {
            let thud = lp.run(r.f() * 2.0 - 1.0) * 3.0 * (-t / 0.04).exp();
            let wood = (TAU * 260.0 * t).sin() * (-t / 0.03).exp();
            (thud + wood) * (t / 0.001).min(1.0)
        }),
        0.8,
    )
}

/// Siseo de la granada de humo.
pub fn smoke_hiss() -> Vec<f32> {
    let mut r = Rng::new(152);
    let mut hp = Hp::new(2500.0);
    let mut lp = Lp::new(7000.0);
    normalize(
        gen(3.5, |t| {
            let env = (t / 0.08).min(1.0) * (1.0 - (t / 3.5).powi(2)).max(0.0) * (0.8 + 0.2 * (t * 13.0).sin());
            lp.run(hp.run(r.f() * 2.0 - 1.0)) * env
        }),
        0.35,
    )
}

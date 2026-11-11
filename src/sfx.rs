//! Efectos de sonido sintetizados por código (sin archivos externos).
//! Cada función devuelve muestras mono en [-1, 1]; `to_wav` las empaqueta en WAV.

use crate::math::Rng;
use std::f32::consts::TAU;

pub const RATE: u32 = 22050;

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

fn gunshot(seed: u64, cutoff: f32, decay: f32, thump: f32, dur: f32, peak: f32) -> Vec<f32> {
    let mut r = Rng::new(seed);
    let mut lp = Lp::new(cutoff);
    let mut lp2 = Lp::new(cutoff * 0.2);
    normalize(
        gen(dur, |t| {
            let n = r.f() * 2.0 - 1.0;
            let body = lp.run(n) * (-t / decay).exp();
            let tail = lp2.run(n) * (-t / (decay * 3.0)).exp() * 1.8;
            let th = (TAU * (75.0 - 35.0 * t) * t).sin() * (-t / 0.07).exp() * thump;
            let attack = (t / 0.0015).min(1.0);
            (body + tail + th) * attack
        }),
        peak,
    )
}

pub fn rifle() -> Vec<f32> {
    gunshot(1, 5500.0, 0.05, 0.9, 0.75, 0.95)
}
pub fn smg() -> Vec<f32> {
    gunshot(2, 4500.0, 0.03, 0.6, 0.32, 0.8)
}
pub fn enemy_rifle() -> Vec<f32> {
    gunshot(3, 1700.0, 0.08, 0.5, 0.9, 0.85)
}
pub fn enemy_smg() -> Vec<f32> {
    gunshot(4, 2000.0, 0.035, 0.4, 0.35, 0.7)
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

pub fn kar98k() -> Vec<f32> {
    gunshot(13, 4800.0, 0.06, 1.0, 0.9, 1.0)
}
pub fn mp40() -> Vec<f32> {
    gunshot(14, 3900.0, 0.03, 0.55, 0.3, 0.78)
}
/// Cerrojo del Kar98k tras el disparo.
pub fn bolt() -> Vec<f32> {
    clicks(15, &[0.0, 0.25, 0.45], 0.6, 0.35)
}

pub fn pistol() -> Vec<f32> {
    gunshot(11, 6500.0, 0.025, 0.4, 0.4, 0.75)
}
/// Ametralladora ligera: más grave y con más cola que un subfusil.
pub fn lmg() -> Vec<f32> {
    gunshot(12, 3600.0, 0.045, 1.0, 0.5, 0.9)
}
/// Fusil de francotirador: un estampido largo que rebota en el valle.
pub fn sniper() -> Vec<f32> {
    let mut v = gunshot(13, 6000.0, 0.06, 1.1, 1.4, 0.95);
    let echo = (0.32 * RATE as f32) as usize;
    for i in (echo..v.len()).rev() {
        v[i] += v[i - echo] * 0.3;
    }
    normalize(v, 0.95)
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

//! Simulación compartida entre servidor y cliente.
//! El cliente ejecuta exactamente el mismo `step` para predecir su propio
//! movimiento; el servidor lo ejecuta de forma autoritativa.

use crate::math::*;
use crate::world::*;
use std::f32::consts::PI;

pub const TICK_HZ: u32 = 60;
pub const DT: f32 = 1.0 / TICK_HZ as f32;
pub const GRAVITY: f32 = 16.0;
pub const PLAYER_R: f32 = 0.33;
pub const STAND_H: f32 = 1.8;
pub const CROUCH_H: f32 = 1.2;
pub const STAND_EYE: f32 = 1.65;
pub const CROUCH_EYE: f32 = 1.05;
pub const MAX_HEALTH: f32 = 100.0;
pub const START_GRENADES: u8 = 2;

pub const ALLIES: u8 = 0;
pub const AXIS: u8 = 1;

pub fn team_name(t: u8) -> &'static str {
    if t == ALLIES {
        "Aliados"
    } else {
        "Eje"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GunKind {
    Rifle,
    Smg,
}

pub struct GunDef {
    pub name: &'static str,
    pub kind: GunKind,
    pub mag: u32,
    pub start_reserve: u32,
    pub interval: f32,
    pub auto: bool,
    pub damage: f32,
    pub head_mult: f32,
    pub spread_hip: f32,
    pub spread_ads: f32,
    pub spread_move: f32,
    pub bloom: f32,
    pub kick: f32,
    pub reload: f32,
    pub reload_empty: f32,
    /// Multiplicador de daño más allá de 35 m.
    pub falloff: f32,
    pub ads_fov: f32,
}

/// Índice = bando * 2 + ranura (0 fusil, 1 subfusil).
pub const GUNS: [GunDef; 4] = [
    GunDef {
        name: "M1 Garand",
        kind: GunKind::Rifle,
        mag: 8,
        start_reserve: 48,
        interval: 0.2,
        auto: false,
        damage: 105.0,
        head_mult: 2.0,
        spread_hip: 0.045,
        spread_ads: 0.0015,
        spread_move: 0.03,
        bloom: 0.0,
        kick: 0.045,
        reload: 2.3,
        reload_empty: 2.0,
        falloff: 1.0,
        ads_fov: 45.0,
    },
    GunDef {
        name: "Thompson",
        kind: GunKind::Smg,
        mag: 30,
        start_reserve: 120,
        interval: 0.086,
        auto: true,
        damage: 34.0,
        head_mult: 2.2,
        spread_hip: 0.04,
        spread_ads: 0.012,
        spread_move: 0.025,
        bloom: 0.004,
        kick: 0.012,
        reload: 2.6,
        reload_empty: 3.0,
        falloff: 0.7,
        ads_fov: 55.0,
    },
    GunDef {
        name: "Kar98k",
        kind: GunKind::Rifle,
        mag: 5,
        start_reserve: 30,
        interval: 1.15,
        auto: false,
        damage: 125.0,
        head_mult: 2.0,
        spread_hip: 0.06,
        spread_ads: 0.0008,
        spread_move: 0.035,
        bloom: 0.0,
        kick: 0.07,
        reload: 2.4,
        reload_empty: 2.8,
        falloff: 1.0,
        ads_fov: 40.0,
    },
    GunDef {
        name: "MP40",
        kind: GunKind::Smg,
        mag: 32,
        start_reserve: 128,
        interval: 0.105,
        auto: true,
        damage: 31.0,
        head_mult: 2.2,
        spread_hip: 0.037,
        spread_ads: 0.011,
        spread_move: 0.024,
        bloom: 0.0035,
        kick: 0.011,
        reload: 2.5,
        reload_empty: 2.9,
        falloff: 0.7,
        ads_fov: 55.0,
    },
];

pub fn gun_index(side: u8, slot: u8) -> usize {
    (side.min(1) as usize) * 2 + slot.min(1) as usize
}

pub fn gun_def(side: u8, slot: u8) -> &'static GunDef {
    &GUNS[gun_index(side, slot)]
}

/// Botones de un comando. Los marcados "flanco" solo están activos en el
/// comando en que se pulsaron.
pub mod btn {
    pub const FIRE: u16 = 1;
    pub const FIRE_PRESSED: u16 = 2; // flanco
    pub const ADS: u16 = 4;
    pub const JUMP: u16 = 8; // flanco
    pub const CROUCH: u16 = 16;
    pub const SPRINT: u16 = 32;
    pub const RELOAD: u16 = 64; // flanco
    pub const GRENADE: u16 = 128; // flanco
    pub const USE: u16 = 256;
}

/// Un paso de entrada del jugador (uno por tick de 1/60 s).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cmd {
    pub seq: u32,
    /// Tick del servidor que el cliente estaba viendo (para compensar el lag).
    pub view_tick: u32,
    pub yaw: f32,
    pub pitch: f32,
    pub fwd: i8,
    pub strafe: i8,
    pub buttons: u16,
    pub slot: u8,
}

impl Cmd {
    pub fn has(&self, b: u16) -> bool {
        self.buttons & b != 0
    }
    pub fn fwd_f(&self) -> f32 {
        self.fwd as f32 / 127.0
    }
    pub fn strafe_f(&self) -> f32 {
        self.strafe as f32 / 127.0
    }
    pub fn axis(v: f32) -> i8 {
        (v.clamp(-1.0, 1.0) * 127.0).round() as i8
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ammo {
    pub mag: u32,
    pub reserve: u32,
}

/// Estado de un jugador que se simula (y se predice en su propio cliente).
#[derive(Clone, Debug, PartialEq)]
pub struct PState {
    pub pos: V3,
    pub vel: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub side: u8,
    pub grounded: bool,
    pub crouching: bool,
    pub crouch: f32,
    pub sprinting: bool,
    pub stamina: f32,
    pub ammo: [Ammo; 2],
    pub slot: u8,
    pub cooldown: f32,
    pub reload_t: f32,
    pub reload_total: f32,
    pub switch_t: f32,
    pub throw_t: f32,
    pub grenades: u8,
    pub ads: f32,
    pub bloom: f32,
    pub moving: f32,
    pub bob: f32,
    /// Ocupado colocando/desactivando la bomba: no se puede mover ni disparar.
    pub busy: bool,
}

impl PState {
    pub fn new(pos: V3, yaw: f32, side: u8) -> PState {
        let a = |slot: u8| {
            let d = gun_def(side, slot);
            Ammo { mag: d.mag, reserve: d.start_reserve }
        };
        PState {
            pos,
            vel: V3::ZERO,
            yaw,
            pitch: 0.0,
            side,
            grounded: true,
            crouching: false,
            crouch: 0.0,
            sprinting: false,
            stamina: 4.0,
            ammo: [a(0), a(1)],
            slot: 0,
            cooldown: 0.0,
            reload_t: 0.0,
            reload_total: 1.0,
            switch_t: 0.0,
            throw_t: 0.0,
            grenades: START_GRENADES,
            ads: 0.0,
            bloom: 0.0,
            moving: 0.0,
            bob: 0.0,
            busy: false,
        }
    }
    pub fn eye(&self) -> V3 {
        self.pos + V3::UP * lerp(STAND_EYE, CROUCH_EYE, self.crouch)
    }
    pub fn height(&self) -> f32 {
        lerp(STAND_H, CROUCH_H, self.crouch)
    }
    pub fn forward(&self) -> V3 {
        V3::from_yaw_pitch(self.yaw, self.pitch)
    }
    pub fn right(&self) -> V3 {
        V3::right_of_yaw(self.yaw)
    }
    pub fn gun(&self) -> &'static GunDef {
        gun_def(self.side, self.slot)
    }
    pub fn spread(&self) -> f32 {
        let d = self.gun();
        let mut s = lerp(d.spread_hip, d.spread_ads, self.ads)
            + self.moving * d.spread_move * (1.0 - 0.7 * self.ads)
            + self.bloom;
        if !self.grounded {
            s += 0.06;
        }
        if self.crouching {
            s *= 0.75;
        }
        s
    }
    pub fn lowered(&self) -> f32 {
        let mut l = 0.0f32;
        if self.reload_t > 0.0 {
            let p = 1.0 - self.reload_t / self.reload_total;
            l = l.max((p * PI).sin());
        }
        if self.switch_t > 0.0 {
            l = l.max((self.switch_t / 0.5 * PI).sin().abs());
        }
        if self.throw_t > 0.0 {
            l = l.max((self.throw_t / 0.8 * PI).sin());
        }
        if self.busy {
            l = 1.0;
        }
        l
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Outcome {
    /// (origen, dirección con dispersión aplicada)
    pub shot: Option<(V3, V3)>,
    pub throw: Option<(V3, V3)>,
    pub reload: bool,
    pub ping: bool,
    pub dry: bool,
    pub switched: bool,
    /// Retroceso vertical a aplicar a la vista (radianes).
    pub kick: f32,
}

/// Avanza un jugador un tick con un comando. Determinista salvo la dispersión.
pub fn step(world: &World, p: &mut PState, cmd: &Cmd, rng: &mut Rng) -> Outcome {
    let dt = DT;
    let mut out = Outcome::default();
    p.yaw = wrap_angle(cmd.yaw);
    p.pitch = cmd.pitch.clamp(-1.45, 1.45);
    p.bloom = (p.bloom - dt * 0.08).max(0.0);
    p.cooldown -= dt;
    p.switch_t = (p.switch_t - dt).max(0.0);
    p.throw_t = (p.throw_t - dt).max(0.0);

    let fwd_in = if p.busy { 0.0 } else { cmd.fwd_f() };
    let strafe_in = if p.busy { 0.0 } else { cmd.strafe_f() };

    // Agacharse
    let can_stand = world.body_free(p.pos, PLAYER_R, STAND_H);
    if cmd.has(btn::CROUCH) {
        p.crouching = true;
    } else if p.crouching && can_stand {
        p.crouching = false;
    }
    p.crouch = approach(p.crouch, if p.crouching { 1.0 } else { 0.0 }, dt * 7.0);

    // Correr
    p.sprinting = cmd.has(btn::SPRINT)
        && fwd_in > 0.5
        && !p.crouching
        && p.stamina > 0.05
        && p.reload_t <= 0.0
        && !cmd.has(btn::ADS)
        && !cmd.has(btn::FIRE);
    if p.sprinting {
        p.stamina = (p.stamina - dt).max(0.0);
    } else {
        p.stamina = (p.stamina + dt * 0.6).min(4.0);
    }
    let ads_target = if cmd.has(btn::ADS) && p.reload_t <= 0.0 && p.switch_t <= 0.0 && !p.sprinting && !p.busy {
        1.0
    } else {
        0.0
    };
    p.ads = approach(p.ads, ads_target, dt * 6.0);

    // Movimiento
    let fwd = V3::from_yaw(p.yaw);
    let right = p.right();
    let mut wish = fwd * fwd_in + right * strafe_in;
    if wish.len() > 1.0 {
        wish = wish.norm();
    }
    let speed = if p.sprinting {
        6.8
    } else if p.crouching {
        2.4
    } else {
        4.4
    } * (1.0 - 0.45 * p.ads);
    let target = wish * speed;
    let k = ((if p.grounded { 14.0 } else { 2.5 }) * dt).min(1.0);
    p.vel.x += (target.x - p.vel.x) * k;
    p.vel.z += (target.z - p.vel.z) * k;
    if cmd.has(btn::JUMP) && p.grounded && !p.busy {
        if p.crouching {
            if can_stand {
                p.crouching = false;
            }
        } else {
            p.vel.y = 5.6;
            p.grounded = false;
        }
    }
    p.vel.y -= GRAVITY * dt;
    let delta = p.vel * dt;
    let (np, grounded, _) = world.move_body(p.pos, PLAYER_R, p.height(), delta);
    if delta.y > 0.0 && np.y - p.pos.y < delta.y * 0.5 {
        p.vel.y = 0.0;
    }
    let actual = (np - p.pos) / dt;
    p.vel.x = actual.x;
    p.vel.z = actual.z;
    p.grounded = grounded;
    if grounded && p.vel.y < 0.0 {
        p.vel.y = 0.0;
    }
    p.pos = np;
    let hs = p.vel.flat().len();
    p.moving = (hs / 4.4).min(1.0);
    if p.grounded {
        p.bob += hs * dt * 1.8;
    }

    // Cambio de arma
    let want_slot = cmd.slot.min(1);
    if want_slot != p.slot && p.throw_t <= 0.0 && !p.busy {
        p.slot = want_slot;
        p.switch_t = 0.5;
        p.reload_t = 0.0;
        p.cooldown = p.cooldown.max(0.25);
        out.switched = true;
    }

    // Recarga en curso
    if p.reload_t > 0.0 {
        p.reload_t -= dt;
        if p.reload_t <= 0.0 {
            p.reload_t = 0.0;
            let def = p.gun();
            let a = &mut p.ammo[p.slot as usize];
            let take = (def.mag - a.mag).min(a.reserve);
            a.mag += take;
            a.reserve -= take;
        }
    }

    let def = p.gun();
    let ready = p.reload_t <= 0.0 && p.switch_t <= 0.0 && p.throw_t <= 0.0 && !p.busy;
    let ammo = p.ammo[p.slot as usize];
    if ready
        && ammo.reserve > 0
        && ammo.mag < def.mag
        && (cmd.has(btn::RELOAD) || (ammo.mag == 0 && p.cooldown <= 0.0))
    {
        p.reload_total = if ammo.mag == 0 { def.reload_empty } else { def.reload };
        p.reload_t = p.reload_total;
        out.reload = true;
        return out;
    }

    let trigger = if def.auto { cmd.has(btn::FIRE) } else { cmd.has(btn::FIRE_PRESSED) };
    if trigger && ready && p.cooldown <= 0.0 {
        if ammo.mag > 0 {
            p.sprinting = false;
            p.ammo[p.slot as usize].mag -= 1;
            p.cooldown = def.interval;
            let spread = p.spread();
            p.bloom = (p.bloom + def.bloom).min(0.035);
            let f = p.forward();
            let r = p.right();
            let up = r.cross(f).norm();
            let a = rng.range(0.0, 2.0 * PI);
            let rr = spread * rng.f().sqrt();
            let dir = (f + r * (rr * a.cos()) + up * (rr * a.sin())).norm();
            out.shot = Some((p.eye(), dir));
            out.kick = def.kick * (1.0 - 0.4 * p.ads) * if p.crouching { 0.8 } else { 1.0 };
            if def.kind == GunKind::Rifle && def.mag == 8 && p.ammo[p.slot as usize].mag == 0 {
                out.ping = true;
            }
        } else if cmd.has(btn::FIRE_PRESSED) {
            out.dry = true;
        }
    }

    if cmd.has(btn::GRENADE) && p.grenades > 0 && ready {
        p.grenades -= 1;
        p.throw_t = 0.8;
        let f = p.forward();
        let origin = p.eye() + f * 0.4 + p.right() * 0.2;
        let vel = f * 15.0 + V3::UP * 2.5 + p.vel.flat() * 0.5;
        out.throw = Some((origin, vel));
    }
    out
}

/// (cuerpo, cabeza) de un jugador en una posición dada.
pub fn hitboxes(pos: V3, crouch: f32) -> (Aabb, Aabb) {
    let body_h = lerp(1.45, 0.95, crouch);
    let head_y = lerp(1.62, 1.12, crouch);
    (
        Aabb::cs(pos + V3::UP * (body_h * 0.5), v3(0.6, body_h, 0.6)),
        Aabb::cs(pos + V3::UP * head_y, v3(0.32, 0.34, 0.32)),
    )
}

pub struct Grenade {
    pub id: u16,
    pub pos: V3,
    pub vel: V3,
    pub fuse: f32,
    pub owner: u8,
    pub side: u8,
    pub bounce_cd: f32,
}

/// Física de una granada. Devuelve true si ha rebotado con fuerza.
pub fn grenade_step(world: &World, g: &mut Grenade, dt: f32) -> bool {
    let mut bounced = false;
    g.fuse -= dt;
    g.bounce_cd -= dt;
    g.vel.y -= GRAVITY * dt;
    for axis in 0..3 {
        let mut np = g.pos;
        np.set(axis, np.get(axis) + g.vel.get(axis) * dt);
        let bb = Aabb::cs(np, v3(0.16, 0.16, 0.16));
        let hit = (axis == 1 && np.y < 0.08) || world.blocks.iter().any(|b| b.b.overlaps(&bb));
        if hit {
            let speed = g.vel.len();
            g.vel.set(axis, -g.vel.get(axis) * 0.35);
            for o in 0..3 {
                if o != axis {
                    g.vel.set(o, g.vel.get(o) * 0.7);
                }
            }
            if speed > 2.5 && g.bounce_cd <= 0.0 {
                g.bounce_cd = 0.15;
                bounced = true;
            }
        } else {
            g.pos = np;
        }
    }
    if g.pos.y < 0.08 {
        g.pos.y = 0.08;
    }
    bounced
}

//! Protocolo de red sobre UDP: serialización binaria y tipos de mensaje.

use crate::math::*;
use crate::sim::{Ammo, Cmd, PState};

pub const PROTOCOL: u16 = 1;
pub const DEFAULT_PORT: u16 = 28960;
pub const MAGIC: [u8; 2] = *b"ED";
/// Una instantánea cada 3 ticks = 20 por segundo.
pub const SNAP_EVERY: u32 = 3;
pub const MAX_PACKET: usize = 1400;
pub const MAX_PLAYERS: usize = 16;
pub const TIMEOUT_SECS: f32 = 10.0;

// ----------------------------------------------------------------------
// Escritura / lectura
// ----------------------------------------------------------------------

#[derive(Default)]
pub struct W(pub Vec<u8>);

impl W {
    pub fn new(kind: u8) -> W {
        let mut w = W(Vec::with_capacity(256));
        w.0.extend_from_slice(&MAGIC);
        w.u8(kind);
        w
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.0.push(v as u8);
    }
    pub fn i8(&mut self, v: i8) {
        self.0.push(v as u8);
    }
    pub fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn v3(&mut self, v: V3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    /// Ángulo en [-PI, PI] cuantizado a 16 bits.
    pub fn angle(&mut self, a: f32) {
        self.i16((wrap_angle(a) / std::f32::consts::PI * 32767.0) as i16);
    }
    pub fn str(&mut self, s: &str) {
        let b = s.as_bytes();
        let mut n = b.len().min(120);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.u8(n as u8);
        self.0.extend_from_slice(&b[..n]);
    }
}

pub struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    /// Valida la cabecera y devuelve (tipo, lector).
    pub fn open(b: &'a [u8]) -> Option<(u8, R<'a>)> {
        if b.len() < 3 || b[0..2] != MAGIC {
            return None;
        }
        Some((b[2], R { b, p: 3 }))
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.p + n > self.b.len() {
            return None;
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    pub fn bool(&mut self) -> Option<bool> {
        self.u8().map(|v| v != 0)
    }
    pub fn i8(&mut self) -> Option<i8> {
        self.u8().map(|v| v as i8)
    }
    pub fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]))
    }
    pub fn i16(&mut self) -> Option<i16> {
        self.take(2).map(|s| i16::from_le_bytes([s[0], s[1]]))
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    pub fn f32(&mut self) -> Option<f32> {
        let v = self.take(4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))?;
        if v.is_finite() {
            Some(v)
        } else {
            None
        }
    }
    pub fn v3(&mut self) -> Option<V3> {
        Some(v3(self.f32()?, self.f32()?, self.f32()?))
    }
    pub fn angle(&mut self) -> Option<f32> {
        self.i16().map(|v| v as f32 / 32767.0 * std::f32::consts::PI)
    }
    pub fn str(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        let s = self.take(n)?;
        Some(String::from_utf8_lossy(s).into_owned())
    }
}

/// Limpia un nombre de jugador o servidor.
pub fn clean_name(s: &str, max: usize) -> String {
    let t: String = s
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string();
    if t.is_empty() {
        "Soldado".to_string()
    } else {
        t
    }
}

// ----------------------------------------------------------------------
// Tipos de mensaje
// ----------------------------------------------------------------------

pub mod c2s {
    pub const CONNECT: u8 = 1;
    pub const CMDS: u8 = 2;
    pub const DISCONNECT: u8 = 3;
    pub const DISCOVER: u8 = 4;
}

pub mod s2c {
    pub const WELCOME: u8 = 1;
    pub const REJECT: u8 = 2;
    pub const SNAPSHOT: u8 = 3;
    pub const INFO: u8 = 4;
    pub const KICKED: u8 = 5;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dm,
    Tdm,
    Sd,
    Ctf,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Dm, Mode::Tdm, Mode::Sd, Mode::Ctf];
    pub fn id(self) -> u8 {
        match self {
            Mode::Dm => 0,
            Mode::Tdm => 1,
            Mode::Sd => 2,
            Mode::Ctf => 3,
        }
    }
    pub fn from_id(v: u8) -> Mode {
        match v {
            1 => Mode::Tdm,
            2 => Mode::Sd,
            3 => Mode::Ctf,
            _ => Mode::Dm,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::Dm => "Todos contra todos",
            Mode::Tdm => "Por equipos",
            Mode::Sd => "Buscar y destruir",
            Mode::Ctf => "Captura la bandera",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Mode::Dm => "dm",
            Mode::Tdm => "tdm",
            Mode::Sd => "sd",
            Mode::Ctf => "ctf",
        }
    }
    pub fn parse(s: &str) -> Option<Mode> {
        Mode::ALL.iter().copied().find(|m| m.short().eq_ignore_ascii_case(s.trim()))
    }
    pub fn teams(self) -> bool {
        self != Mode::Dm
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Esperando jugadores: se juega libre, sin puntuar.
    Warmup,
    Playing,
    /// Fin de ronda (Buscar y destruir).
    RoundEnd,
    /// Fin de partida: marcador final antes del siguiente modo.
    Intermission,
}

impl Phase {
    pub fn id(self) -> u8 {
        match self {
            Phase::Warmup => 0,
            Phase::Playing => 1,
            Phase::RoundEnd => 2,
            Phase::Intermission => 3,
        }
    }
    pub fn from_id(v: u8) -> Phase {
        match v {
            1 => Phase::Playing,
            2 => Phase::RoundEnd,
            3 => Phase::Intermission,
            _ => Phase::Warmup,
        }
    }
}

pub mod weapon {
    // 0..=3: índice de GUNS
    pub const GRENADE: u8 = 4;
    pub const BOMB: u8 = 5;
    pub const SUICIDE: u8 = 6;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    Shot { shooter: u8, from: V3, to: V3, gun: u8 },
    Hit { shooter: u8 },
    Hurt { victim: u8, from: V3 },
    Kill { killer: u8, victim: u8, weapon: u8, head: bool, killer_name: String, victim_name: String },
    Explosion { pos: V3, big: bool },
    Bounce { pos: V3 },
    Throw { who: u8, pos: V3 },
    Reload { who: u8, pos: V3 },
    Ping { who: u8, pos: V3 },
    Chat { from: u8, team_only: bool, name: String, text: String },
    Announce { text: String, tone: u8 },
    Whistle,
}

pub mod tone {
    pub const NEUTRAL: u8 = 0;
    pub const ALLIES: u8 = 1;
    pub const AXIS: u8 = 2;
    pub const ALERT: u8 = 3;
}

impl Ev {
    pub fn write(&self, w: &mut W) {
        match self {
            Ev::Shot { shooter, from, to, gun } => {
                w.u8(0);
                w.u8(*shooter);
                w.v3(*from);
                w.v3(*to);
                w.u8(*gun);
            }
            Ev::Hit { shooter } => {
                w.u8(1);
                w.u8(*shooter);
            }
            Ev::Hurt { victim, from } => {
                w.u8(2);
                w.u8(*victim);
                w.v3(*from);
            }
            Ev::Kill { killer, victim, weapon, head, killer_name, victim_name } => {
                w.u8(3);
                w.u8(*killer);
                w.u8(*victim);
                w.u8(*weapon);
                w.bool(*head);
                w.str(killer_name);
                w.str(victim_name);
            }
            Ev::Explosion { pos, big } => {
                w.u8(4);
                w.v3(*pos);
                w.bool(*big);
            }
            Ev::Bounce { pos } => {
                w.u8(5);
                w.v3(*pos);
            }
            Ev::Throw { who, pos } => {
                w.u8(6);
                w.u8(*who);
                w.v3(*pos);
            }
            Ev::Reload { who, pos } => {
                w.u8(7);
                w.u8(*who);
                w.v3(*pos);
            }
            Ev::Ping { who, pos } => {
                w.u8(8);
                w.u8(*who);
                w.v3(*pos);
            }
            Ev::Chat { from, team_only, name, text } => {
                w.u8(9);
                w.u8(*from);
                w.bool(*team_only);
                w.str(name);
                w.str(text);
            }
            Ev::Announce { text, tone } => {
                w.u8(10);
                w.str(text);
                w.u8(*tone);
            }
            Ev::Whistle => w.u8(11),
        }
    }
    pub fn read(r: &mut R) -> Option<Ev> {
        Some(match r.u8()? {
            0 => Ev::Shot { shooter: r.u8()?, from: r.v3()?, to: r.v3()?, gun: r.u8()? },
            1 => Ev::Hit { shooter: r.u8()? },
            2 => Ev::Hurt { victim: r.u8()?, from: r.v3()? },
            3 => Ev::Kill {
                killer: r.u8()?,
                victim: r.u8()?,
                weapon: r.u8()?,
                head: r.bool()?,
                killer_name: r.str()?,
                victim_name: r.str()?,
            },
            4 => Ev::Explosion { pos: r.v3()?, big: r.bool()? },
            5 => Ev::Bounce { pos: r.v3()? },
            6 => Ev::Throw { who: r.u8()?, pos: r.v3()? },
            7 => Ev::Reload { who: r.u8()?, pos: r.v3()? },
            8 => Ev::Ping { who: r.u8()?, pos: r.v3()? },
            9 => Ev::Chat { from: r.u8()?, team_only: r.bool()?, name: r.str()?, text: r.str()? },
            10 => Ev::Announce { text: r.str()?, tone: r.u8()? },
            11 => Ev::Whistle,
            _ => return None,
        })
    }
}

pub fn write_cmd(w: &mut W, c: &Cmd) {
    w.u32(c.seq);
    w.u32(c.view_tick);
    w.f32(c.yaw);
    w.f32(c.pitch);
    w.i8(c.fwd);
    w.i8(c.strafe);
    w.u16(c.buttons);
    w.u8(c.slot);
}

pub fn read_cmd(r: &mut R) -> Option<Cmd> {
    Some(Cmd {
        seq: r.u32()?,
        view_tick: r.u32()?,
        yaw: r.f32()?,
        pitch: r.f32()?,
        fwd: r.i8()?,
        strafe: r.i8()?,
        buttons: r.u16()?,
        slot: r.u8()?,
    })
}

/// Estado completo del propio jugador (para reconciliar la predicción).
pub fn write_pstate(w: &mut W, p: &PState) {
    w.v3(p.pos);
    w.v3(p.vel);
    w.f32(p.yaw);
    w.f32(p.pitch);
    w.u8(p.side);
    let flags = p.grounded as u8 | (p.crouching as u8) << 1 | (p.sprinting as u8) << 2 | (p.busy as u8) << 3;
    w.u8(flags);
    w.f32(p.crouch);
    w.f32(p.stamina);
    for a in &p.ammo {
        w.u16(a.mag as u16);
        w.u16(a.reserve as u16);
    }
    w.u8(p.slot);
    w.f32(p.cooldown);
    w.f32(p.reload_t);
    w.f32(p.reload_total);
    w.f32(p.switch_t);
    w.f32(p.throw_t);
    w.u8(p.grenades);
    w.f32(p.ads);
    w.f32(p.bloom);
    w.f32(p.moving);
    w.f32(p.bob);
}

pub fn read_pstate(r: &mut R) -> Option<PState> {
    let pos = r.v3()?;
    let vel = r.v3()?;
    let yaw = r.f32()?;
    let pitch = r.f32()?;
    let side = r.u8()?;
    let flags = r.u8()?;
    let crouch = r.f32()?;
    let stamina = r.f32()?;
    let mut ammo = [Ammo::default(); 2];
    for a in ammo.iter_mut() {
        a.mag = r.u16()? as u32;
        a.reserve = r.u16()? as u32;
    }
    Some(PState {
        pos,
        vel,
        yaw,
        pitch,
        side,
        grounded: flags & 1 != 0,
        crouching: flags & 2 != 0,
        sprinting: flags & 4 != 0,
        busy: flags & 8 != 0,
        crouch,
        stamina,
        ammo,
        slot: r.u8()?,
        cooldown: r.f32()?,
        reload_t: r.f32()?,
        reload_total: r.f32()?,
        switch_t: r.f32()?,
        throw_t: r.f32()?,
        grenades: r.u8()?,
        ads: r.f32()?,
        bloom: r.f32()?,
        moving: r.f32()?,
        bob: r.f32()?,
    })
}

/// Un jugador tal y como lo ven los demás.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub id: u8,
    pub name: String,
    pub team: u8,
    pub side: u8,
    pub alive: bool,
    pub crouch: f32,
    pub ads: bool,
    pub sprint: bool,
    pub has_flag: bool,
    pub slot: u8,
    pub pos: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub moving: f32,
    pub kills: u16,
    pub deaths: u16,
    pub score: i16,
    pub ping: u16,
}

impl PlayerInfo {
    pub fn write(&self, w: &mut W) {
        w.u8(self.id);
        w.str(&self.name);
        w.u8(self.team);
        w.u8(self.side);
        let f = self.alive as u8 | (self.ads as u8) << 1 | (self.sprint as u8) << 2 | (self.has_flag as u8) << 3;
        w.u8(f);
        w.u8((self.crouch.clamp(0.0, 1.0) * 255.0) as u8);
        w.u8(self.slot);
        w.v3(self.pos);
        w.angle(self.yaw);
        w.angle(self.pitch);
        w.u8((self.moving.clamp(0.0, 1.0) * 255.0) as u8);
        w.u16(self.kills);
        w.u16(self.deaths);
        w.i16(self.score);
        w.u16(self.ping);
    }
    pub fn read(r: &mut R) -> Option<PlayerInfo> {
        let id = r.u8()?;
        let name = r.str()?;
        let team = r.u8()?;
        let side = r.u8()?;
        let f = r.u8()?;
        Some(PlayerInfo {
            id,
            name,
            team,
            side,
            alive: f & 1 != 0,
            ads: f & 2 != 0,
            sprint: f & 4 != 0,
            has_flag: f & 8 != 0,
            crouch: r.u8()? as f32 / 255.0,
            slot: r.u8()?,
            pos: r.v3()?,
            yaw: r.angle()?,
            pitch: r.angle()?,
            moving: r.u8()? as f32 / 255.0,
            kills: r.u16()?,
            deaths: r.u16()?,
            score: r.i16()?,
            ping: r.u16()?,
        })
    }
}

pub mod flag_state {
    pub const HOME: u8 = 0;
    pub const CARRIED: u8 = 1;
    pub const DROPPED: u8 = 2;
}

pub mod bomb_state {
    pub const NONE: u8 = 0;
    pub const PLANTED: u8 = 1;
    pub const DEFUSED: u8 = 2;
    pub const EXPLODED: u8 = 3;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlagInfo {
    pub state: u8,
    pub carrier: u8,
    pub pos: V3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrenadeInfo {
    pub id: u16,
    pub pos: V3,
    pub side: u8,
}

/// Instantánea del servidor tal y como la recibe un cliente concreto.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub tick: u32,
    pub ack: u32,
    pub mode: Mode,
    pub phase: Phase,
    pub timer: f32,
    pub score_limit: u16,
    pub team_scores: [u16; 2],
    pub round: u8,
    pub attackers: u8,
    pub flags: [FlagInfo; 2],
    pub bomb_state: u8,
    pub bomb_site: u8,
    pub bomb_timer: f32,
    /// Progreso propio colocando o desactivando la bomba (0..1).
    pub progress: f32,
    pub my_team: u8,
    pub my_alive: bool,
    pub respawn_in: f32,
    pub health: f32,
    pub me: Option<PState>,
    pub players: Vec<PlayerInfo>,
    pub grenades: Vec<GrenadeInfo>,
    pub events: Vec<(u32, Ev)>,
}

/// Información pública que un servidor anuncia en la red local.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerInfo {
    pub name: String,
    pub mode: Mode,
    pub players: u8,
    pub max: u8,
    pub port: u16,
}

impl Snapshot {
    /// Escribe la instantánea. Los eventos se añaden mientras quepan en el paquete.
    pub fn write(&self, w: &mut W) {
        w.u32(self.tick);
        w.u32(self.ack);
        w.u8(self.mode.id());
        w.u8(self.phase.id());
        w.f32(self.timer);
        w.u16(self.score_limit);
        w.u16(self.team_scores[0]);
        w.u16(self.team_scores[1]);
        w.u8(self.round);
        w.u8(self.attackers);
        for f in &self.flags {
            w.u8(f.state);
            w.u8(f.carrier);
            w.v3(f.pos);
        }
        w.u8(self.bomb_state);
        w.u8(self.bomb_site);
        w.f32(self.bomb_timer);
        w.f32(self.progress);
        w.u8(self.my_team);
        w.bool(self.my_alive);
        w.f32(self.respawn_in);
        w.f32(self.health);
        match &self.me {
            Some(p) => {
                w.u8(1);
                write_pstate(w, p);
            }
            None => w.u8(0),
        }
        w.u8(self.players.len() as u8);
        for p in &self.players {
            p.write(w);
        }
        w.u8(self.grenades.len() as u8);
        for g in &self.grenades {
            w.u16(g.id);
            w.v3(g.pos);
            w.u8(g.side);
        }
        // Eventos: contador al final, se rellena según el espacio
        let count_at = w.len();
        w.u8(0);
        let mut n = 0u8;
        for (id, e) in &self.events {
            let mut tmp = W(Vec::with_capacity(64));
            tmp.u32(*id);
            e.write(&mut tmp);
            if w.len() + tmp.len() > MAX_PACKET || n == 255 {
                break;
            }
            w.0.extend_from_slice(&tmp.0);
            n += 1;
        }
        w.0[count_at] = n;
    }

    pub fn read(r: &mut R) -> Option<Snapshot> {
        let tick = r.u32()?;
        let ack = r.u32()?;
        let mode = Mode::from_id(r.u8()?);
        let phase = Phase::from_id(r.u8()?);
        let timer = r.f32()?;
        let score_limit = r.u16()?;
        let team_scores = [r.u16()?, r.u16()?];
        let round = r.u8()?;
        let attackers = r.u8()?;
        let mut flags = [FlagInfo { state: 0, carrier: 0, pos: V3::ZERO }; 2];
        for f in flags.iter_mut() {
            f.state = r.u8()?;
            f.carrier = r.u8()?;
            f.pos = r.v3()?;
        }
        let bomb_state = r.u8()?;
        let bomb_site = r.u8()?;
        let bomb_timer = r.f32()?;
        let progress = r.f32()?;
        let my_team = r.u8()?;
        let my_alive = r.bool()?;
        let respawn_in = r.f32()?;
        let health = r.f32()?;
        let me = if r.u8()? == 1 { Some(read_pstate(r)?) } else { None };
        let np = r.u8()? as usize;
        let mut players = Vec::with_capacity(np);
        for _ in 0..np {
            players.push(PlayerInfo::read(r)?);
        }
        let ng = r.u8()? as usize;
        let mut grenades = Vec::with_capacity(ng);
        for _ in 0..ng {
            grenades.push(GrenadeInfo { id: r.u16()?, pos: r.v3()?, side: r.u8()? });
        }
        let ne = r.u8()? as usize;
        let mut events = Vec::with_capacity(ne);
        for _ in 0..ne {
            let id = r.u32()?;
            events.push((id, Ev::read(r)?));
        }
        Some(Snapshot {
            tick,
            ack,
            mode,
            phase,
            timer,
            score_limit,
            team_scores,
            round,
            attackers,
            flags,
            bomb_state,
            bomb_site,
            bomb_timer,
            progress,
            my_team,
            my_alive,
            respawn_in,
            health,
            me,
            players,
            grenades,
            events,
        })
    }
}

impl ServerInfo {
    pub fn write(&self, w: &mut W) {
        w.str(&self.name);
        w.u8(self.mode.id());
        w.u8(self.players);
        w.u8(self.max);
        w.u16(self.port);
    }
    pub fn read(r: &mut R) -> Option<ServerInfo> {
        Some(ServerInfo { name: r.str()?, mode: Mode::from_id(r.u8()?), players: r.u8()?, max: r.u8()?, port: r.u16()? })
    }
}

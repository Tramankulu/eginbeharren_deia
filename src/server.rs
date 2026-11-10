//! Servidor autoritativo. Lo usan tanto el servidor dedicado como el
//! "Crear partida" del juego (en un hilo aparte).

use crate::math::*;
use crate::net::*;
use crate::sim::*;
use crate::world::World;
use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub name: String,
    pub port: u16,
    pub max_players: usize,
    /// Rotación de modos.
    pub modes: Vec<Mode>,
    /// Sustituye al límite de puntuación por defecto del modo.
    pub score_limit: Option<u16>,
    /// Sustituye al límite de tiempo por defecto (minutos).
    pub time_limit_min: Option<f32>,
    /// Jugadores necesarios para empezar a puntuar.
    pub min_players: usize,
    pub log: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            name: "Servidor de Eginbeharraren deia".into(),
            port: DEFAULT_PORT,
            max_players: 12,
            modes: vec![Mode::Tdm],
            score_limit: None,
            time_limit_min: None,
            min_players: 2,
            log: false,
        }
    }
}

pub const SD_ROUND_TIME: f32 = 150.0;
pub const SD_BOMB_TIME: f32 = 45.0;
pub const SD_PLANT_TIME: f32 = 5.0;
pub const SD_DEFUSE_TIME: f32 = 7.0;
pub const SITE_RADIUS: f32 = 2.2;
pub const FLAG_RADIUS: f32 = 1.6;
pub const FLAG_RETURN: f32 = 30.0;

fn default_limit(m: Mode) -> u16 {
    match m {
        Mode::Dm => 30,
        Mode::Tdm => 75,
        Mode::Sd => 4,
        Mode::Ctf => 5,
    }
}

fn default_minutes(m: Mode) -> f32 {
    match m {
        Mode::Dm => 10.0,
        Mode::Tdm => 15.0,
        Mode::Sd => 0.0,
        Mode::Ctf => 15.0,
    }
}

pub struct Slot {
    pub addr: SocketAddr,
    pub name: String,
    pub team: u8,
    pub ps: PState,
    pub alive: bool,
    pub health: f32,
    pub since_hit: f32,
    pub respawn_t: f32,
    pub last_seq: u32,
    pub last_recv: Instant,
    pub event_ack: u32,
    pub budget: f32,
    pub team_req: u8,
    pub kills: u16,
    pub deaths: u16,
    pub score: i16,
    pub ping: f32,
    pub last_chat: u16,
    pub history: VecDeque<(u32, V3, f32)>,
    pub use_held: bool,
    pub progress: f32,
}

pub struct Server {
    pub sock: UdpSocket,
    pub cfg: ServerConfig,
    pub world: World,
    pub slots: Vec<Option<Slot>>,
    pub tick: u32,
    pub rng: Rng,
    pub mode: Mode,
    mode_idx: usize,
    pub phase: Phase,
    pub phase_t: f32,
    pub team_scores: [u16; 2],
    pub round: u8,
    pub flags: [FlagInfo; 2],
    flag_drop_t: [f32; 2],
    pub bomb_state: u8,
    pub bomb_site: u8,
    pub bomb_timer: f32,
    pub grenades: Vec<Grenade>,
    next_grenade: u16,
    events: VecDeque<(u32, Ev)>,
    next_event: u32,
    warmup_count: f32,
}

impl Server {
    pub fn bind(cfg: ServerConfig) -> std::io::Result<Server> {
        let sock = UdpSocket::bind(("0.0.0.0", cfg.port))?;
        sock.set_nonblocking(true)?;
        let world = World::village();
        let mode = cfg.modes.first().copied().unwrap_or(Mode::Tdm);
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let mut s = Server {
            sock,
            world,
            slots: (0..MAX_PLAYERS).map(|_| None).collect(),
            tick: 0,
            rng: Rng::new(seed),
            mode,
            mode_idx: 0,
            phase: Phase::Warmup,
            phase_t: 0.0,
            team_scores: [0, 0],
            round: 0,
            flags: [FlagInfo { state: flag_state::HOME, carrier: 0, pos: V3::ZERO }; 2],
            flag_drop_t: [0.0; 2],
            bomb_state: bomb_state::NONE,
            bomb_site: 0,
            bomb_timer: 0.0,
            grenades: Vec::new(),
            next_grenade: 1,
            events: VecDeque::new(),
            next_event: 1,
            warmup_count: -1.0,
            cfg,
        };
        s.cfg.max_players = s.cfg.max_players.clamp(2, MAX_PLAYERS);
        if s.cfg.modes.is_empty() {
            s.cfg.modes.push(Mode::Tdm);
        }
        s.setup_mode();
        s.log(&format!(
            "Servidor \"{}\" escuchando en el puerto UDP {} · modos: {}",
            s.cfg.name,
            s.local_port(),
            s.cfg.modes.iter().map(|m| m.name()).collect::<Vec<_>>().join(", ")
        ));
        Ok(s)
    }

    pub fn local_port(&self) -> u16 {
        self.sock.local_addr().map(|a| a.port()).unwrap_or(self.cfg.port)
    }

    fn log(&self, s: &str) {
        if self.cfg.log {
            println!("[{:>6.0}s] {}", self.tick as f32 / TICK_HZ as f32, s);
        }
    }

    /// Bucle principal a 60 ticks por segundo hasta que `stop` sea true.
    pub fn run(mut self, stop: Arc<AtomicBool>) {
        let step = Duration::from_nanos(1_000_000_000 / TICK_HZ as u64);
        let mut next = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            self.update();
            next += step;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else if now - next > Duration::from_millis(250) {
                next = now;
            }
        }
        self.shutdown("El servidor se ha cerrado");
    }

    pub fn shutdown(&mut self, reason: &str) {
        for s in self.slots.iter().flatten() {
            let mut w = W::new(s2c::KICKED);
            w.str(reason);
            let _ = self.sock.send_to(&w.0, s.addr);
        }
    }

    pub fn player_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    fn team_count(&self, t: u8) -> usize {
        self.slots.iter().flatten().filter(|s| s.team == t).count()
    }

    fn push(&mut self, e: Ev) {
        self.events.push_back((self.next_event, e));
        self.next_event += 1;
        while self.events.len() > 512 {
            self.events.pop_front();
        }
    }

    fn announce(&mut self, text: impl Into<String>, tone: u8) {
        let text = text.into();
        self.log(&text);
        self.push(Ev::Announce { text, tone });
    }

    pub fn score_limit(&self) -> u16 {
        self.cfg.score_limit.unwrap_or_else(|| default_limit(self.mode))
    }

    fn match_seconds(&self) -> f32 {
        self.cfg.time_limit_min.unwrap_or_else(|| default_minutes(self.mode)) * 60.0
    }

    fn side_of(&self, id: usize, team: u8) -> u8 {
        if self.mode.teams() {
            team
        } else {
            (id % 2) as u8
        }
    }

    pub fn attackers(&self) -> u8 {
        if (self.round.max(1) as u32 - 1) / 3 % 2 == 0 {
            AXIS
        } else {
            ALLIES
        }
    }

    // ------------------------------------------------------------------
    // Red
    // ------------------------------------------------------------------

    fn recv_all(&mut self) {
        let mut buf = [0u8; 2048];
        for _ in 0..4096 {
            match self.sock.recv_from(&mut buf) {
                Ok((n, addr)) => {
                    let data = buf[..n].to_vec();
                    self.handle(&data, addr);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => break,
                // En Windows, un ICMP "puerto inalcanzable" aparece como error aquí: se ignora.
                Err(_) => continue,
            }
        }
    }

    fn slot_of(&self, addr: SocketAddr) -> Option<usize> {
        self.slots.iter().position(|s| s.as_ref().map(|s| s.addr == addr).unwrap_or(false))
    }

    fn handle(&mut self, data: &[u8], addr: SocketAddr) {
        let Some((kind, mut r)) = R::open(data) else { return };
        match kind {
            c2s::DISCOVER => {
                let nonce = r.u32().unwrap_or(0);
                let mut w = W::new(s2c::INFO);
                w.u32(nonce);
                ServerInfo {
                    name: self.cfg.name.clone(),
                    mode: self.mode,
                    players: self.player_count() as u8,
                    max: self.cfg.max_players as u8,
                    port: self.local_port(),
                }
                .write(&mut w);
                let _ = self.sock.send_to(&w.0, addr);
            }
            c2s::CONNECT => {
                let (Some(proto), Some(name)) = (r.u16(), r.str()) else { return };
                self.on_connect(addr, proto, &name);
            }
            c2s::CMDS => {
                if let Some(id) = self.slot_of(addr) {
                    self.on_cmds(id, &mut r);
                }
            }
            c2s::DISCONNECT => {
                if let Some(id) = self.slot_of(addr) {
                    self.remove(id, "se ha desconectado");
                }
            }
            _ => {}
        }
    }

    fn send_welcome(&self, id: usize) {
        let s = self.slots[id].as_ref().unwrap();
        let mut w = W::new(s2c::WELCOME);
        w.u8(id as u8);
        w.u32(self.tick);
        w.str(&self.cfg.name);
        w.u8(self.mode.id());
        let _ = self.sock.send_to(&w.0, s.addr);
    }

    fn on_connect(&mut self, addr: SocketAddr, proto: u16, name: &str) {
        if let Some(id) = self.slot_of(addr) {
            self.send_welcome(id);
            return;
        }
        let reject = |s: &Server, why: &str| {
            let mut w = W::new(s2c::REJECT);
            w.str(why);
            let _ = s.sock.send_to(&w.0, addr);
        };
        if proto != PROTOCOL {
            reject(self, "Versión del juego distinta a la del servidor");
            return;
        }
        if self.player_count() >= self.cfg.max_players {
            reject(self, "El servidor está lleno");
            return;
        }
        let Some(id) = self.slots.iter().position(|s| s.is_none()) else {
            reject(self, "El servidor está lleno");
            return;
        };
        let mut name = clean_name(name, 20);
        let base = name.clone();
        let mut k = 2;
        while self.slots.iter().flatten().any(|s| s.name == name) {
            name = format!("{base} ({k})");
            k += 1;
        }
        let team = if self.mode.teams() { self.balanced_team() } else { 0 };
        let side = self.side_of(id, team);
        self.slots[id] = Some(Slot {
            addr,
            name: name.clone(),
            team,
            ps: PState::new(V3::ZERO, 0.0, side),
            alive: false,
            health: MAX_HEALTH,
            since_hit: 99.0,
            respawn_t: 0.5,
            last_seq: 0,
            last_recv: Instant::now(),
            event_ack: self.next_event - 1,
            budget: 4.0,
            team_req: 0,
            kills: 0,
            deaths: 0,
            score: 0,
            ping: 0.0,
            last_chat: 0,
            history: VecDeque::new(),
            use_held: false,
            progress: 0.0,
        });
        self.send_welcome(id);
        let txt = if self.mode.teams() {
            format!("{} se une a los {}", name, team_name(team))
        } else {
            format!("{name} se une a la partida")
        };
        self.announce(txt, tone::NEUTRAL);
    }

    fn balanced_team(&mut self) -> u8 {
        let a = self.team_count(ALLIES);
        let b = self.team_count(AXIS);
        if a < b {
            ALLIES
        } else if b < a {
            AXIS
        } else if self.team_scores[0] > self.team_scores[1] {
            AXIS
        } else if self.team_scores[1] > self.team_scores[0] {
            ALLIES
        } else if self.rng.chance(0.5) {
            ALLIES
        } else {
            AXIS
        }
    }

    pub fn remove(&mut self, id: usize, why: &str) {
        if self.slots[id].is_none() {
            return;
        }
        self.drop_flag(id);
        let name = self.slots[id].as_ref().unwrap().name.clone();
        self.slots[id] = None;
        self.announce(format!("{name} {why}"), tone::NEUTRAL);
    }

    fn on_cmds(&mut self, id: usize, r: &mut R) {
        let (Some(snap_tick), Some(ev_ack), Some(team_req), Some(chat_id)) = (r.u32(), r.u32(), r.u8(), r.u16()) else {
            return;
        };
        let chat = if chat_id != 0 {
            let (Some(team_only), Some(text)) = (r.bool(), r.str()) else { return };
            Some((team_only, text))
        } else {
            None
        };
        let Some(n) = r.u8() else { return };
        let mut cmds = Vec::with_capacity(n as usize);
        for _ in 0..n.min(8) {
            match read_cmd(r) {
                Some(c) => cmds.push(c),
                None => return,
            }
        }
        let tick = self.tick;
        {
            let s = self.slots[id].as_mut().unwrap();
            s.last_recv = Instant::now();
            if ev_ack > s.event_ack && ev_ack < self.next_event {
                s.event_ack = ev_ack;
            }
            if snap_tick <= tick && snap_tick > 0 {
                let p = (tick - snap_tick) as f32 * 1000.0 / TICK_HZ as f32;
                s.ping = if s.ping == 0.0 { p } else { s.ping * 0.9 + p * 0.1 };
            }
        }
        if let Some((team_only, text)) = chat {
            let s = self.slots[id].as_mut().unwrap();
            if chat_id != s.last_chat {
                s.last_chat = chat_id;
                let text: String = text.chars().filter(|c| !c.is_control()).take(100).collect();
                let name = s.name.clone();
                if !text.trim().is_empty() {
                    self.log(&format!("{name}: {text}"));
                    self.push(Ev::Chat { from: id as u8, team_only, name, text });
                }
            }
        }
        if team_req != 0 {
            self.request_team(id, team_req);
        }
        // Comandos en orden, una sola vez cada uno
        cmds.sort_by_key(|c| c.seq);
        for c in cmds {
            let s = self.slots[id].as_mut().unwrap();
            if c.seq <= s.last_seq {
                continue;
            }
            if s.budget < 1.0 {
                // Más comandos de los posibles: se descarta (antitrampas de velocidad)
                s.last_seq = c.seq;
                continue;
            }
            s.budget -= 1.0;
            s.last_seq = c.seq;
            self.apply_cmd(id, &c);
        }
    }

    fn request_team(&mut self, id: usize, req: u8) {
        if !self.mode.teams() {
            return;
        }
        let want = if req == 1 { ALLIES } else if req == 2 { AXIS } else { return };
        let s = self.slots[id].as_mut().unwrap();
        if s.team_req == req {
            return;
        }
        s.team_req = req;
        if s.team == want {
            return;
        }
        // Impedir desequilibrar más de un jugador
        let (mine, other) = (self.team_count(want), self.team_count(1 - want));
        if mine >= other {
            return;
        }
        self.drop_flag(id);
        let side = want;
        let s = self.slots[id].as_mut().unwrap();
        s.team = want;
        s.ps.side = side;
        s.alive = false;
        s.respawn_t = 1.0;
        let name = s.name.clone();
        self.announce(format!("{} se cambia a los {}", name, team_name(want)), tone::NEUTRAL);
    }

    fn apply_cmd(&mut self, id: usize, cmd: &Cmd) {
        let frozen = self.phase == Phase::Intermission;
        let out = {
            let s = self.slots[id].as_mut().unwrap();
            s.use_held = cmd.has(btn::USE);
            if !s.alive || frozen {
                s.ps.yaw = cmd.yaw;
                s.ps.pitch = cmd.pitch;
                return;
            }
            step(&self.world, &mut s.ps, cmd, &mut self.rng)
        };
        let s = self.slots[id].as_ref().unwrap();
        let (pos, side, slot) = (s.ps.pos, s.ps.side, s.ps.slot);
        if let Some((eye, dir)) = out.shot {
            self.fire(id, eye, dir, cmd.view_tick, gun_index(side, slot) as u8);
        }
        if let Some((p, v)) = out.throw {
            let gid = self.next_grenade;
            self.next_grenade = self.next_grenade.wrapping_add(1).max(1);
            self.grenades.push(Grenade { id: gid, pos: p, vel: v, fuse: 3.0, owner: id as u8, side, bounce_cd: 0.0 });
            self.push(Ev::Throw { who: id as u8, pos: p });
        }
        if out.reload {
            self.push(Ev::Reload { who: id as u8, pos });
        }
        if out.ping {
            self.push(Ev::Ping { who: id as u8, pos });
        }
    }

    fn history_at(s: &Slot, tick: u32) -> (V3, f32) {
        let mut best = (s.ps.pos, s.ps.crouch);
        let mut bd = u32::MAX;
        for &(t, p, c) in s.history.iter() {
            let d = t.abs_diff(tick);
            if d < bd {
                bd = d;
                best = (p, c);
            }
        }
        best
    }

    fn fire(&mut self, shooter: usize, eye: V3, dir: V3, view_tick: u32, gun: u8) {
        let def = &GUNS[gun as usize];
        let rewind = view_tick.clamp(self.tick.saturating_sub(TICK_HZ), self.tick);
        let my_team = self.slots[shooter].as_ref().unwrap().team;
        let teams = self.mode.teams();
        let wh = self.world.raycast(eye, dir, 250.0);
        let mut max_t = wh.as_ref().map(|h| h.t).unwrap_or(250.0);
        let mut hit: Option<(usize, bool)> = None;
        for (i, s) in self.slots.iter().enumerate() {
            let Some(s) = s else { continue };
            if i == shooter || !s.alive || (teams && s.team == my_team) {
                continue;
            }
            let (pos, crouch) = Self::history_at(s, rewind);
            let (body, head) = hitboxes(pos, crouch);
            if let Some((t, _)) = head.ray(eye, dir, max_t) {
                if t < max_t {
                    max_t = t;
                    hit = Some((i, true));
                }
            }
            if let Some((t, _)) = body.ray(eye, dir, max_t) {
                if t < max_t {
                    max_t = t;
                    hit = Some((i, false));
                }
            }
        }
        let right = V3::right_of_yaw(dir.yaw());
        let from = eye + dir * 0.6 + right * 0.12 - V3::UP * 0.12;
        let to = eye + dir * max_t;
        self.push(Ev::Shot { shooter: shooter as u8, from, to, gun });
        if let Some((victim, head)) = hit {
            let mut dmg = def.damage * if head { def.head_mult } else { 1.0 };
            if max_t > 35.0 {
                dmg *= def.falloff;
            }
            self.push(Ev::Hit { shooter: shooter as u8 });
            self.damage(victim, dmg, Some(shooter), gun, head, eye);
        }
    }

    fn damage(&mut self, victim: usize, dmg: f32, attacker: Option<usize>, weapon: u8, head: bool, from: V3) {
        if self.phase == Phase::Intermission {
            return;
        }
        let Some(v) = self.slots[victim].as_mut() else { return };
        if !v.alive {
            return;
        }
        v.health -= dmg;
        v.since_hit = 0.0;
        self.push(Ev::Hurt { victim: victim as u8, from });
        if self.slots[victim].as_ref().unwrap().health <= 0.0 {
            self.kill(victim, attacker, weapon, head);
        }
    }

    fn respawn_delay(&self) -> f32 {
        match (self.phase, self.mode) {
            (Phase::Warmup, _) => 2.0,
            (_, Mode::Ctf) => 5.0,
            _ => 3.0,
        }
    }

    fn kill(&mut self, victim: usize, attacker: Option<usize>, weapon: u8, head: bool) {
        self.drop_flag(victim);
        let counting = self.phase == Phase::Playing;
        let delay = self.respawn_delay();
        let (vname, vteam) = {
            let v = self.slots[victim].as_mut().unwrap();
            v.alive = false;
            v.health = 0.0;
            v.respawn_t = delay;
            v.progress = 0.0;
            if counting {
                v.deaths += 1;
            }
            (v.name.clone(), v.team)
        };
        let killer = attacker.filter(|&a| a != victim && self.slots[a].is_some());
        let mut kname = String::new();
        match killer {
            Some(a) => {
                let k = self.slots[a].as_mut().unwrap();
                kname = k.name.clone();
                let kteam = k.team;
                if counting {
                    k.kills += 1;
                    k.score += 1;
                    if self.mode == Mode::Tdm && kteam != vteam {
                        self.team_scores[kteam as usize] += 1;
                    }
                }
            }
            None => {
                if counting && matches!(self.mode, Mode::Dm | Mode::Tdm) {
                    let v = self.slots[victim].as_mut().unwrap();
                    v.score -= 1;
                }
            }
        }
        let weapon = if killer.is_none() && weapon != BOMB_W { weapon::SUICIDE } else { weapon };
        self.log(&format!("{} -> {} ({})", if kname.is_empty() { "-" } else { &kname }, vname, weapon));
        self.push(Ev::Kill {
            killer: killer.map(|k| k as u8).unwrap_or(victim as u8),
            victim: victim as u8,
            weapon,
            head,
            killer_name: kname,
            victim_name: vname,
        });
        if counting {
            self.check_score_end();
        }
    }

    // ------------------------------------------------------------------
    // Reaparición
    // ------------------------------------------------------------------

    fn spawn(&mut self, id: usize) {
        let team = self.slots[id].as_ref().unwrap().team;
        let side = self.side_of(id, team);
        let (pos, yaw) = self.pick_spawn(id, team);
        let s = self.slots[id].as_mut().unwrap();
        s.ps = PState::new(pos, yaw, side);
        s.alive = true;
        s.health = MAX_HEALTH;
        s.since_hit = 99.0;
        s.progress = 0.0;
        s.history.clear();
    }

    fn occupied(&self, p: V3, except: usize) -> bool {
        self.slots
            .iter()
            .enumerate()
            .any(|(i, s)| i != except && s.as_ref().map(|s| s.alive && s.ps.pos.dist_flat(p) < 1.2).unwrap_or(false))
    }

    fn pick_spawn(&mut self, id: usize, team: u8) -> (V3, f32) {
        let fixed: Option<&Vec<V3>> = match self.mode {
            Mode::Sd => Some(if team == self.attackers() { &self.world.axis_spawns } else { &self.world.allied_spawns }),
            Mode::Ctf => Some(if team == ALLIES { &self.world.allied_spawns } else { &self.world.axis_spawns }),
            _ => None,
        };
        if let Some(list) = fixed {
            let list = list.clone();
            let start = self.rng.index(list.len());
            for k in 0..list.len() {
                let p = list[(start + k) % list.len()];
                if !self.occupied(p, id) {
                    return (p, (v3(0.0, 0.0, 0.0) - p).yaw());
                }
            }
            let p = list[start] + v3(self.rng.range(-1.0, 1.0), 0.0, self.rng.range(-1.0, 1.0));
            return (p, (V3::ZERO - p).yaw());
        }
        // Reaparición dinámica: lejos de los enemigos y fuera de su vista
        let mut cands: Vec<V3> = self.world.waypoints.clone();
        cands.extend(self.world.allied_spawns.iter());
        cands.extend(self.world.axis_spawns.iter());
        let teams = self.mode.teams();
        let enemies: Vec<V3> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let s = s.as_ref()?;
                (i != id && s.alive && (!teams || s.team != team)).then(|| s.ps.eye())
            })
            .collect();
        let friends: Vec<V3> = if teams {
            self.slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    let s = s.as_ref()?;
                    (i != id && s.alive && s.team == team).then_some(s.ps.pos)
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut best = (cands[0], f32::MIN);
        for &c in &cands {
            if self.occupied(c, id) {
                continue;
            }
            let eye = c + V3::UP * STAND_EYE;
            let mind = enemies.iter().map(|e| e.dist(eye)).fold(60.0f32, f32::min);
            let mut score = mind.min(60.0);
            if enemies.iter().any(|e| e.dist(eye) < 70.0 && self.world.los(*e, eye)) {
                score -= 40.0;
            }
            if let Some(fd) = friends.iter().map(|f| f.dist(c)).reduce(f32::min) {
                score += (25.0 - fd).max(0.0) * 0.4;
            }
            score += self.rng.range(0.0, 8.0);
            if score > best.1 {
                best = (c, score);
            }
        }
        let p = best.0;
        let face = enemies.first().copied().unwrap_or(V3::ZERO);
        (p, (face - p).yaw())
    }

    // ------------------------------------------------------------------
    // Modos de juego
    // ------------------------------------------------------------------

    fn setup_mode(&mut self) {
        self.team_scores = [0, 0];
        self.round = 0;
        self.grenades.clear();
        self.bomb_state = bomb_state::NONE;
        self.bomb_timer = 0.0;
        for t in 0..2 {
            self.flags[t] = FlagInfo { state: flag_state::HOME, carrier: 0, pos: self.world.flag_bases[t] };
        }
        // Reequilibrar equipos al cambiar de modo
        let ids: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].is_some()).collect();
        for (k, &i) in ids.iter().enumerate() {
            let team = if self.mode.teams() { (k % 2) as u8 } else { 0 };
            let side = self.side_of(i, team);
            let s = self.slots[i].as_mut().unwrap();
            s.team = team;
            s.ps.side = side;
            s.team_req = 0;
            s.alive = false;
            s.respawn_t = 0.5;
            s.kills = 0;
            s.deaths = 0;
            s.score = 0;
        }
        self.phase = Phase::Warmup;
        self.warmup_count = -1.0;
        self.phase_t = 0.0;
        self.log(&format!("Modo: {}", self.mode.name()));
    }

    fn enough_players(&self) -> bool {
        let n = self.player_count();
        if n < self.cfg.min_players.max(1) {
            return false;
        }
        if self.mode.teams() && self.cfg.min_players >= 2 {
            return self.team_count(ALLIES) > 0 && self.team_count(AXIS) > 0;
        }
        true
    }

    fn start_match(&mut self) {
        self.team_scores = [0, 0];
        self.round = 0;
        self.grenades.clear();
        for t in 0..2 {
            self.flags[t] = FlagInfo { state: flag_state::HOME, carrier: 0, pos: self.world.flag_bases[t] };
        }
        for s in self.slots.iter_mut().flatten() {
            s.kills = 0;
            s.deaths = 0;
            s.score = 0;
        }
        self.phase = Phase::Playing;
        self.push(Ev::Whistle);
        if self.mode == Mode::Sd {
            self.start_round();
        } else {
            self.phase_t = self.match_seconds();
            let ids: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].is_some()).collect();
            for i in ids {
                self.spawn(i);
            }
            self.announce(format!("¡Empieza la partida! {}", self.mode.name()), tone::ALERT);
        }
    }

    fn start_round(&mut self) {
        self.round += 1;
        if self.round == 4 {
            self.announce("Descanso: los equipos cambian de papel", tone::NEUTRAL);
        }
        self.phase = Phase::Playing;
        self.phase_t = SD_ROUND_TIME;
        self.bomb_state = bomb_state::NONE;
        self.bomb_timer = 0.0;
        self.grenades.clear();
        let ids: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].is_some()).collect();
        for i in ids {
            self.spawn(i);
        }
        let att = self.attackers();
        self.push(Ev::Whistle);
        self.announce(
            format!("Ronda {}: atacan los {}. Destruid el objetivo A o B.", self.round, team_name(att)),
            tone::ALERT,
        );
    }

    fn end_round(&mut self, winner: u8, reason: &str) {
        self.team_scores[winner as usize] += 1;
        self.phase = Phase::RoundEnd;
        self.phase_t = 6.0;
        for s in self.slots.iter_mut().flatten() {
            s.progress = 0.0;
            s.ps.busy = false;
        }
        let tone = if winner == ALLIES { tone::ALLIES } else { tone::AXIS };
        self.announce(format!("{}. Ronda para los {}.", reason, team_name(winner)), tone);
        if self.team_scores[winner as usize] >= self.score_limit() {
            self.end_match();
        }
    }

    fn check_score_end(&mut self) {
        if self.phase != Phase::Playing {
            return;
        }
        let limit = self.score_limit() as i32;
        let done = match self.mode {
            Mode::Dm => self.slots.iter().flatten().any(|s| s.score as i32 >= limit),
            Mode::Tdm | Mode::Ctf => self.team_scores.iter().any(|&t| t as i32 >= limit),
            Mode::Sd => false,
        };
        if done {
            self.end_match();
        }
    }

    fn end_match(&mut self) {
        let text = match self.mode {
            Mode::Dm => {
                let best = self.slots.iter().flatten().max_by_key(|s| (s.score, s.kills));
                match best {
                    Some(s) => format!("¡{} gana la partida con {} puntos!", s.name, s.score),
                    None => "Fin de la partida".to_string(),
                }
            }
            _ => {
                let [a, b] = self.team_scores;
                if a > b {
                    format!("¡Victoria de los Aliados! ({a} a {b})")
                } else if b > a {
                    format!("¡Victoria del Eje! ({b} a {a})")
                } else {
                    format!("Empate ({a} a {b})")
                }
            }
        };
        self.phase = Phase::Intermission;
        self.phase_t = 12.0;
        self.grenades.clear();
        self.announce(text, tone::ALERT);
    }

    fn next_mode(&mut self) {
        self.mode_idx = (self.mode_idx + 1) % self.cfg.modes.len();
        self.mode = self.cfg.modes[self.mode_idx];
        self.setup_mode();
        self.announce(format!("Siguiente partida: {}", self.mode.name()), tone::NEUTRAL);
    }

    fn drop_flag(&mut self, id: usize) {
        if self.mode != Mode::Ctf {
            return;
        }
        for t in 0..2 {
            if self.flags[t].state == flag_state::CARRIED && self.flags[t].carrier as usize == id {
                let pos = self.slots[id].as_ref().map(|s| s.ps.pos).unwrap_or(self.world.flag_bases[t]);
                self.flags[t] = FlagInfo { state: flag_state::DROPPED, carrier: 0, pos: v3(pos.x, 0.0, pos.z) };
                self.flag_drop_t[t] = FLAG_RETURN;
                self.announce(format!("La bandera del {} ha caído", team_name(t as u8)), tone::NEUTRAL);
            }
        }
    }

    fn update_mode(&mut self, dt: f32) {
        match self.phase {
            Phase::Warmup => {
                if self.enough_players() {
                    if self.warmup_count < 0.0 {
                        self.warmup_count = 5.0;
                        self.announce("Hay jugadores suficientes: la partida empieza en 5 segundos", tone::NEUTRAL);
                    }
                    self.warmup_count -= dt;
                    self.phase_t = self.warmup_count.max(0.0);
                    if self.warmup_count <= 0.0 {
                        self.warmup_count = -1.0;
                        self.start_match();
                    }
                } else {
                    self.warmup_count = -1.0;
                    self.phase_t = 0.0;
                }
            }
            Phase::Playing => {
                if !self.enough_players() {
                    self.phase = Phase::Warmup;
                    self.announce("Esperando a más jugadores…", tone::NEUTRAL);
                    return;
                }
                match self.mode {
                    Mode::Sd => self.update_sd(dt),
                    Mode::Ctf => {
                        self.update_ctf(dt);
                        self.tick_match_clock(dt);
                    }
                    _ => self.tick_match_clock(dt),
                }
            }
            Phase::RoundEnd => {
                self.phase_t -= dt;
                if self.phase_t <= 0.0 {
                    self.start_round();
                }
            }
            Phase::Intermission => {
                self.phase_t -= dt;
                if self.phase_t <= 0.0 {
                    self.next_mode();
                }
            }
        }
    }

    fn tick_match_clock(&mut self, dt: f32) {
        self.phase_t -= dt;
        if self.phase_t <= 0.0 && self.phase == Phase::Playing {
            self.announce("¡Se acabó el tiempo!", tone::NEUTRAL);
            self.end_match();
        }
    }

    fn update_ctf(&mut self, dt: f32) {
        for t in 0..2 {
            match self.flags[t].state {
                flag_state::HOME => self.flags[t].pos = self.world.flag_bases[t],
                flag_state::CARRIED => {
                    let c = self.flags[t].carrier as usize;
                    match self.slots.get(c).and_then(|s| s.as_ref()) {
                        Some(s) if s.alive => self.flags[t].pos = s.ps.pos,
                        _ => self.flags[t] = FlagInfo { state: flag_state::HOME, carrier: 0, pos: self.world.flag_bases[t] },
                    }
                }
                _ => {
                    self.flag_drop_t[t] -= dt;
                    if self.flag_drop_t[t] <= 0.0 {
                        self.flags[t] = FlagInfo { state: flag_state::HOME, carrier: 0, pos: self.world.flag_bases[t] };
                        self.announce(format!("La bandera del {} vuelve a su base", team_name(t as u8)), tone::NEUTRAL);
                    }
                }
            }
        }
        for id in 0..self.slots.len() {
            let Some(s) = self.slots[id].as_ref() else { continue };
            if !s.alive {
                continue;
            }
            let (pos, team, name) = (s.ps.pos, s.team as usize, s.name.clone());
            let enemy = 1 - team;
            let ef = self.flags[enemy];
            if (ef.state == flag_state::HOME || ef.state == flag_state::DROPPED) && ef.pos.dist_flat(pos) < FLAG_RADIUS {
                self.flags[enemy] = FlagInfo { state: flag_state::CARRIED, carrier: id as u8, pos };
                let tone = if team == 0 { tone::ALLIES } else { tone::AXIS };
                self.announce(format!("¡{} tiene la bandera del {}!", name, team_name(enemy as u8)), tone);
            }
            let of = self.flags[team];
            if of.state == flag_state::DROPPED && of.pos.dist_flat(pos) < FLAG_RADIUS {
                self.flags[team] = FlagInfo { state: flag_state::HOME, carrier: 0, pos: self.world.flag_bases[team] };
                self.slots[id].as_mut().unwrap().score += 2;
                self.announce(format!("{} devuelve la bandera de los {}", name, team_name(team as u8)), tone::NEUTRAL);
            }
            let ef = self.flags[enemy];
            if ef.state == flag_state::CARRIED
                && ef.carrier as usize == id
                && self.flags[team].state == flag_state::HOME
                && pos.dist_flat(self.world.flag_bases[team]) < FLAG_RADIUS + 0.6
            {
                self.flags[enemy] = FlagInfo { state: flag_state::HOME, carrier: 0, pos: self.world.flag_bases[enemy] };
                self.team_scores[team] += 1;
                self.slots[id].as_mut().unwrap().score += 10;
                let tone = if team == 0 { tone::ALLIES } else { tone::AXIS };
                self.announce(format!("¡{} captura la bandera! Punto para los {}", name, team_name(team as u8)), tone);
                self.push(Ev::Whistle);
                self.check_score_end();
            }
        }
    }

    fn update_sd(&mut self, dt: f32) {
        let att = self.attackers();
        let def = 1 - att;
        let sites = self.world.bomb_sites;
        let planted_at = sites[self.bomb_site as usize];
        let mut plant: Option<(usize, u8)> = None;
        let mut defused_by: Option<usize> = None;
        for id in 0..self.slots.len() {
            let Some(s) = self.slots[id].as_mut() else { continue };
            if !s.alive {
                s.progress = 0.0;
                s.ps.busy = false;
                continue;
            }
            let mut working = false;
            if s.use_held && s.ps.grounded {
                if self.bomb_state == bomb_state::NONE && s.team == att {
                    if let Some(site) = (0..2).find(|&k| sites[k].dist_flat(s.ps.pos) < SITE_RADIUS) {
                        working = true;
                        s.progress += dt / SD_PLANT_TIME;
                        if s.progress >= 1.0 && plant.is_none() {
                            plant = Some((id, site as u8));
                        }
                    }
                } else if self.bomb_state == bomb_state::PLANTED && s.team == def && planted_at.dist_flat(s.ps.pos) < SITE_RADIUS {
                    working = true;
                    s.progress += dt / SD_DEFUSE_TIME;
                    if s.progress >= 1.0 && defused_by.is_none() {
                        defused_by = Some(id);
                    }
                }
            }
            if !working {
                s.progress = 0.0;
            }
            s.ps.busy = working;
        }
        if let Some((id, site)) = plant {
            self.bomb_state = bomb_state::PLANTED;
            self.bomb_site = site;
            self.bomb_timer = SD_BOMB_TIME;
            for s in self.slots.iter_mut().flatten() {
                s.progress = 0.0;
                s.ps.busy = false;
            }
            let s = self.slots[id].as_mut().unwrap();
            s.score += 3;
            let name = s.name.clone();
            let letter = if site == 0 { "A" } else { "B" };
            self.announce(format!("¡{name} ha colocado la bomba en el objetivo {letter}!"), tone::ALERT);
        }
        if let Some(id) = defused_by {
            self.bomb_state = bomb_state::DEFUSED;
            let s = self.slots[id].as_mut().unwrap();
            s.score += 3;
            let name = s.name.clone();
            self.end_round(def, &format!("{name} ha desactivado la bomba"));
            return;
        }

        if self.bomb_state == bomb_state::PLANTED {
            self.bomb_timer -= dt;
            if self.bomb_timer <= 0.0 {
                self.bomb_state = bomb_state::EXPLODED;
                let p = self.world.bomb_sites[self.bomb_site as usize] + V3::UP * 0.5;
                self.explode(p, None, att, true);
                self.end_round(att, "El objetivo ha sido destruido");
                return;
            }
        } else {
            self.phase_t -= dt;
            if self.phase_t <= 0.0 {
                self.end_round(def, "Se acabó el tiempo");
                return;
            }
        }
        let alive = |t: u8| self.slots.iter().flatten().filter(|s| s.team == t && s.alive).count();
        let (aa, da) = (alive(att), alive(def));
        if da == 0 {
            self.end_round(att, &format!("Los {} han sido eliminados", team_name(def)));
        } else if aa == 0 && self.bomb_state == bomb_state::NONE {
            self.end_round(def, &format!("Los {} han sido eliminados", team_name(att)));
        }
    }

    // ------------------------------------------------------------------
    // Granadas y explosiones
    // ------------------------------------------------------------------

    fn update_grenades(&mut self, dt: f32) {
        let mut booms = Vec::new();
        let mut bounces = Vec::new();
        for g in self.grenades.iter_mut() {
            if grenade_step(&self.world, g, dt) {
                bounces.push(g.pos);
            }
            if g.fuse <= 0.0 {
                booms.push((g.pos, g.owner, g.side));
            }
        }
        self.grenades.retain(|g| g.fuse > 0.0);
        for p in bounces {
            self.push(Ev::Bounce { pos: p });
        }
        for (p, owner, side) in booms {
            let owner = owner as usize;
            let o = if self.slots[owner].is_some() { Some(owner) } else { None };
            self.explode(p, o, side, false);
        }
    }

    fn explode(&mut self, pos: V3, owner: Option<usize>, side: u8, big: bool) {
        self.push(Ev::Explosion { pos, big });
        let (radius, max_dmg) = if big { (14.0, 500.0) } else { (7.5, 230.0) };
        let teams = self.mode.teams();
        let owner_team = owner.and_then(|o| self.slots[o].as_ref()).map(|s| s.team);
        let origin = pos + V3::UP * 0.3;
        for i in 0..self.slots.len() {
            let Some(s) = self.slots[i].as_ref() else { continue };
            if !s.alive {
                continue;
            }
            if !big && teams && Some(i) != owner && Some(s.team) == owner_team {
                continue;
            }
            let c = s.ps.pos + V3::UP * 0.9;
            let d = c.dist(pos);
            if d < radius && (big || self.world.los(origin, c)) {
                let dmg = max_dmg * (1.0 - d / radius).powf(1.3) + 10.0;
                let w = if big { BOMB_W } else { weapon::GRENADE };
                self.damage(i, dmg, owner, w, false, pos);
            }
        }
        let _ = side;
    }

    // ------------------------------------------------------------------
    // Tick
    // ------------------------------------------------------------------

    pub fn update(&mut self) {
        let dt = DT;
        self.recv_all();
        self.tick += 1;

        let now = Instant::now();
        let timed_out: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().filter(|s| now.duration_since(s.last_recv).as_secs_f32() > TIMEOUT_SECS).map(|_| i))
            .collect();
        for i in timed_out {
            self.remove(i, "ha perdido la conexión");
        }

        let can_respawn = matches!(self.phase, Phase::Warmup | Phase::Playing) && (self.mode != Mode::Sd || self.phase == Phase::Warmup);
        let mut to_spawn = Vec::new();
        for (i, s) in self.slots.iter_mut().enumerate() {
            let Some(s) = s else { continue };
            s.budget = (s.budget + 1.0).min(10.0);
            if s.alive {
                s.since_hit += dt;
                if s.since_hit > 3.5 {
                    s.health = (s.health + 32.0 * dt).min(MAX_HEALTH);
                }
            } else {
                s.respawn_t -= dt;
                if s.respawn_t <= 0.0 && can_respawn {
                    to_spawn.push(i);
                }
            }
        }
        for i in to_spawn {
            self.spawn(i);
        }

        self.update_grenades(dt);
        self.update_mode(dt);

        let tick = self.tick;
        for s in self.slots.iter_mut().flatten() {
            s.history.push_back((tick, s.ps.pos, s.ps.crouch));
            while s.history.len() > TICK_HZ as usize + 10 {
                s.history.pop_front();
            }
        }

        if self.tick % SNAP_EVERY == 0 {
            self.send_snapshots();
        }
    }

    fn player_infos(&self) -> Vec<PlayerInfo> {
        let carrier = |id: usize| {
            self.mode == Mode::Ctf
                && self.flags.iter().any(|f| f.state == flag_state::CARRIED && f.carrier as usize == id)
        };
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let s = s.as_ref()?;
                Some(PlayerInfo {
                    id: i as u8,
                    name: s.name.clone(),
                    team: s.team,
                    side: s.ps.side,
                    alive: s.alive,
                    crouch: s.ps.crouch,
                    ads: s.ps.ads > 0.5,
                    sprint: s.ps.sprinting,
                    has_flag: carrier(i),
                    slot: s.ps.slot,
                    pos: s.ps.pos,
                    yaw: s.ps.yaw,
                    pitch: s.ps.pitch,
                    moving: s.ps.moving,
                    kills: s.kills,
                    deaths: s.deaths,
                    score: s.score,
                    ping: s.ping.round() as u16,
                })
            })
            .collect()
    }

    fn send_snapshots(&mut self) {
        let players = self.player_infos();
        let grenades: Vec<GrenadeInfo> = self.grenades.iter().map(|g| GrenadeInfo { id: g.id, pos: g.pos, side: g.side }).collect();
        for s in self.slots.iter().flatten() {
            let events: Vec<(u32, Ev)> = self.events.iter().filter(|(id, _)| *id > s.event_ack).cloned().collect();
            let snap = Snapshot {
                tick: self.tick,
                ack: s.last_seq,
                mode: self.mode,
                phase: self.phase,
                timer: self.phase_t.max(0.0),
                score_limit: self.score_limit(),
                team_scores: self.team_scores,
                round: self.round,
                attackers: self.attackers(),
                flags: self.flags,
                bomb_state: self.bomb_state,
                bomb_site: self.bomb_site,
                bomb_timer: self.bomb_timer.max(0.0),
                progress: s.progress,
                my_team: s.team,
                my_alive: s.alive,
                respawn_in: if s.alive { 0.0 } else { s.respawn_t.max(0.0) },
                health: s.health.max(0.0),
                me: if s.alive { Some(s.ps.clone()) } else { None },
                players: players.clone(),
                grenades: grenades.clone(),
                events,
            };
            let mut w = W::new(s2c::SNAPSHOT);
            snap.write(&mut w);
            let _ = self.sock.send_to(&w.0, s.addr);
        }
    }
}

const BOMB_W: u8 = weapon::BOMB;

/// Lanza un servidor en un hilo (para "Crear partida"). Devuelve el puerto
/// real y la bandera para detenerlo.
pub fn spawn_listen_server(cfg: ServerConfig) -> std::io::Result<(u16, Arc<AtomicBool>, std::thread::JoinHandle<()>)> {
    let server = Server::bind(cfg)?;
    let port = server.local_port();
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let h = std::thread::Builder::new().name("servidor".into()).spawn(move || server.run(s2))?;
    Ok((port, stop, h))
}

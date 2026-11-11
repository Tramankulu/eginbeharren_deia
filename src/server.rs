//! Servidor autoritativo. Lo usan tanto el servidor dedicado como el
//! "Crear partida" del juego (en un hilo aparte).

use crate::bots::{BotView, Brain, PView};
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
    /// Bots: se rellena con ellos hasta este número de jugadores (0 = sin bots)
    pub bots: usize,
    /// Puntería y reflejos de los bots (0 = novatos, 1 = veteranos)
    pub bot_skill: f32,
    /// Nación del bando aliado: estadounidenses, británicos o soviéticos
    pub allied_nation: u8,
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
            bots: 0,
            bot_skill: 0.5,
            allied_nation: nation::US,
        }
    }
}

/// Puntería de los bots según el nivel elegido (0 novato … 3 élite).
pub fn bot_skill_of(level: u8) -> f32 {
    [0.2, 0.45, 0.7, 0.92][(level as usize).min(3)]
}

pub const SD_ROUND_TIME: f32 = 150.0;
pub const SD_BOMB_TIME: f32 = 45.0;
pub const SD_PLANT_TIME: f32 = 5.0;
pub const SD_DEFUSE_TIME: f32 = 7.0;
pub const SITE_RADIUS: f32 = 2.2;
pub const FLAG_RADIUS: f32 = 1.6;
pub const FLAG_RETURN: f32 = 30.0;
pub const HQ_RADIUS: f32 = 4.0;
/// Lo que dura una nube de humo (segundos) y su radio
pub const SMOKE_TIME: f32 = 24.0;
pub const SMOKE_RADIUS: f32 = 5.5;
/// Lo que tarda en desaparecer un arma tirada
pub const DROP_TIME: f32 = 40.0;
pub const HQ_CAPTURE: f32 = 8.0;
pub const HQ_DESTROY: f32 = 8.0;
pub const HQ_HOLD: f32 = 90.0;
/// Segundos entre cada punto del equipo que tiene el cuartel
pub const HQ_POINT_EVERY: f32 = 2.0;

fn default_limit(m: Mode) -> u16 {
    match m {
        Mode::Dm => 30,
        Mode::Tdm => 75,
        Mode::Sd => 4,
        Mode::Ctf => 5,
        Mode::Hq => 150,
        Mode::Zombies => 0,
    }
}

fn default_minutes(m: Mode) -> f32 {
    match m {
        Mode::Dm => 10.0,
        Mode::Tdm => 15.0,
        Mode::Sd => 0.0,
        Mode::Ctf => 15.0,
        Mode::Hq => 15.0,
        Mode::Zombies => 0.0,
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
    pub loadout: Loadout,
    pub level: u8,
    /// Controlado por el servidor (bot) en lugar de por un jugador
    pub bot: Option<Box<Brain>>,
    pub zombie: bool,
    /// Pausa entre zarpazos (zombis)
    pub melee_cd: f32,
    /// Recién aparecido y sin haber combatido: un cambio de equipamiento se
    /// aplica al momento (como al elegir arma antes de salir)
    pub fresh: f32,
    /// Ya ha elegido sus armas (hasta entonces no aparece)
    pub ready: bool,
}

impl Slot {
    pub fn is_bot(&self) -> bool {
        self.bot.is_some()
    }
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
    // Cuartel General
    pub hq_state: u8,
    pub hq_owner: u8,
    pub hq_pos: V3,
    pub hq_progress: f32,
    hq_timer: f32,
    hq_point_t: f32,
    hq_capturer: u8,
    hq_spots: Vec<V3>,
    hq_idx: usize,
    // Zombis
    pub wave: u16,
    zombies_to_spawn: u16,
    wave_break: f32,
    /// Disparos recientes (para que los bots «oigan»): (tirador, origen, tick)
    recent_shots: Vec<(u8, V3, u32)>,
    bot_check: f32,
    /// Nubes de humo: (posición, edad en segundos)
    pub smokes: Vec<(V3, f32)>,
    /// Armas en el suelo: (información, munición en cargador, en reserva, tiempo que les queda)
    pub drops: Vec<(DropInfo, u32, u32, f32)>,
    next_drop: u16,
}

impl Server {
    pub fn bind(cfg: ServerConfig) -> std::io::Result<Server> {
        let sock = UdpSocket::bind(("0.0.0.0", cfg.port))?;
        sock.set_nonblocking(true)?;
        let world = World::load();
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
            hq_state: hq_state::NONE,
            hq_owner: 0,
            hq_pos: V3::ZERO,
            hq_progress: 0.0,
            hq_timer: 0.0,
            hq_point_t: 0.0,
            hq_capturer: 0,
            hq_spots: Vec::new(),
            hq_idx: 0,
            wave: 0,
            zombies_to_spawn: 0,
            wave_break: 0.0,
            recent_shots: Vec::new(),
            bot_check: 0.0,
            smokes: Vec::new(),
            drops: Vec::new(),
            next_drop: 1,
            cfg,
        };
        s.hq_spots = hq_spots(&s.world);
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

    /// Jugadores de verdad (sin bots).
    pub fn human_count(&self) -> usize {
        self.slots.iter().flatten().filter(|s| !s.is_bot()).count()
    }

    /// Nación de un bando en esta partida.
    pub fn nation_of(&self, side: u8) -> u8 {
        if self.mode == Mode::Zombies && side == AXIS {
            nation::ZOMBIE
        } else if side == AXIS {
            nation::DE
        } else {
            self.cfg.allied_nation
        }
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
        if self.mode.teams() || self.mode == Mode::Zombies {
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
        self.slots.iter().position(|s| s.as_ref().map(|s| s.addr == addr && !s.is_bot()).unwrap_or(false))
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
                let map = r.u32().unwrap_or(0);
                self.on_connect(addr, proto, &name, map);
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

    fn on_connect(&mut self, addr: SocketAddr, proto: u16, name: &str, map: u32) {
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
        if map != self.world.map_id {
            reject(self, "El servidor usa otro escenario: instala los mismos modelos que el servidor");
            return;
        }
        if self.human_count() >= self.cfg.max_players {
            reject(self, "El servidor está lleno");
            return;
        }
        // Un bot cede su sitio al jugador
        if self.slots.iter().all(|s| s.is_some()) || self.player_count() >= self.cfg.max_players {
            if let Some(b) = self.slots.iter().position(|s| s.as_ref().map(|s| s.is_bot() && !s.zombie).unwrap_or(false)) {
                self.remove_quiet(b);
            } else if let Some(b) = self.slots.iter().position(|s| s.as_ref().map(|s| s.is_bot()).unwrap_or(false)) {
                self.remove_quiet(b);
            }
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
        let team = if self.mode == Mode::Zombies { ALLIES } else if self.mode.teams() { self.balanced_team() } else { 0 };
        let side = self.side_of(id, team);
        self.slots[id] = Some(Slot {
            loadout: Loadout::default(),
            level: 1,
            bot: None,
            zombie: false,
            melee_cd: 0.0,
            fresh: 0.0,
            ready: false,
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
        // Con bots, se cuenta a los humanos para repartirlos también
        let (ha, hb) = (
            self.slots.iter().flatten().filter(|s| s.team == ALLIES && !s.is_bot()).count(),
            self.slots.iter().flatten().filter(|s| s.team == AXIS && !s.is_bot()).count(),
        );
        if a == b && ha != hb {
            return if ha < hb { ALLIES } else { AXIS };
        }
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

    /// Quita un bot sin anunciarlo.
    fn remove_quiet(&mut self, id: usize) {
        self.drop_flag(id);
        self.slots[id] = None;
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
        let (Some(snap_tick), Some(ev_ack), Some(team_req), Some(class), Some(primary), Some(perk_id), Some(secondary), Some(level), Some(ready), Some(chat_id)) =
            (r.u32(), r.u32(), r.u8(), r.u8(), r.u8(), r.u8(), r.u8(), r.u8(), r.bool(), r.u16())
        else {
            return;
        };
        {
            let nat = self.slots[id].as_ref().map(|s| s.ps.nation).unwrap_or(0);
            let s = self.slots[id].as_mut().unwrap();
            let new = Loadout { class: class.min(CLASSES.len() as u8 - 1), primary, perk: perk_id.min(perk::COUNT - 1), secondary };
            if new != s.loadout && s.alive && s.fresh > 0.0 && !s.ps.is_mounted() {
                // Aún no ha entrado en combate: cambiar de armas ya
                let (pos, yaw, pitch) = (s.ps.pos, s.ps.yaw, s.ps.pitch);
                s.ps = PState::equip(pos, yaw, s.ps.side, nat, &new);
                s.ps.pitch = pitch;
            }
            s.loadout = new;
            s.level = level.clamp(1, 20);
            s.ready = s.ready || ready;
        }
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
        if !self.mode.teams() || self.mode == Mode::Zombies {
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
        // Impedir desequilibrar más de un jugador (los bots se reparten solos)
        let humans = |t: u8| self.slots.iter().flatten().filter(|s| s.team == t && !s.is_bot()).count();
        let (mine, other) = (humans(want), humans(1 - want));
        if mine > other {
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
        let pressed_use = {
            let s = self.slots[id].as_ref().unwrap();
            cmd.has(btn::USE) && !s.use_held && s.alive && !frozen
        };
        if pressed_use && self.return_grenade(id) {
            let s = self.slots[id].as_mut().unwrap();
            s.use_held = true;
            return;
        }
        // F junto a un arma del suelo (y lejos de las ametralladoras): cogerla
        let near_turret = {
            let p = self.slots[id].as_ref().unwrap().ps.pos;
            self.world.turrets.iter().any(|t| t.stand.dist_flat(p) < MOUNT_RANGE)
        };
        if pressed_use && !near_turret && self.pickup_weapon(id) {
            let s = self.slots[id].as_mut().unwrap();
            s.use_held = true;
            return;
        }
        // Una ametralladora montada solo la puede usar uno
        let mut cmd = *cmd;
        if cmd.has(btn::MOUNT) {
            let me = &self.slots[id].as_ref().unwrap().ps;
            if !me.is_mounted() {
                let pos = me.pos;
                let taken = |k: usize| self.slots.iter().enumerate().any(|(i, o)| i != id && o.as_ref().map(|o| o.alive && o.ps.mounted as usize == k).unwrap_or(false));
                let free = self.world.turrets.iter().enumerate().any(|(k, t)| t.stand.dist_flat(pos) < MOUNT_RANGE && !taken(k));
                if !free || self.slots[id].as_ref().unwrap().zombie {
                    cmd.buttons &= !btn::MOUNT;
                }
            }
        }
        let cmd = &cmd;
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
        let (pos, side, gun_id, nat) = (s.ps.pos, s.ps.side, s.ps.gun_id(), s.ps.nation);
        if out.shot.is_some() || out.throw.is_some() || out.melee.is_some() {
            if let Some(s) = self.slots[id].as_mut() {
                s.fresh = 0.0;
            }
        }
        if let Some((eye, dir)) = out.shot {
            self.recent_shots.push((id as u8, eye, self.tick));
            self.fire(id, eye, dir, cmd.view_tick, gun_id);
        }
        if let Some((p, v)) = out.throw {
            let gid = self.next_grenade;
            self.next_grenade = self.next_grenade.wrapping_add(1).max(1);
            let fuse = if out.smoke { 1.6 } else { 3.0 };
            self.grenades.push(Grenade { id: gid, pos: p, vel: v, fuse, owner: id as u8, side, bounce_cd: 0.0, nation: nat, smoke: out.smoke });
            self.push(Ev::Throw { who: id as u8, pos: p });
        }
        if let Some((eye, dir)) = out.melee {
            self.melee(id, eye, dir);
        }
        if out.reload {
            self.push(Ev::Reload { who: id as u8, pos });
        }
        if out.ping {
            self.push(Ev::Ping { who: id as u8, pos });
        }
    }

    /// Culatazo: da al enemigo más cercano que esté delante y a mano.
    fn melee(&mut self, id: usize, eye: V3, dir: V3) {
        let my_team = self.slots[id].as_ref().unwrap().team;
        let teams = self.mode.teams();
        let flat = dir.flat().norm();
        let mut best: Option<(usize, f32)> = None;
        for (i, s) in self.slots.iter().enumerate() {
            let Some(s) = s else { continue };
            if i == id || !s.alive || (teams && s.team == my_team) {
                continue;
            }
            let to = s.ps.pos + V3::UP * 1.0 - (eye - V3::UP * 0.6);
            let d = to.flat().len();
            if d > MELEE_RANGE || to.y.abs() > 1.4 {
                continue;
            }
            if to.flat().norm().dot(flat) < 0.35 && d > 0.7 {
                continue;
            }
            if !self.world.los(eye, s.ps.pos + V3::UP * 1.1) {
                continue;
            }
            if best.map(|b| d < b.1).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        let pos = eye + dir * 0.8;
        self.push(Ev::Melee { who: id as u8, pos, hit: best.is_some() });
        if let Some((victim, _)) = best {
            self.push(Ev::Hit { shooter: id as u8 });
            self.damage(victim, MELEE_DAMAGE, Some(id), weapon::BASH, false, eye);
        }
    }

    /// Recoge una granada que haya a los pies y la lanza hacia donde se mira.
    fn return_grenade(&mut self, id: usize) -> bool {
        let s = self.slots[id].as_ref().unwrap();
        if s.zombie || s.ps.busy || s.ps.throw_t > 0.0 {
            return false;
        }
        let (pos, eye, f, side, vel) = (s.ps.pos, s.ps.eye(), s.ps.forward(), s.ps.side, s.ps.vel);
        let right = s.ps.right();
        let Some(g) = self.grenades.iter_mut().filter(|g| !g.smoke && g.fuse > 0.25 && g.pos.dist_flat(pos) < 1.7 && (g.pos.y - pos.y).abs() < 1.6).min_by(|a, b| {
            a.pos.dist(pos).partial_cmp(&b.pos.dist(pos)).unwrap_or(std::cmp::Ordering::Equal)
        }) else {
            return false;
        };
        g.owner = id as u8;
        g.side = side;
        g.pos = eye + f * 0.4 + right * 0.2;
        g.vel = f * 15.0 + V3::UP * 2.5 + vel.flat() * 0.5;
        let p = g.pos;
        if let Some(s) = self.slots[id].as_mut() {
            s.ps.throw_t = 0.8;
        }
        self.push(Ev::Throw { who: id as u8, pos: p });
        true
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
        let def = crate::sim::gun(gun);
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
        let dmg = if v.ps.perk == perk::TOUGH && (weapon as usize) < GUNS.len() { dmg * 0.8 } else { dmg };
        v.health -= dmg;
        v.since_hit = 0.0;
        v.fresh = 0.0;
        self.push(Ev::Hurt { victim: victim as u8, from });
        if self.slots[victim].as_ref().unwrap().health <= 0.0 {
            self.kill(victim, attacker, weapon, head);
        }
    }

    fn respawn_delay(&self, team: u8) -> f32 {
        match (self.phase, self.mode) {
            (Phase::Warmup, _) => 2.0,
            (_, Mode::Ctf) => 5.0,
            // Quien tiene el cuartel tarda más en volver
            (_, Mode::Hq) if self.hq_state == hq_state::HELD && self.hq_owner == team => 10.0,
            _ => 3.0,
        }
    }

    fn give_xp(&mut self, who: usize, amount: u16, reason: u8) {
        if self.slots.get(who).and_then(|s| s.as_ref()).map(|s| !s.is_bot()).unwrap_or(false) {
            self.push(Ev::Xp { who: who as u8, amount, reason });
        }
    }

    /// Deja en el suelo un arma con su munición.
    fn drop_gun(&mut self, pos: V3, yaw: f32, gun: u8, mag: u32, reserve: u32) {
        if matches!(crate::sim::gun(gun).kind, GunKind::Mounted | GunKind::Melee) || mag + reserve == 0 {
            return;
        }
        let id = self.next_drop;
        self.next_drop = self.next_drop.wrapping_add(1).max(1);
        let p = v3(pos.x, (pos.y).max(0.0) + 0.06, pos.z);
        self.drops.push((DropInfo { id, pos: p, yaw, gun }, mag, reserve, DROP_TIME));
        while self.drops.len() > 32 {
            self.drops.remove(0);
        }
    }

    /// Munición del suelo al pasar por encima (si es de un arma que se lleva)
    /// y cambio de arma con F.
    fn update_drops(&mut self, dt: f32) {
        for d in self.drops.iter_mut() {
            d.3 -= dt;
        }
        self.drops.retain(|d| d.3 > 0.0);
        let mut k = 0;
        while k < self.drops.len() {
            let (info, mag, reserve, _) = self.drops[k];
            let mut taken = None;
            for (i, s) in self.slots.iter_mut().enumerate() {
                let Some(s) = s else { continue };
                if !s.alive || s.zombie || s.ps.pos.dist_flat(info.pos) > 1.0 || (s.ps.pos.y - info.pos.y).abs() > 1.2 {
                    continue;
                }
                if info.gun == AMMO_BOX {
                    // Caja de munición: un cargador más para cada arma
                    let mut got = false;
                    for slot in 0..2 {
                        let def = crate::sim::gun(s.ps.guns[slot]);
                        let cap = def.start_reserve * 2;
                        let a = &mut s.ps.ammo[slot];
                        if a.reserve < cap {
                            a.reserve = (a.reserve + def.mag * 2).min(cap);
                            got = true;
                        }
                    }
                    if s.ps.grenades < 3 {
                        s.ps.grenades += 1;
                        got = true;
                    }
                    if got {
                        taken = Some(i);
                        break;
                    }
                    continue;
                }
                for slot in 0..2 {
                    if s.ps.guns[slot] != info.gun {
                        continue;
                    }
                    let def = crate::sim::gun(info.gun);
                    let cap = def.start_reserve * 2;
                    let a = &mut s.ps.ammo[slot];
                    if a.reserve < cap {
                        a.reserve = (a.reserve + mag + reserve).min(cap);
                        taken = Some(i);
                    }
                    break;
                }
                if taken.is_some() {
                    break;
                }
            }
            if let Some(i) = taken {
                self.push(Ev::Pickup { who: i as u8, pos: info.pos, gun: info.gun, weapon: false });
                self.drops.remove(k);
            } else {
                k += 1;
            }
        }
    }

    /// Cambia el arma en la mano por la del suelo más cercana.
    fn pickup_weapon(&mut self, id: usize) -> bool {
        let Some(s) = self.slots[id].as_ref() else { return false };
        if s.zombie || s.ps.busy || s.ps.is_mounted() {
            return false;
        }
        let pos = s.ps.pos;
        let Some(k) = self
            .drops
            .iter()
            .enumerate()
            .filter(|(_, d)| d.0.pos.dist_flat(pos) < 1.4 && d.0.gun != AMMO_BOX && !s.ps.guns.contains(&d.0.gun))
            .min_by(|a, b| a.1 .0.pos.dist_flat(pos).partial_cmp(&b.1 .0.pos.dist_flat(pos)).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, _)| k)
        else {
            return false;
        };
        let (info, mag, reserve, _) = self.drops.remove(k);
        let s = self.slots[id].as_mut().unwrap();
        let slot = (s.ps.slot as usize).min(1);
        let old = (s.ps.guns[slot], s.ps.ammo[slot]);
        s.ps.guns[slot] = info.gun;
        s.ps.ammo[slot] = Ammo { mag, reserve };
        s.ps.reload_t = 0.0;
        s.ps.switch_t = 0.5;
        let (p, yaw) = (s.ps.pos, s.ps.yaw);
        self.drop_gun(p + V3::from_yaw(yaw) * 0.3, yaw, old.0, old.1.mag, old.1.reserve);
        self.push(Ev::Pickup { who: id as u8, pos: info.pos, gun: info.gun, weapon: true });
        true
    }

    fn kill(&mut self, victim: usize, attacker: Option<usize>, weapon: u8, head: bool) {
        self.drop_flag(victim);
        // El caído deja el arma que llevaba en la mano, con su munición
        if let Some(v) = self.slots[victim].as_ref() {
            if !v.zombie && v.alive {
                let slot = (v.ps.slot as usize).min(1);
                let (pos, yaw, g, a) = (v.ps.pos, v.ps.yaw, v.ps.guns[slot], v.ps.ammo[slot]);
                self.drop_gun(pos, yaw, g, a.mag, a.reserve);
            } else if v.zombie && v.alive {
                // Los zombis dejan a veces una caja de munición
                let (pos, yaw) = (v.ps.pos, v.ps.yaw);
                if self.rng.chance(0.45) {
                    let id = self.next_drop;
                    self.next_drop = self.next_drop.wrapping_add(1).max(1);
                    self.drops.push((DropInfo { id, pos: v3(pos.x, pos.y.max(0.0) + 0.06, pos.z), yaw, gun: AMMO_BOX }, 0, 0, DROP_TIME));
                    while self.drops.len() > 32 {
                        self.drops.remove(0);
                    }
                }
            }
        }
        let counting = self.phase == Phase::Playing;
        let vt = self.slots[victim].as_ref().map(|v| v.team).unwrap_or(0);
        let delay = self.respawn_delay(vt);
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
                if kteam != vteam || !self.mode.teams() {
                    self.give_xp(a, xp::KILL, xp_reason::KILL);
                    if head {
                        self.give_xp(a, xp::HEADSHOT, xp_reason::HEADSHOT);
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
        let nat = self.nation_of(side);
        let zhealth = 60.0 + 15.0 * self.wave as f32;
        let s = self.slots[id].as_mut().unwrap();
        s.ps = PState::equip(pos, yaw, side, if s.zombie { nation::ZOMBIE } else { nat }, &s.loadout);
        s.alive = true;
        s.fresh = 12.0;
        s.health = if s.zombie { zhealth } else { MAX_HEALTH };
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
        if self.slots[id].as_ref().map(|s| s.zombie).unwrap_or(false) {
            return self.zombie_spawn(id);
        }
        let fixed: Option<&Vec<V3>> = match self.mode {
            Mode::Sd => Some(if team == self.attackers() { &self.world.axis_spawns } else { &self.world.allied_spawns }),
            Mode::Ctf => Some(if team == ALLIES { &self.world.allied_spawns } else { &self.world.axis_spawns }),
            Mode::Zombies => Some(&self.world.allied_spawns),
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
        // Los bots se vuelven a crear para el modo nuevo
        for i in 0..self.slots.len() {
            if self.slots[i].as_ref().map(|s| s.is_bot()).unwrap_or(false) {
                self.slots[i] = None;
            }
        }
        self.hq_state = hq_state::NONE;
        self.hq_progress = 0.0;
        self.hq_timer = 3.0;
        self.wave = 0;
        self.zombies_to_spawn = 0;
        self.wave_break = 0.0;
        self.smokes.clear();
        self.drops.clear();
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
            let team = if self.mode == Mode::Zombies { ALLIES } else if self.mode.teams() { (k % 2) as u8 } else { 0 };
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
        if self.mode == Mode::Zombies {
            return self.human_count() > 0;
        }
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
        self.hq_state = hq_state::NONE;
        self.hq_timer = 3.0;
        if self.mode == Mode::Zombies {
            self.wave = 0;
            self.phase_t = 0.0;
            let ids: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].as_ref().map(|s| !s.zombie && s.ready).unwrap_or(false)).collect();
            for i in ids {
                self.spawn(i);
            }
            self.start_wave();
        } else if self.mode == Mode::Sd {
            self.start_round();
        } else {
            self.phase_t = self.match_seconds();
            let ids: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].as_ref().map(|s| s.ready).unwrap_or(false)).collect();
            for i in ids {
                self.spawn(i);
            }
            self.announce(format!("¡Empieza la partida! {}", self.mode.name()), tone::ALERT);
        }
    }

    fn start_round(&mut self) {
        self.round += 1;
        self.drops.clear();
        if self.round == 4 {
            self.announce("Descanso: los equipos cambian de papel", tone::NEUTRAL);
        }
        self.phase = Phase::Playing;
        self.phase_t = SD_ROUND_TIME;
        self.bomb_state = bomb_state::NONE;
        self.bomb_timer = 0.0;
        self.grenades.clear();
        let ids: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].as_ref().map(|s| s.ready).unwrap_or(false)).collect();
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
            Mode::Tdm | Mode::Ctf | Mode::Hq => self.team_scores.iter().any(|&t| t as i32 >= limit),
            Mode::Sd | Mode::Zombies => false,
        };
        if done {
            self.end_match();
        }
    }

    fn end_match(&mut self) {
        // Experiencia por jugar y por ganar
        let winner: Option<u8> = match self.mode {
            Mode::Dm | Mode::Zombies => None,
            _ => match self.team_scores[0].cmp(&self.team_scores[1]) {
                std::cmp::Ordering::Greater => Some(ALLIES),
                std::cmp::Ordering::Less => Some(AXIS),
                _ => None,
            },
        };
        let best_dm = if self.mode == Mode::Dm { self.slots.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|s| (i, s.score, s.kills))).max_by_key(|x| (x.1, x.2)).map(|x| x.0) } else { None };
        for i in 0..self.slots.len() {
            let Some(s) = self.slots[i].as_ref() else { continue };
            let team = s.team;
            self.give_xp(i, xp::MATCH, xp_reason::MATCH);
            if winner == Some(team) || best_dm == Some(i) {
                self.give_xp(i, xp::WIN, xp_reason::WIN);
            }
        }
        let text = match self.mode {
            Mode::Zombies => format!("Fin de la partida: habéis resistido {} oleadas", self.wave.saturating_sub(1)),
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
                    Mode::Hq => {
                        self.update_hq(dt);
                        self.tick_match_clock(dt);
                    }
                    Mode::Zombies => self.update_zombies(dt),
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
                self.give_xp(id, xp::OBJECTIVE / 2, xp_reason::OBJECTIVE);
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
                self.give_xp(id, xp::OBJECTIVE, xp_reason::OBJECTIVE);
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
            self.give_xp(id, xp::OBJECTIVE, xp_reason::OBJECTIVE);
            let s = self.slots[id].as_mut().unwrap();
            s.score += 3;
            let name = s.name.clone();
            let letter = if site == 0 { "A" } else { "B" };
            self.announce(format!("¡{name} ha colocado la bomba en el objetivo {letter}!"), tone::ALERT);
        }
        if let Some(id) = defused_by {
            self.bomb_state = bomb_state::DEFUSED;
            self.give_xp(id, xp::OBJECTIVE, xp_reason::OBJECTIVE);
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
                if g.smoke {
                    self.smokes.push((g.pos, 0.0));
                    self.events.push_back((self.next_event, Ev::Smoke { pos: g.pos }));
                    self.next_event += 1;
                } else {
                    booms.push((g.pos, g.owner, g.side));
                }
            }
        }
        self.grenades.retain(|g| g.fuse > 0.0);
        for s in self.smokes.iter_mut() {
            s.1 += dt;
        }
        self.smokes.retain(|s| s.1 < SMOKE_TIME);
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
            .filter_map(|(i, s)| s.as_ref().filter(|s| !s.is_bot() && now.duration_since(s.last_recv).as_secs_f32() > TIMEOUT_SECS).map(|_| i))
            .collect();
        for i in timed_out {
            self.remove(i, "ha perdido la conexión");
        }

        let can_respawn = matches!(self.phase, Phase::Warmup | Phase::Playing) && (!matches!(self.mode, Mode::Sd | Mode::Zombies) || self.phase == Phase::Warmup);
        let mut to_spawn = Vec::new();
        for (i, s) in self.slots.iter_mut().enumerate() {
            let Some(s) = s else { continue };
            if s.zombie {
                continue;
            }
            s.budget = (s.budget + 1.0).min(10.0);
            if !s.ready {
                // Todavía eligiendo armas: no aparece
                s.respawn_t = s.respawn_t.min(0.3);
                continue;
            }
            if s.alive {
                s.fresh = (s.fresh - dt).max(0.0);
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

        self.bot_check -= dt;
        if self.bot_check <= 0.0 {
            self.bot_check = 1.0;
            self.manage_bots();
        }
        self.run_bots();
        self.recent_shots.retain(|s| tick_age(s.2, self.tick) < TICK_HZ);

        self.update_grenades(dt);
        self.update_drops(dt);
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
                    gun: s.ps.gun_id(),
                    nation: s.ps.nation,
                    level: if s.is_bot() { 0 } else { s.level },
                    bot: s.is_bot(),
                    mounted: if s.alive { s.ps.mounted } else { NO_TURRET },
                    melee: s.ps.melee_t > 0.45,
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
        let grenades: Vec<GrenadeInfo> = self.grenades.iter().map(|g| GrenadeInfo { id: g.id, pos: g.pos, side: g.side, nation: g.nation, smoke: g.smoke }).collect();
        for s in self.slots.iter().flatten().filter(|s| !s.is_bot()) {
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
                hq_state: self.hq_state,
                hq_owner: self.hq_owner,
                hq_pos: self.hq_pos,
                hq_progress: self.hq_progress,
                allied_nation: self.cfg.allied_nation,
                wave: self.wave,
                zombies_left: self.zombies_left(),
                progress: s.progress,
                my_team: s.team,
                my_alive: s.alive,
                respawn_in: if s.alive { 0.0 } else { s.respawn_t.max(0.0) },
                health: s.health.max(0.0),
                me: if s.alive { Some(s.ps.clone()) } else { None },
                players: players.clone(),
                grenades: grenades.clone(),
                smokes: self.smokes.clone(),
                drops: self.drops.iter().map(|d| d.0).collect(),
                events,
            };
            let mut w = W::new(s2c::SNAPSHOT);
            snap.write(&mut w);
            let _ = self.sock.send_to(&w.0, s.addr);
        }
    }
}

const BOMB_W: u8 = weapon::BOMB;

fn tick_age(t: u32, now: u32) -> u32 {
    now.saturating_sub(t)
}

/// Sitios posibles del Cuartel General: puntos de paso repartidos por el
/// centro del mapa, lejos de las zonas de reaparición.
fn hq_spots(w: &World) -> Vec<V3> {
    let mean = |v: &Vec<V3>| v.iter().fold(V3::ZERO, |a, b| a + *b) / v.len().max(1) as f32;
    let (a, b) = (mean(&w.allied_spawns), mean(&w.axis_spawns));
    let mut cands: Vec<V3> = w.waypoints.iter().copied().filter(|p| p.dist_flat(a) > 22.0 && p.dist_flat(b) > 22.0 && w.body_free(*p, 1.0, 1.8)).collect();
    cands.extend(w.bomb_sites.iter());
    let mut out: Vec<V3> = Vec::new();
    // Elegir puntos separados entre sí (al menos 18 m)
    while let Some(i) = cands.iter().position(|c| out.iter().all(|o| o.dist_flat(*c) > 18.0)) {
        out.push(cands.remove(i));
        if out.len() >= 8 {
            break;
        }
    }
    if out.is_empty() {
        out.push((a + b) * 0.5);
    }
    out
}

const BOT_NAMES: [[&str; 12]; 5] = [
    ["Miller", "Jackson", "O'Brien", "Kowalski", "Russo", "Walker", "Hayes", "Novak", "Price", "Murphy", "Carter", "Hughes"],
    ["Davies", "MacGregor", "Evans", "Thompson", "Wright", "Price", "Fletcher", "Hall", "Baker", "Cole", "Turner", "Ward"],
    ["Ivanov", "Petrov", "Smirnov", "Volkov", "Sokolov", "Popov", "Kuznetsov", "Morozov", "Lebedev", "Orlov", "Zaitsev", "Belov"],
    ["Weber", "Fischer", "Schmidt", "Becker", "Hoffmann", "Krüger", "Wolf", "Braun", "Keller", "Lang", "Vogel", "Frank"],
    ["Zombi", "Caminante", "Podrido", "Hambriento", "Gruñón", "Sin nombre", "Mordedor", "Errante", "Pálido", "Lento", "Hueco", "Frío"],
];

impl Server {
    // ------------------------------------------------------------------
    // Bots
    // ------------------------------------------------------------------

    fn bot_name(&mut self, nat: u8) -> String {
        let list = BOT_NAMES[(nat as usize).min(4)];
        for _ in 0..30 {
            let base = list[self.rng.index(list.len())];
            let name = if nat == nation::ZOMBIE { base.to_string() } else { format!("{base} [bot]") };
            if !self.slots.iter().flatten().any(|s| s.name == name) {
                return name;
            }
        }
        format!("Bot {}", self.rng.index(1000))
    }

    pub fn add_bot(&mut self, team: u8, zombie: bool) -> Option<usize> {
        let id = self.slots.iter().position(|s| s.is_none())?;
        let side = if zombie { AXIS } else { self.side_of(id, team) };
        let nat = if zombie { nation::ZOMBIE } else { self.nation_of(side) };
        let name = self.bot_name(nat);
        let skill = (self.cfg.bot_skill + self.rng.range(-0.15, 0.15)).clamp(0.0, 1.0);
        let class = self.rng.index(CLASSES.len()) as u8;
        // Pocos francotiradores: cambian a fusilero la mitad de las veces
        let class = if class == 3 && self.rng.chance(0.5) { 0 } else { class };
        let perk_id = self.rng.index(perk::COUNT as usize) as u8;
        let opts = Loadout::options(class, nat);
        let primary = if opts.is_empty() { 255 } else { opts[self.rng.index(opts.len())] };
        let brain = Brain::new(skill, zombie, (self.rng.f() * 4.0e9) as u64 + id as u64);
        self.slots[id] = Some(Slot {
            addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            name,
            team: if zombie { AXIS } else { team },
            ps: PState::new(V3::ZERO, 0.0, side),
            alive: false,
            health: MAX_HEALTH,
            since_hit: 99.0,
            respawn_t: 0.5 + self.rng.range(0.0, 1.0),
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
            loadout: Loadout { class, primary, perk: perk_id, secondary: 255 },
            level: 1,
            bot: Some(Box::new(brain)),
            zombie,
            melee_cd: 0.0,
            fresh: 0.0,
            ready: true,
        });
        Some(id)
    }

    /// Añade o quita bots para llegar al número configurado.
    fn manage_bots(&mut self) {
        if self.mode == Mode::Zombies {
            // Los soldados bots acompañan a los humanos; los zombis van aparte
            let soldiers = self.slots.iter().flatten().filter(|s| s.is_bot() && !s.zombie).count();
            let humans = self.human_count();
            let want = self.cfg.bots.min(4).saturating_sub(humans);
            if soldiers < want {
                self.add_bot(ALLIES, false);
            } else if soldiers > want {
                if let Some(i) = self.slots.iter().position(|s| s.as_ref().map(|s| s.is_bot() && !s.zombie).unwrap_or(false)) {
                    self.remove_quiet(i);
                }
            }
            return;
        }
        let target = self.cfg.bots.min(self.cfg.max_players);
        let total = self.player_count();
        if total < target {
            let team = if self.mode.teams() {
                let (a, b) = (self.team_count(ALLIES), self.team_count(AXIS));
                if a <= b { ALLIES } else { AXIS }
            } else {
                0
            };
            self.add_bot(team, false);
        } else if total > target {
            // Quitar un bot del equipo más numeroso
            let (a, b) = (self.team_count(ALLIES), self.team_count(AXIS));
            let from = if a >= b { ALLIES } else { AXIS };
            let pick = self
                .slots
                .iter()
                .position(|s| s.as_ref().map(|s| s.is_bot() && (!self.mode.teams() || s.team == from)).unwrap_or(false))
                .or_else(|| self.slots.iter().position(|s| s.as_ref().map(|s| s.is_bot()).unwrap_or(false)));
            if let Some(i) = pick {
                self.remove_quiet(i);
            }
        } else if self.mode.teams() {
            // Reequilibrar con bots si un equipo tiene dos más que el otro
            let (a, b) = (self.team_count(ALLIES), self.team_count(AXIS));
            if a.abs_diff(b) >= 2 {
                let from = if a > b { ALLIES } else { AXIS };
                if let Some(i) = self.slots.iter().position(|s| s.as_ref().map(|s| s.is_bot() && s.team == from).unwrap_or(false)) {
                    self.remove_quiet(i);
                    self.add_bot(1 - from, false);
                }
            }
        }
    }

    fn bot_view(&self) -> BotView {
        let players = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let s = s.as_ref()?;
                Some(PView {
                    id: i as u8,
                    team: s.team,
                    alive: s.alive,
                    pos: s.ps.pos,
                    eye: s.ps.eye(),
                    crouch: s.ps.crouch,
                    zombie: s.zombie,
                    health: s.health,
                    carrying: self.mode == Mode::Ctf && self.flags.iter().any(|f| f.state == flag_state::CARRIED && f.carrier as usize == i),
                    moving: s.ps.moving,
                })
            })
            .collect();
        BotView {
            mode: self.mode,
            phase: self.phase,
            teams: self.mode.teams(),
            players,
            grenades: self.grenades.iter().map(|g| (g.pos, g.fuse, self.slots.get(g.owner as usize).and_then(|s| s.as_ref()).map(|s| s.team).unwrap_or(9))).collect(),
            shots: self.recent_shots.iter().map(|s| (s.0, s.1)).collect(),
            flags: self.flags,
            flag_bases: self.world.flag_bases,
            bomb_state: self.bomb_state,
            bomb_site: self.bomb_site,
            bomb_sites: self.world.bomb_sites,
            attackers: self.attackers(),
            hq_state: self.hq_state,
            hq_pos: self.hq_pos,
            hq_owner: self.hq_owner,
            smokes: self.smokes.iter().map(|s| s.0).collect(),
        }
    }

    fn run_bots(&mut self) {
        let ids: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].as_ref().map(|s| s.is_bot()).unwrap_or(false)).collect();
        if ids.is_empty() {
            return;
        }
        let view = self.bot_view();
        let tick = self.tick;
        for id in ids {
            let (cmd, say) = {
                let s = self.slots[id].as_mut().unwrap();
                let mut brain = s.bot.take().unwrap();
                let cmd = brain.think(id as u8, &view, &self.world, &s.ps, s.alive, s.health, tick);
                let say = brain.say.take();
                s.bot = Some(brain);
                (cmd, say)
            };
            if let Some(text) = say {
                let s = self.slots[id].as_ref().unwrap();
                if s.alive && !s.zombie {
                    let name = s.name.clone();
                    self.push(Ev::Chat { from: id as u8, team_only: self.mode.teams(), name, text });
                }
            }
            let s = self.slots[id].as_mut().unwrap();
            s.last_seq = cmd.seq;
            s.last_recv = Instant::now();
            self.apply_cmd(id, &cmd);
        }
    }

    // ------------------------------------------------------------------
    // Cuartel General
    // ------------------------------------------------------------------

    fn update_hq(&mut self, dt: f32) {
        let inside = |s: &Server, t: u8| {
            s.slots.iter().flatten().filter(|p| p.alive && p.team == t && p.ps.pos.dist_flat(s.hq_pos) < HQ_RADIUS && (p.ps.pos.y - s.hq_pos.y).abs() < 2.5).count()
        };
        match self.hq_state {
            hq_state::NONE => {
                self.hq_timer -= dt;
                if self.hq_timer <= 0.0 {
                    let n = self.hq_spots.len();
                    let mut k = self.rng.index(n);
                    if n > 1 && k == self.hq_idx {
                        k = (k + 1) % n;
                    }
                    self.hq_idx = k;
                    self.hq_pos = self.hq_spots[k];
                    self.hq_state = hq_state::NEUTRAL;
                    self.hq_progress = 0.0;
                    self.announce("¡Ha aparecido un cuartel general! Tomadlo", tone::ALERT);
                    self.push(Ev::Whistle);
                }
            }
            hq_state::NEUTRAL => {
                let (a, b) = (inside(self, ALLIES), inside(self, AXIS));
                let who = match (a > 0, b > 0) {
                    (true, false) => Some(ALLIES),
                    (false, true) => Some(AXIS),
                    _ => None,
                };
                match who {
                    Some(t) => {
                        if t != self.hq_capturer {
                            self.hq_capturer = t;
                            self.hq_progress = 0.0;
                        }
                        let n = if t == ALLIES { a } else { b };
                        self.hq_progress += dt / HQ_CAPTURE * (1.0 + 0.25 * (n as f32 - 1.0)).min(2.0);
                        if self.hq_progress >= 1.0 {
                            self.hq_state = hq_state::HELD;
                            self.hq_owner = t;
                            self.hq_progress = 0.0;
                            self.hq_timer = HQ_HOLD;
                            self.hq_point_t = 0.0;
                            let ids: Vec<usize> = (0..self.slots.len())
                                .filter(|&i| self.slots[i].as_ref().map(|p| p.alive && p.team == t && p.ps.pos.dist_flat(self.hq_pos) < HQ_RADIUS).unwrap_or(false))
                                .collect();
                            for i in ids {
                                self.give_xp(i, xp::OBJECTIVE, xp_reason::OBJECTIVE);
                                self.slots[i].as_mut().unwrap().score += 5;
                            }
                            let tone = if t == ALLIES { tone::ALLIES } else { tone::AXIS };
                            self.announce(format!("Los {} han tomado el cuartel general", team_name(t)), tone);
                        }
                    }
                    None if a == 0 && b == 0 => self.hq_progress = (self.hq_progress - dt / HQ_CAPTURE * 0.5).max(0.0),
                    None => {}
                }
            }
            _ => {
                let owner = self.hq_owner;
                self.hq_point_t += dt;
                while self.hq_point_t >= HQ_POINT_EVERY {
                    self.hq_point_t -= HQ_POINT_EVERY;
                    self.team_scores[owner as usize] += 1;
                }
                let (mine, theirs) = (inside(self, owner), inside(self, 1 - owner));
                if theirs > 0 && mine == 0 {
                    self.hq_progress += dt / HQ_DESTROY;
                } else {
                    self.hq_progress = (self.hq_progress - dt / HQ_DESTROY * 0.5).max(0.0);
                }
                self.hq_timer -= dt;
                if self.hq_progress >= 1.0 {
                    let enemy = 1 - owner;
                    let ids: Vec<usize> = (0..self.slots.len())
                        .filter(|&i| self.slots[i].as_ref().map(|p| p.alive && p.team == enemy && p.ps.pos.dist_flat(self.hq_pos) < HQ_RADIUS).unwrap_or(false))
                        .collect();
                    for i in ids {
                        self.give_xp(i, xp::OBJECTIVE, xp_reason::OBJECTIVE);
                        self.slots[i].as_mut().unwrap().score += 5;
                    }
                    self.hq_state = hq_state::NONE;
                    self.hq_timer = 6.0;
                    self.hq_progress = 0.0;
                    let tone = if enemy == ALLIES { tone::ALLIES } else { tone::AXIS };
                    self.announce(format!("¡Los {} han destruido el cuartel general enemigo!", team_name(enemy)), tone);
                    self.push(Ev::Explosion { pos: self.hq_pos + V3::UP * 0.5, big: false });
                } else if self.hq_timer <= 0.0 {
                    self.hq_state = hq_state::NONE;
                    self.hq_timer = 6.0;
                    self.hq_progress = 0.0;
                    self.announce("El cuartel general se traslada a otro sitio", tone::NEUTRAL);
                }
                self.check_score_end();
            }
        }
    }

    // ------------------------------------------------------------------
    // Zombis
    // ------------------------------------------------------------------

    fn zombies_left(&self) -> u16 {
        if self.mode != Mode::Zombies {
            return 0;
        }
        self.zombies_to_spawn + self.slots.iter().flatten().filter(|s| s.zombie && s.alive).count() as u16
    }

    fn start_wave(&mut self) {
        self.wave += 1;
        let humans = self.slots.iter().flatten().filter(|s| !s.zombie).count() as u16;
        self.zombies_to_spawn = 4 + self.wave * 3 + humans.saturating_sub(1) * 2;
        self.wave_break = 0.0;
        // Revivir a los caídos
        let ids: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].as_ref().map(|s| !s.zombie && !s.alive && s.ready).unwrap_or(false)).collect();
        for i in ids {
            self.spawn(i);
        }
        self.announce(format!("Oleada {}: ¡ya vienen!", self.wave), tone::ALERT);
        self.push(Ev::Whistle);
    }

    fn zombie_spawn(&mut self, _id: usize) -> (V3, f32) {
        // Lejos de los vivos y fuera de su vista
        let humans: Vec<V3> = self.slots.iter().flatten().filter(|s| !s.zombie && s.alive).map(|s| s.ps.eye()).collect();
        let mut cands: Vec<V3> = self.world.waypoints.clone();
        cands.extend(self.world.axis_spawns.iter());
        let mut best = (cands[0], f32::MIN);
        for c in cands {
            let eye = c + V3::UP * 1.5;
            let d = humans.iter().map(|h| h.dist(eye)).fold(200.0f32, f32::min);
            let seen = humans.iter().any(|h| h.dist(eye) < 50.0 && self.world.los(*h, eye));
            let score = if d < 14.0 { -100.0 } else { 50.0 - (d - 30.0).abs() } - if seen { 40.0 } else { 0.0 } + self.rng.range(0.0, 15.0);
            if score > best.1 {
                best = (c, score);
            }
        }
        let face = humans.first().copied().unwrap_or(V3::ZERO);
        (best.0, (face - best.0).yaw())
    }

    fn update_zombies(&mut self, dt: f32) {
        // Ataques cuerpo a cuerpo
        let humans: Vec<(usize, V3)> = (0..self.slots.len())
            .filter_map(|i| self.slots[i].as_ref().filter(|s| !s.zombie && s.alive).map(|s| (i, s.ps.pos)))
            .collect();
        let zombies: Vec<usize> = (0..self.slots.len()).filter(|&i| self.slots[i].as_ref().map(|s| s.zombie).unwrap_or(false)).collect();
        let dmg = 28.0 + 2.0 * self.wave as f32;
        for &z in &zombies {
            let s = self.slots[z].as_mut().unwrap();
            s.melee_cd -= dt;
            if !s.alive || s.melee_cd > 0.0 {
                continue;
            }
            let (zp, zf) = (s.ps.pos, V3::from_yaw(s.ps.yaw));
            if let Some(&(h, hp)) = humans.iter().find(|(_, hp)| hp.dist_flat(zp) < 1.35 && (hp.y - zp.y).abs() < 1.2 && (*hp - zp).flat().norm().dot(zf) > 0.2) {
                s.melee_cd = 1.1;
                self.push(Ev::Growl { who: z as u8, pos: zp, attack: true });
                self.damage(h, dmg, Some(z), weapon::MELEE, false, zp + V3::UP);
                let _ = hp;
            } else if self.rng.chance(0.002) {
                self.push(Ev::Growl { who: z as u8, pos: zp, attack: false });
            }
        }
        // Retirar zombis caídos y hacer salir nuevos
        for &z in &zombies {
            let s = self.slots[z].as_ref().unwrap();
            if !s.alive && s.respawn_t < -2.0 {
                self.slots[z] = None;
            } else if !s.alive {
                self.slots[z].as_mut().unwrap().respawn_t -= dt;
            }
        }
        if self.wave_break > 0.0 {
            self.wave_break -= dt;
            self.phase_t = self.wave_break.max(0.0);
            if self.wave_break <= 0.0 {
                self.start_wave();
            }
            return;
        }
        let alive_z = self.slots.iter().flatten().filter(|s| s.zombie && s.alive).count();
        let cap = (8 + self.wave as usize).min(14);
        if self.zombies_to_spawn > 0 && alive_z < cap && self.rng.chance(0.04) {
            if let Some(id) = self.add_bot(AXIS, true) {
                self.zombies_to_spawn -= 1;
                self.spawn(id);
                // Más rápidos en cada oleada
                let wave = self.wave;
                if let Some(b) = self.slots[id].as_mut().and_then(|s| s.bot.as_mut()) {
                    b.zombie_speed = (0.5 + 0.06 * wave as f32).min(1.0);
                    b.zombie_sprint = wave >= 5;
                }
            }
        }
        let living = self.slots.iter().flatten().filter(|s| !s.zombie && s.alive).count();
        if living == 0 {
            self.team_scores[0] = self.wave.saturating_sub(1);
            self.end_match();
            return;
        }
        if self.zombies_to_spawn == 0 && self.zombies_left() == 0 {
            self.team_scores[0] = self.wave;
            for i in 0..self.slots.len() {
                if self.slots[i].as_ref().map(|s| !s.zombie).unwrap_or(false) {
                    self.give_xp(i, xp::WAVE, xp_reason::WAVE);
                }
            }
            self.announce(format!("¡Oleada {} superada! La siguiente llega en 10 segundos", self.wave), tone::ALLIES);
            self.wave_break = 10.0;
        }
    }
}


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

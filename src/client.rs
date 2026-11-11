//! Cliente de red: conexión, predicción del propio jugador, reconciliación
//! con el servidor e interpolación del resto. Rust puro (sin gráficos).

use crate::math::*;
use crate::net::*;
use crate::sim::*;
use crate::world::World;
use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Instant;

/// Retraso de interpolación (ticks): 6 = 100 ms, dos instantáneas.
pub const INTERP_TICKS: f64 = 6.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Connecting,
    Connected,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInput {
    pub fwd: f32,
    pub strafe: f32,
    pub look_yaw: f32,
    pub look_pitch: f32,
    pub fire: bool,
    pub fire_pressed: bool,
    pub ads: bool,
    pub jump: bool,
    pub crouch: bool,
    pub sprint: bool,
    pub reload: bool,
    pub grenade: bool,
    pub use_key: bool,
    pub slot: Option<u8>,
    pub next_slot: bool,
    /// Culatazo (flanco)
    pub melee: bool,
    /// Granada de humo (flanco)
    pub smoke: bool,
    /// Coger o soltar la ametralladora montada (flanco)
    pub mount: bool,
}

/// Efectos del propio jugador predichos localmente (sin esperar al servidor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LocalFx {
    Shot { gun: u8 },
    Reload { gun: u8 },
    Ping,
    Dry,
    Throw,
    Switch,
    Melee,
    Mount,
}

pub fn resolve(addr: &str) -> Option<SocketAddr> {
    let a = addr.trim();
    if a.is_empty() {
        return None;
    }
    let with_port = if a.contains(':') { a.to_string() } else { format!("{a}:{DEFAULT_PORT}") };
    with_port.to_socket_addrs().ok()?.find(|s| s.is_ipv4())
}

pub struct NetClient {
    sock: UdpSocket,
    pub server: SocketAddr,
    pub status: Status,
    pub my_id: u8,
    pub server_name: String,
    name: String,
    started: Instant,
    last_connect: Instant,
    last_recv: Instant,
    seq: u32,
    pending: VecDeque<Cmd>,
    pub pred: Option<PState>,
    pred_err: V3,
    pub view_yaw: f32,
    pub view_pitch: f32,
    kick_back: f32,
    pub recoil: f32,
    pub muzzle: f32,
    pub snap: Option<Snapshot>,
    buf: VecDeque<(u32, Vec<PlayerInfo>, Vec<GrenadeInfo>)>,
    clock: f64,
    have_clock: bool,
    last_event: u32,
    pub events: Vec<Ev>,
    pub local: Vec<LocalFx>,
    acc: f32,
    rng: Rng,
    replay_rng: Rng,
    edges: u16,
    pub slot: u8,
    pub team_req: u8,
    /// Equipamiento elegido y nivel (se mandan en cada paquete)
    pub loadout: Loadout,
    pub level: u8,
    chat_id: u16,
    chat_pending: Option<(bool, String, u8)>,
    pub bytes_in: usize,
    /// Huella del escenario local (se envía al conectar)
    map_id: u32,
}

impl NetClient {
    pub fn connect(server: SocketAddr, name: &str) -> std::io::Result<NetClient> {
        Self::connect_map(server, name, 0)
    }

    pub fn connect_map(server: SocketAddr, name: &str, map_id: u32) -> std::io::Result<NetClient> {
        let sock = UdpSocket::bind(("0.0.0.0", 0))?;
        sock.set_nonblocking(true)?;
        let now = Instant::now();
        let mut c = NetClient {
            sock,
            server,
            status: Status::Connecting,
            my_id: 0,
            server_name: String::new(),
            name: clean_name(name, 20),
            map_id,
            started: now,
            last_connect: now,
            last_recv: now,
            seq: 0,
            pending: VecDeque::new(),
            pred: None,
            pred_err: V3::ZERO,
            view_yaw: 0.0,
            view_pitch: 0.0,
            kick_back: 0.0,
            recoil: 0.0,
            muzzle: 0.0,
            snap: None,
            buf: VecDeque::new(),
            clock: 0.0,
            have_clock: false,
            last_event: 0,
            events: Vec::new(),
            local: Vec::new(),
            acc: 0.0,
            rng: Rng::new(now.elapsed().as_nanos() as u64 ^ 0x5eed),
            replay_rng: Rng::new(7),
            edges: 0,
            slot: 0,
            team_req: 0,
            loadout: Loadout::default(),
            level: 1,
            chat_id: 0,
            chat_pending: None,
            bytes_in: 0,
        };
        c.send_connect();
        Ok(c)
    }

    fn send(&self, w: &W) {
        let _ = self.sock.send_to(&w.0, self.server);
    }

    fn send_connect(&mut self) {
        let mut w = W::new(c2s::CONNECT);
        w.u16(PROTOCOL);
        w.str(&self.name);
        w.u32(self.map_id);
        self.send(&w);
        self.last_connect = Instant::now();
    }

    pub fn disconnect(&mut self) {
        let w = W::new(c2s::DISCONNECT);
        for _ in 0..3 {
            self.send(&w);
        }
        self.status = Status::Failed("Desconectado".into());
    }

    pub fn send_chat(&mut self, text: &str, team_only: bool) {
        let t = text.trim();
        if t.is_empty() {
            return;
        }
        self.chat_id = self.chat_id.wrapping_add(1).max(1);
        self.chat_pending = Some((team_only, t.chars().take(100).collect(), 8));
    }

    pub fn render_tick(&self) -> f64 {
        self.clock - INTERP_TICKS
    }

    pub fn alive(&self) -> bool {
        self.pred.is_some()
    }

    /// Ojo de la cámara del jugador local (con corrección suavizada).
    pub fn eye(&self) -> Option<V3> {
        self.pred.as_ref().map(|p| p.eye() + self.pred_err)
    }

    pub fn me_info(&self) -> Option<&PlayerInfo> {
        self.snap.as_ref()?.players.iter().find(|p| p.id == self.my_id)
    }

    fn recv(&mut self, world: &World) {
        let mut buf = [0u8; 2048];
        for _ in 0..512 {
            match self.sock.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if from != self.server {
                        continue;
                    }
                    self.bytes_in += n;
                    self.last_recv = Instant::now();
                    let data = buf[..n].to_vec();
                    self.handle(&data, world);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => break,
                Err(_) => continue,
            }
        }
    }

    fn handle(&mut self, data: &[u8], world: &World) {
        let Some((kind, mut r)) = R::open(data) else { return };
        match kind {
            s2c::WELCOME => {
                if self.status == Status::Connecting {
                    let (Some(id), Some(tick), Some(name)) = (r.u8(), r.u32(), r.str()) else { return };
                    self.my_id = id;
                    self.server_name = name;
                    self.clock = tick as f64;
                    self.have_clock = true;
                    self.status = Status::Connected;
                }
            }
            s2c::REJECT | s2c::KICKED => {
                let why = r.str().unwrap_or_else(|| "Rechazado por el servidor".into());
                self.status = Status::Failed(why);
            }
            s2c::SNAPSHOT => {
                if self.status != Status::Connected {
                    return;
                }
                if let Some(s) = Snapshot::read(&mut r) {
                    self.on_snapshot(s, world);
                }
            }
            _ => {}
        }
    }

    fn on_snapshot(&mut self, mut s: Snapshot, world: &World) {
        for (id, e) in s.events.drain(..) {
            if id > self.last_event {
                self.last_event = id;
                self.events.push(e);
            }
        }
        let newest = self.snap.as_ref().map(|p| p.tick).unwrap_or(0);
        if s.tick <= newest {
            return;
        }
        // Reloj del servidor
        let t = s.tick as f64;
        if !self.have_clock || (t - self.clock).abs() > 20.0 {
            self.clock = t;
            self.have_clock = true;
        } else {
            self.clock += (t - self.clock) * 0.05;
        }
        self.buf.push_back((s.tick, s.players.clone(), s.grenades.clone()));
        while self.buf.len() > 30 {
            self.buf.pop_front();
        }
        // Reconciliación de la predicción
        self.pending.retain(|c| c.seq > s.ack);
        match (&s.me, s.my_alive) {
            (Some(ps), true) => {
                let mut p = ps.clone();
                for c in self.pending.iter() {
                    step(world, &mut p, c, &mut self.replay_rng);
                }
                match &self.pred {
                    Some(old) => {
                        let err = old.pos - p.pos;
                        self.pred_err = if err.len() < 2.0 { self.pred_err + err } else { V3::ZERO };
                    }
                    None => {
                        // Acabamos de reaparecer: mirar hacia donde el servidor indica
                        self.view_yaw = ps.yaw;
                        self.view_pitch = 0.0;
                        self.pred_err = V3::ZERO;
                        self.kick_back = 0.0;
                        self.slot = ps.slot;
                    }
                }
                self.pred = Some(p);
            }
            _ => self.pred = None,
        }
        self.snap = Some(s);
    }

    /// Avanza un fotograma: recibe, genera comandos a 60 Hz, predice y envía.
    pub fn update(&mut self, inp: &FrameInput, dt: f32, world: &World) {
        self.events.clear();
        self.local.clear();
        self.recv(world);

        match self.status {
            Status::Connecting => {
                if self.started.elapsed().as_secs_f32() > 8.0 {
                    self.status = Status::Failed("No se ha podido conectar con el servidor".into());
                } else if self.last_connect.elapsed().as_secs_f32() > 0.5 {
                    self.send_connect();
                }
                return;
            }
            Status::Failed(_) => return,
            Status::Connected => {}
        }
        if self.last_recv.elapsed().as_secs_f32() > TIMEOUT_SECS {
            self.status = Status::Failed("Se ha perdido la conexión con el servidor".into());
            return;
        }

        // Vista (la orientación la decide el cliente)
        self.view_yaw = wrap_angle(self.view_yaw + inp.look_yaw);
        self.view_pitch = (self.view_pitch + inp.look_pitch).clamp(-1.45, 1.45);
        let rec = self.kick_back * (1.0 - (-7.0 * dt).exp());
        self.view_pitch -= rec;
        self.kick_back -= rec;
        self.recoil = (self.recoil - dt * 7.0).max(0.0);
        self.muzzle = (self.muzzle - dt).max(0.0);
        self.pred_err *= (-12.0 * dt).exp();

        if inp.fire_pressed {
            self.edges |= btn::FIRE_PRESSED;
        }
        if inp.jump {
            self.edges |= btn::JUMP;
        }
        if inp.reload {
            self.edges |= btn::RELOAD;
        }
        if inp.grenade {
            self.edges |= btn::GRENADE;
        }
        if inp.melee {
            self.edges |= btn::MELEE;
        }
        if inp.smoke {
            self.edges |= btn::SMOKE;
        }
        if inp.mount {
            self.edges |= btn::MOUNT;
        }
        if let Some(s) = inp.slot {
            self.slot = s.min(1);
        }
        if inp.next_slot {
            self.slot = 1 - self.slot.min(1);
        }

        self.clock += dt as f64 * TICK_HZ as f64;
        self.acc += dt;
        let mut n = 0;
        while self.acc >= DT && n < 6 {
            self.acc -= DT;
            n += 1;
            self.seq += 1;
            let mut b = self.edges;
            self.edges = 0;
            if inp.fire {
                b |= btn::FIRE;
            }
            if inp.ads {
                b |= btn::ADS;
            }
            if inp.crouch {
                b |= btn::CROUCH;
            }
            if inp.sprint {
                b |= btn::SPRINT;
            }
            if inp.use_key {
                b |= btn::USE;
            }
            let cmd = Cmd {
                seq: self.seq,
                view_tick: self.render_tick().max(0.0) as u32,
                yaw: self.view_yaw,
                pitch: self.view_pitch,
                fwd: Cmd::axis(inp.fwd),
                strafe: Cmd::axis(inp.strafe),
                buttons: b,
                slot: self.slot,
            };
            if let Some(p) = self.pred.as_mut() {
                let gun = p.gun_id();
                let side_kick = p.gun().kick_side * (1.0 - 0.4 * p.ads);
                let out = step(world, p, &cmd, &mut self.rng);
                let gun_after = p.gun_id();
                if out.shot.is_some() {
                    self.view_pitch = (self.view_pitch + out.kick).clamp(-1.45, 1.45);
                    self.kick_back += out.kick * 0.6;
                    // Cada arma tira hacia un lado distinto
                    self.view_yaw += (self.rng.range(-1.0, 1.0) + 0.35) * side_kick;
                    self.recoil = 1.0;
                    self.muzzle = 0.05;
                    self.local.push(LocalFx::Shot { gun });
                }
                if out.reload {
                    self.local.push(LocalFx::Reload { gun: gun_after });
                }
                if out.ping {
                    self.local.push(LocalFx::Ping);
                }
                if out.dry {
                    self.local.push(LocalFx::Dry);
                }
                if out.throw.is_some() {
                    self.local.push(LocalFx::Throw);
                }
                if out.switched {
                    self.local.push(LocalFx::Switch);
                }
                if out.melee.is_some() {
                    self.local.push(LocalFx::Melee);
                }
                if out.mount.is_some() {
                    self.local.push(LocalFx::Mount);
                }
            }
            self.pending.push_back(cmd);
            while self.pending.len() > 180 {
                self.pending.pop_front();
            }
            self.send_cmds();
        }
        if self.acc > DT * 4.0 {
            self.acc = 0.0;
        }
    }

    fn send_cmds(&mut self) {
        let mut w = W::new(c2s::CMDS);
        w.u32(self.snap.as_ref().map(|s| s.tick).unwrap_or(0));
        w.u32(self.last_event);
        w.u8(self.team_req);
        w.u8(self.loadout.class);
        w.u8(self.loadout.primary);
        w.u8(self.loadout.perk);
        w.u8(self.loadout.secondary);
        w.u8(self.level);
        match self.chat_pending.as_mut() {
            Some((team_only, text, left)) => {
                w.u16(self.chat_id);
                w.bool(*team_only);
                w.str(text);
                *left -= 1;
                if *left == 0 {
                    self.chat_pending = None;
                }
            }
            None => w.u16(0),
        }
        let k = self.pending.len().min(4);
        w.u8(k as u8);
        for c in self.pending.iter().skip(self.pending.len() - k) {
            write_cmd(&mut w, c);
        }
        self.send(&w);
    }

    /// Jugadores remotos interpolados en el tiempo de dibujo.
    pub fn remotes(&self) -> Vec<PlayerInfo> {
        let rt = self.render_tick();
        let (a, b) = match self.bracket(rt) {
            Some(x) => x,
            None => return Vec::new(),
        };
        let (ta, pa, _) = &self.buf[a];
        let (tb, pb, _) = &self.buf[b];
        let t = if tb > ta { ((rt - *ta as f64) / (*tb - *ta) as f64).clamp(0.0, 1.0) as f32 } else { 1.0 };
        pb.iter()
            .filter(|p| p.id != self.my_id)
            .map(|p| {
                let mut out = p.clone();
                if let Some(o) = pa.iter().find(|o| o.id == p.id) {
                    if o.alive == p.alive && o.pos.dist(p.pos) < 5.0 {
                        out.pos = o.pos.lerp(p.pos, t);
                        out.yaw = o.yaw + wrap_angle(p.yaw - o.yaw) * t;
                        out.pitch = lerp(o.pitch, p.pitch, t);
                        out.crouch = lerp(o.crouch, p.crouch, t);
                        out.moving = lerp(o.moving, p.moving, t);
                    }
                }
                out
            })
            .collect()
    }

    pub fn grenades(&self) -> Vec<GrenadeInfo> {
        let rt = self.render_tick();
        let Some((a, b)) = self.bracket(rt) else { return Vec::new() };
        let (ta, _, ga) = &self.buf[a];
        let (tb, _, gb) = &self.buf[b];
        let t = if tb > ta { ((rt - *ta as f64) / (*tb - *ta) as f64).clamp(0.0, 1.0) as f32 } else { 1.0 };
        gb.iter()
            .map(|g| {
                let mut o = *g;
                if let Some(p) = ga.iter().find(|x| x.id == g.id) {
                    o.pos = p.pos.lerp(g.pos, t);
                }
                o
            })
            .collect()
    }

    fn bracket(&self, rt: f64) -> Option<(usize, usize)> {
        if self.buf.is_empty() {
            return None;
        }
        let last = self.buf.len() - 1;
        if rt >= self.buf[last].0 as f64 {
            return Some((last, last));
        }
        if rt <= self.buf[0].0 as f64 {
            return Some((0, 0));
        }
        for i in 0..last {
            if self.buf[i].0 as f64 <= rt && rt <= self.buf[i + 1].0 as f64 {
                return Some((i, i + 1));
            }
        }
        Some((last, last))
    }
}

/// Busca servidores en la red local (y en este mismo PC).
pub struct LanScanner {
    sock: Option<UdpSocket>,
    nonce: u32,
    last: Option<Instant>,
    pub found: Vec<(SocketAddr, ServerInfo, Instant)>,
}

impl Default for LanScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl LanScanner {
    pub fn new() -> LanScanner {
        let sock = UdpSocket::bind(("0.0.0.0", 0)).ok().and_then(|s| {
            s.set_nonblocking(true).ok()?;
            let _ = s.set_broadcast(true);
            Some(s)
        });
        LanScanner { sock, nonce: 0x4634, last: None, found: Vec::new() }
    }

    pub fn poll(&mut self) {
        let Some(sock) = &self.sock else { return };
        if self.last.map(|t| t.elapsed().as_secs_f32() > 2.0).unwrap_or(true) {
            let mut w = W::new(c2s::DISCOVER);
            w.u32(self.nonce);
            for ip in [Ipv4Addr::BROADCAST, Ipv4Addr::LOCALHOST] {
                let _ = sock.send_to(&w.0, SocketAddr::new(IpAddr::V4(ip), DEFAULT_PORT));
            }
            self.last = Some(Instant::now());
        }
        let mut buf = [0u8; 512];
        while let Ok((n, from)) = sock.recv_from(&mut buf) {
            let Some((kind, mut r)) = R::open(&buf[..n]) else { continue };
            if kind != s2c::INFO || r.u32() != Some(self.nonce) {
                continue;
            }
            if let Some(info) = ServerInfo::read(&mut r) {
                // El mismo PC aparece por la IP local y por 127.0.0.1: preferir la de red
                let addr = SocketAddr::new(from.ip(), info.port);
                let now = Instant::now();
                match self.found.iter_mut().find(|f| f.0 == addr) {
                    Some(f) => {
                        f.1 = info;
                        f.2 = now;
                    }
                    None => self.found.push((addr, info, now)),
                }
            }
        }
        self.found.retain(|f| f.2.elapsed().as_secs_f32() < 7.0);
        let has_lan_copy = |f: &(SocketAddr, ServerInfo, Instant), all: &Vec<(SocketAddr, ServerInfo, Instant)>| {
            f.0.ip().is_loopback() && all.iter().any(|o| !o.0.ip().is_loopback() && o.1 == f.1)
        };
        let all = self.found.clone();
        self.found.retain(|f| !has_lan_copy(f, &all));
    }
}

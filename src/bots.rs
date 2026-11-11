//! Inteligencia de los bots (soldados y zombis) que maneja el servidor.
//!
//! Cada bot produce, tick a tick, el mismo `Cmd` que mandaría un jugador:
//! así se mueven, apuntan y disparan con la misma simulación. Las decisiones
//! (a quién atacar, dónde cubrirse, cuándo flanquear o lanzar una granada)
//! se toman unas pocas veces por segundo.

use crate::math::*;
use crate::net::{bomb_state, flag_state, hq_state, FlagInfo, Mode, Phase};
use crate::sim::*;
use crate::world::World;

/// Un jugador tal y como lo ve un bot.
#[derive(Clone, Debug)]
pub struct PView {
    pub id: u8,
    pub team: u8,
    pub alive: bool,
    pub pos: V3,
    pub eye: V3,
    pub crouch: f32,
    pub zombie: bool,
    pub health: f32,
    pub carrying: bool,
}

/// Lo que sabe un bot de la partida en este tick.
pub struct BotView {
    pub mode: Mode,
    pub phase: Phase,
    pub teams: bool,
    pub players: Vec<PView>,
    /// (posición, mecha restante, equipo de quien la lanzó)
    pub grenades: Vec<(V3, f32, u8)>,
    /// Disparos del último segundo: (tirador, origen)
    pub shots: Vec<(u8, V3)>,
    pub flags: [FlagInfo; 2],
    pub flag_bases: [V3; 2],
    pub bomb_state: u8,
    pub bomb_site: u8,
    pub bomb_sites: [V3; 2],
    pub attackers: u8,
    pub hq_state: u8,
    pub hq_pos: V3,
    pub hq_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Plan {
    /// Ir hacia el objetivo del modo (o patrullar)
    Advance,
    /// Ponerse a cubierto en `at` hasta `until`
    Cover { at: V3, until: f32 },
    /// Rodear al enemigo por un lado
    Flank { at: V3, until: f32 },
    /// Huir de una granada
    Evade { at: V3, until: f32 },
}

pub struct Brain {
    pub skill: f32,
    pub zombie: bool,
    pub zombie_speed: f32,
    pub zombie_sprint: bool,
    /// Frase que gritar al equipo (la recoge el servidor)
    pub say: Option<String>,
    rng: Rng,
    seq: u32,
    t: f32,
    yaw: f32,
    pitch: f32,
    initialized: bool,
    plan: Plan,
    goal: V3,
    next: V3,
    repath: f32,
    think_t: f32,
    target: Option<u8>,
    seen_for: f32,
    last_known: Option<(V3, f32)>,
    aim_err: V3,
    burst: f32,
    pause: f32,
    strafe: f32,
    strafe_t: f32,
    crouch_t: f32,
    nade_cd: f32,
    say_cd: f32,
    stuck_t: f32,
    stuck_from: V3,
    unstick: f32,
    wander: Option<V3>,
    wander_t: f32,
    role: u8,
    prev_health: f32,
    hurt_t: f32,
    /// Rodeo hacia el objetivo (por un flanco) y si ya se ha pasado por él
    via: Option<V3>,
    via_done: bool,
}

const FOV_COS: f32 = 0.35;

impl Brain {
    pub fn new(skill: f32, zombie: bool, seed: u64) -> Brain {
        let mut rng = Rng::new(seed.max(1));
        let role = rng.index(4) as u8;
        Brain {
            skill,
            zombie,
            zombie_speed: 0.6,
            zombie_sprint: false,
            say: None,
            seq: 0,
            t: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            initialized: false,
            plan: Plan::Advance,
            goal: V3::ZERO,
            next: V3::ZERO,
            repath: 0.0,
            think_t: 0.0,
            target: None,
            seen_for: 0.0,
            last_known: None,
            aim_err: V3::ZERO,
            burst: 0.0,
            pause: 0.0,
            strafe: 0.0,
            strafe_t: 0.0,
            crouch_t: 0.0,
            nade_cd: rng.range(4.0, 10.0),
            say_cd: rng.range(2.0, 8.0),
            stuck_t: 0.0,
            stuck_from: V3::ZERO,
            unstick: 0.0,
            wander: None,
            wander_t: 0.0,
            role,
            prev_health: MAX_HEALTH,
            hurt_t: 99.0,
            via: None,
            via_done: false,
            rng,
        }
    }

    fn shout_any(&mut self, opts: &[&str]) {
        let k = self.rng.index(opts.len());
        self.shout(opts[k]);
    }

    fn shout(&mut self, text: &str) {
        if self.say_cd <= 0.0 && !self.zombie {
            self.say = Some(text.to_string());
            self.say_cd = self.rng.range(7.0, 14.0);
        }
    }

    /// Genera el comando de este tick.
    #[allow(clippy::too_many_arguments)]
    pub fn think(&mut self, me: u8, v: &BotView, world: &World, ps: &PState, alive: bool, health: f32, tick: u32) -> Cmd {
        let dt = DT;
        self.t += dt;
        self.seq += 1;
        self.say_cd -= dt;
        self.nade_cd -= dt;
        let mut cmd = Cmd { seq: self.seq, view_tick: tick, yaw: self.yaw, pitch: self.pitch, fwd: 0, strafe: 0, buttons: 0, slot: 0 };
        if !alive {
            self.initialized = false;
            self.via = None;
            self.via_done = false;
            self.target = None;
            self.plan = Plan::Advance;
            self.prev_health = MAX_HEALTH;
            return cmd;
        }
        if !self.initialized {
            self.initialized = true;
            self.yaw = ps.yaw;
            self.pitch = 0.0;
            self.goal = ps.pos;
            self.next = ps.pos;
            self.repath = 0.0;
            self.stuck_from = ps.pos;
            self.stuck_t = 0.0;
        }
        if health < self.prev_health - 0.5 {
            self.hurt_t = 0.0;
        }
        self.hurt_t += dt;
        self.prev_health = health;

        let my_team = v.players.iter().find(|p| p.id == me).map(|p| p.team).unwrap_or(0);
        let eye = ps.eye();
        let is_enemy = |p: &PView| p.id != me && p.alive && (!v.teams || p.team != my_team);

        // ---- Percepción ----
        let fwd = V3::from_yaw_pitch(self.yaw, self.pitch);
        let mut best: Option<(u8, f32, V3)> = None;
        for p in v.players.iter().filter(|p| is_enemy(p)) {
            let chest = p.pos + V3::UP * lerp(1.25, 0.8, p.crouch);
            let d = eye.dist(chest);
            if d > 140.0 {
                continue;
            }
            let dir = (chest - eye) / d.max(0.01);
            let in_fov = dir.dot(fwd) > FOV_COS || d < 9.0 || self.zombie;
            if in_fov && world.los(eye, chest) {
                // Preferir al más cercano y al que ya teníamos
                let score = d - if Some(p.id) == self.target { 8.0 } else { 0.0 };
                if best.map(|b| score < b.1).unwrap_or(true) {
                    best = Some((p.id, score, chest));
                }
            }
        }
        // Oído: disparos enemigos cercanos
        for &(sh, from) in &v.shots {
            if let Some(p) = v.players.iter().find(|p| p.id == sh) {
                if is_enemy(p) && from.dist(eye) < 55.0 && best.is_none() {
                    self.last_known = Some((p.pos, 0.0));
                }
            }
        }
        match best {
            Some((id, _, chest)) => {
                if self.target != Some(id) {
                    self.target = Some(id);
                    self.seen_for = 0.0;
                    // El primer disparo va desviado (más cuanto más lejos); se corrige con el tiempo
                    let e = ((1.0 - self.skill) * 1.2 + 0.3) * (1.0 + eye.dist(chest) / 20.0);
                    self.aim_err = v3(self.rng.range(-e, e), self.rng.range(-e * 0.6, e * 0.6), self.rng.range(-e, e));
                    if !self.zombie && self.rng.chance(0.3) {
                        self.shout_any(&["¡Enemigo a la vista!", "¡Contacto!", "¡Ahí hay uno!", "¡Fuego a discreción!"]);
                    }
                }
                self.seen_for += dt;
                self.last_known = Some((chest - V3::UP * 1.2, 0.0));
            }
            None => {
                self.target = None;
                self.seen_for = 0.0;
            }
        }
        if let Some((p, age)) = self.last_known {
            self.last_known = if age > 12.0 { None } else { Some((p, age + dt)) };
        }
        let target = self.target.and_then(|id| v.players.iter().find(|p| p.id == id).cloned());

        if self.zombie {
            return self.think_zombie(cmd, v, world, ps, my_team, target);
        }

        // ---- Decisiones (unas cuatro veces por segundo) ----
        self.think_t -= dt;
        if self.think_t <= 0.0 {
            self.think_t = self.rng.range(0.2, 0.35);
            // Granadas a los pies: huir
            if let Some(&(gp, fuse, _)) = v.grenades.iter().find(|g| g.0.dist(ps.pos) < 6.5 && g.1 < 2.8) {
                let away = (ps.pos - gp).flat();
                let away = if away.len() < 0.1 { V3::from_yaw(self.yaw) } else { away.norm() };
                let at = self.free_point(world, ps.pos + away * 8.0, ps.pos);
                self.plan = Plan::Evade { at, until: self.t + fuse + 0.4 };
                self.shout("¡Granada! ¡A cubierto!");
            }
            let reloading = ps.reload_t > 0.0;
            let in_combat = target.is_some() || self.last_known.map(|l| l.1 < 3.0).unwrap_or(false);
            match self.plan {
                Plan::Evade { until, .. } if self.t < until => {}
                Plan::Cover { until, .. } if self.t < until && (health < 85.0 || reloading) => {}
                Plan::Flank { until, at } if self.t < until && ps.pos.dist_flat(at) > 2.0 && target.is_none() => {}
                _ => {
                    self.plan = Plan::Advance;
                    // Herido o recargando en pleno combate: buscar cobertura
                    if in_combat && (health < 55.0 && self.hurt_t < 2.0 || reloading && target.is_some()) {
                        if let Some(threat) = target.as_ref().map(|t| t.eye).or(self.last_known.map(|l| l.0 + V3::UP * 1.5)) {
                            if let Some(at) = self.find_cover(world, ps.pos, threat) {
                                self.plan = Plan::Cover { at, until: self.t + self.rng.range(2.5, 5.0) };
                                if reloading {
                                    self.shout_any(&["¡Recargo! ¡Cubridme!", "¡Cambio cargador!"]);
                                } else {
                                    self.shout_any(&["¡Me han dado!", "¡Estoy herido, me pongo a cubierto!"]);
                                }
                            }
                        }
                    } else if target.is_none() {
                        if let Some((lk, age)) = self.last_known {
                            // Enemigo escondido: granada o flanqueo
                            let d = lk.dist_flat(ps.pos);
                            if age > 0.8 && age < 6.0 && d > 8.0 && d < 28.0 && ps.grenades > 0 && self.nade_cd <= 0.0 && self.rng.chance(0.35 + 0.3 * self.skill) {
                                let friends_near = v.players.iter().any(|p| p.alive && p.id != me && p.team == my_team && v.teams && p.pos.dist(lk) < 6.0);
                                if !friends_near {
                                    let (yaw, pitch) = throw_angles(eye, lk + V3::UP * 0.3);
                                    self.yaw = yaw;
                                    self.pitch = pitch;
                                    cmd.yaw = yaw;
                                    cmd.pitch = pitch;
                                    cmd.buttons |= btn::GRENADE;
                                    self.nade_cd = self.rng.range(10.0, 18.0);
                                    self.shout_any(&["¡Granada va!", "¡Fuego en el agujero!", "¡Granada!"]);
                                    return cmd;
                                }
                            }
                            if age > 2.0 && age < 8.0 && self.rng.chance(0.25) {
                                let side = if self.rng.chance(0.5) { 1.0 } else { -1.0 };
                                let to = (lk - ps.pos).flat().norm();
                                let perp = to.cross(V3::UP).norm() * side;
                                let at = self.free_point(world, lk + perp * 12.0 - to * 4.0, ps.pos);
                                self.plan = Plan::Flank { at, until: self.t + 12.0 };
                                self.shout(if side > 0.0 { "¡Flanqueo por la derecha!" } else { "¡Flanqueo por la izquierda!" });
                            }
                        }
                    }
                }
            }
            // A dónde ir
            self.goal = match self.plan {
                Plan::Cover { at, .. } | Plan::Flank { at, .. } | Plan::Evade { at, .. } => at,
                Plan::Advance => self.objective(me, my_team, v, world, ps),
            };
        }

        // ---- Apuntar ----
        let mut wants_fire = false;
        let mut ads = false;
        let def = ps.gun();
        if let Some(t) = &target {
            let d = eye.dist(t.eye);
            // La cabeza a veces, si es bueno
            let head = self.skill > 0.6 && d < 40.0 && (self.seq / 90) % 3 == 0;
            let aim_at = if head { t.eye + V3::UP * 0.02 } else { t.pos + V3::UP * lerp(1.15, 0.75, t.crouch) };
            self.aim_err = self.aim_err * (1.0 - dt * (0.5 + 1.6 * self.skill)).max(0.0);
            // Pulso: nadie apunta perfecto, y menos a lo lejos
            let jit = (1.0 - 0.8 * self.skill) * 0.012 * d;
            let ph = self.t + self.role as f32 * 1.7;
            let goal = aim_at + self.aim_err + v3((ph * 3.1).sin(), (ph * 2.3).sin() * 0.6, (ph * 2.7).cos()) * jit;
            let dir = (goal - eye).norm();
            self.turn_to(dir.yaw(), dir.y.clamp(-1.0, 1.0).asin(), 2.5 + 6.0 * self.skill);
            let err = angle_between(V3::from_yaw_pitch(self.yaw, self.pitch), dir);
            let react = 0.75 - 0.5 * self.skill;
            let tolerance = 0.03 + 0.5 / d.max(2.0);
            if self.seen_for > react && err < tolerance && ps.reload_t <= 0.0 {
                wants_fire = true;
            }
            ads = d > 14.0 || def.kind == GunKind::Sniper;
        } else if let Some((lk, age)) = self.last_known {
            if age < 4.0 {
                let dir = (lk + V3::UP * 1.3 - eye).norm();
                self.turn_to(dir.yaw(), dir.y.asin() * 0.5, 3.0);
            }
        }

        // ---- Moverse ----
        let in_cover = matches!(self.plan, Plan::Cover { at, .. } if ps.pos.dist_flat(at) < 1.0);
        let mut move_dir = V3::ZERO;
        self.repath -= dt;
        if self.repath <= 0.0 || self.next.dist_flat(ps.pos) < 0.8 {
            self.repath = 0.5;
            self.next = world.nav_target(ps.pos, self.goal);
        }
        let to_next = (self.next - ps.pos).flat();
        let arrived = self.goal.dist_flat(ps.pos) < 1.2;
        if !arrived && to_next.len() > 0.3 {
            move_dir = to_next.norm();
        }
        // En combate: moverse de lado a lado y a veces agacharse
        if target.is_some() && !in_cover && !matches!(self.plan, Plan::Evade { .. }) {
            self.strafe_t -= dt;
            if self.strafe_t <= 0.0 {
                self.strafe_t = self.rng.range(0.5, 1.4);
                self.strafe = [-1.0, 0.0, 1.0][self.rng.index(3)];
                self.crouch_t = if self.rng.chance(0.25) { self.rng.range(1.0, 2.5) } else { 0.0 };
            }
            let d = target.as_ref().map(|t| t.pos.dist(ps.pos)).unwrap_or(30.0);
            // A distancia, no avanzar en campo abierto: disparar. Salvo si hay
            // un objetivo que tomar: entonces se sigue avanzando mientras se dispara
            let pushing = matches!(v.mode, Mode::Ctf | Mode::Sd | Mode::Hq) && self.goal.dist_flat(ps.pos) > 12.0;
            if !pushing && (d < 45.0 && def.kind != GunKind::Smg || d < 12.0) {
                move_dir = move_dir * 0.3;
            } else if pushing {
                move_dir = move_dir * 0.85;
            }
            move_dir = move_dir + V3::right_of_yaw(self.yaw) * self.strafe * 0.8;
        } else if target.is_none() && self.last_known.is_none() && move_dir.len() > 0.1 {
            // Mirar hacia donde se camina
            self.turn_to(move_dir.yaw(), 0.0, 4.0);
        }
        self.crouch_t -= dt;
        let crouch = in_cover && ps.reload_t > 0.0 || in_cover && target.is_none() || self.crouch_t > 0.0 || ps.busy;

        // Atascado: saltar y probar otro camino
        self.stuck_t += dt;
        if self.stuck_t > 1.5 {
            if move_dir.len() > 0.2 && ps.pos.dist_flat(self.stuck_from) < 0.5 {
                self.unstick = 1.0;
                self.wander = Some(self.random_waypoint(world));
                self.repath = 0.0;
            }
            self.stuck_t = 0.0;
            self.stuck_from = ps.pos;
        }
        if self.unstick > 0.0 {
            self.unstick -= dt;
            cmd.buttons |= btn::JUMP;
            move_dir = move_dir + V3::right_of_yaw(self.yaw) * 0.7;
        }

        if move_dir.len() > 0.05 {
            let m = if move_dir.len() > 1.0 { move_dir.norm() } else { move_dir };
            let f = V3::from_yaw(self.yaw);
            let r = V3::right_of_yaw(self.yaw);
            cmd.fwd = Cmd::axis(m.dot(f));
            cmd.strafe = Cmd::axis(m.dot(r));
            let far = self.goal.dist_flat(ps.pos) > 15.0;
            if target.is_none() && far && !crouch || matches!(self.plan, Plan::Evade { .. }) {
                cmd.buttons |= btn::SPRINT;
            }
        }
        if crouch {
            cmd.buttons |= btn::CROUCH;
        }

        // Objetivos de modo que piden mantener «usar»
        if self.wants_use(my_team, v, ps) && target.is_none() {
            cmd.buttons |= btn::USE;
            cmd.fwd = 0;
            cmd.strafe = 0;
        }

        // ---- Disparar ----
        if wants_fire {
            if def.auto {
                // Ráfagas a distancia, fuego continuo de cerca
                let d = target.as_ref().map(|t| t.pos.dist(ps.pos)).unwrap_or(10.0);
                if self.pause > 0.0 {
                    self.pause -= dt;
                } else {
                    self.burst += dt;
                    cmd.buttons |= btn::FIRE;
                    if d > 15.0 && self.burst > self.rng.range(0.25, 0.6) {
                        self.burst = 0.0;
                        self.pause = self.rng.range(0.15, 0.4);
                    }
                }
            } else {
                // Tiro a tiro: con una pausa humana entre disparos
                self.pause -= dt;
                if self.pause <= 0.0 {
                    cmd.buttons |= btn::FIRE | btn::FIRE_PRESSED;
                    self.pause = def.interval + self.rng.range(0.25, 0.7) * (1.3 - 0.6 * self.skill);
                }
            }
            if ads {
                cmd.buttons |= btn::ADS;
            }
        } else if ads && target.is_some() {
            cmd.buttons |= btn::ADS;
        }
        // Recargar en calma; cambiar a la pistola si se acaba la munición
        let a = ps.ammo[ps.slot as usize];
        if target.is_none() && a.mag * 3 < def.mag && a.reserve > 0 && ps.reload_t <= 0.0 {
            cmd.buttons |= btn::RELOAD;
        }
        cmd.slot = if ps.ammo[0].mag == 0 && ps.ammo[0].reserve == 0 { 1 } else { 0 };
        cmd.yaw = self.yaw;
        cmd.pitch = self.pitch;
        cmd
    }

    fn think_zombie(&mut self, mut cmd: Cmd, v: &BotView, world: &World, ps: &PState, my_team: u8, target: Option<PView>) -> Cmd {
        // Va a por el vivo más cercano, lo vea o no
        let prey = target.or_else(|| {
            v.players
                .iter()
                .filter(|p| p.alive && !p.zombie && p.team != my_team)
                .min_by(|a, b| a.pos.dist(ps.pos).partial_cmp(&b.pos.dist(ps.pos)).unwrap_or(std::cmp::Ordering::Equal))
                .cloned()
        });
        let Some(prey) = prey else { return cmd };
        self.repath -= DT;
        if self.repath <= 0.0 || self.next.dist_flat(ps.pos) < 0.8 {
            self.repath = 0.4;
            self.next = world.nav_target(ps.pos, prey.pos);
        }
        let to = (self.next - ps.pos).flat();
        let d = prey.pos.dist_flat(ps.pos);
        let face = if d < 6.0 { (prey.pos - ps.pos).flat() } else { to };
        if face.len() > 0.1 {
            // Tambaleo
            let wobble = (self.t * 1.7 + self.seq as f32 * 0.0).sin() * 0.15;
            self.turn_to(face.yaw() + wobble, -0.15, 5.0);
        }
        if to.len() > 0.2 && d > 0.9 {
            let m = to.norm();
            let f = V3::from_yaw(self.yaw);
            let r = V3::right_of_yaw(self.yaw);
            cmd.fwd = Cmd::axis(m.dot(f) * self.zombie_speed);
            cmd.strafe = Cmd::axis(m.dot(r) * self.zombie_speed);
            if self.zombie_sprint && d < 25.0 {
                cmd.fwd = Cmd::axis(1.0);
                cmd.buttons |= btn::SPRINT;
            }
        }
        // Atascado
        self.stuck_t += DT;
        if self.stuck_t > 1.2 {
            if ps.pos.dist_flat(self.stuck_from) < 0.4 && d > 1.5 {
                cmd.buttons |= btn::JUMP;
                self.repath = 0.0;
            }
            self.stuck_t = 0.0;
            self.stuck_from = ps.pos;
        }
        cmd.yaw = self.yaw;
        cmd.pitch = self.pitch;
        cmd
    }

    fn turn_to(&mut self, yaw: f32, pitch: f32, rate: f32) {
        let step = rate * DT;
        self.yaw = approach_angle(self.yaw, yaw, step);
        self.pitch = approach(self.pitch, pitch.clamp(-1.3, 1.3), step);
    }

    fn random_waypoint(&mut self, world: &World) -> V3 {
        if world.waypoints.is_empty() {
            return V3::ZERO;
        }
        world.waypoints[self.rng.index(world.waypoints.len())]
    }

    /// Punto libre cercano a `want`, alcanzable desde `from`.
    fn free_point(&mut self, world: &World, want: V3, from: V3) -> V3 {
        let lim = world.half - 2.0;
        let want = v3(want.x.clamp(-lim, lim), 0.0, want.z.clamp(-lim, lim));
        if world.body_free(want, 0.4, 1.8) && world.walk_clear(from, want) {
            return want;
        }
        world
            .waypoints
            .iter()
            .copied()
            .min_by(|a, b| a.dist_flat(want).partial_cmp(&b.dist_flat(want)).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(from)
    }

    /// Busca dónde cubrirse de `threat`: un punto cercano y alcanzable desde
    /// el que no se le vea agachado (y, a ser posible, sí de pie, para asomarse).
    fn find_cover(&mut self, world: &World, pos: V3, threat: V3) -> Option<V3> {
        let mut best: Option<(V3, f32)> = None;
        let mut cands: Vec<V3> = world.waypoints.iter().copied().filter(|w| w.dist_flat(pos) < 16.0).collect();
        for k in 0..12 {
            let a = k as f32 / 12.0 * std::f32::consts::TAU + self.rng.range(0.0, 0.4);
            let r = self.rng.range(2.0, 8.0);
            cands.push(pos + v3(a.cos() * r, 0.0, a.sin() * r));
        }
        for c in cands {
            let c = v3(c.x, 0.0, c.z);
            if !world.body_free(c, 0.4, 1.3) || !world.walk_clear(pos, c) {
                continue;
            }
            let low = c + V3::UP * CROUCH_EYE;
            if world.los(threat, low) {
                continue;
            }
            let high = c + V3::UP * STAND_EYE;
            let peek = world.los(threat, high);
            let score = c.dist_flat(pos) - if peek { 4.0 } else { 0.0 } + (c.dist_flat(threat) < 6.0) as i32 as f32 * 20.0;
            if best.map(|b| score < b.1).unwrap_or(true) {
                best = Some((c, score));
            }
        }
        best.map(|b| b.0)
    }

    /// Camino hacia `to` dando un rodeo por un flanco (según el papel del
    /// bot), para no ir todos por la calle principal.
    fn route(&mut self, world: &World, from: V3, to: V3) -> V3 {
        if self.via_done || self.role == 3 || from.dist_flat(to) < 30.0 {
            return to;
        }
        if self.via.is_none() {
            let side = if self.role % 2 == 0 { 1.0 } else { -1.0 };
            let dir = (to - from).flat().norm();
            let perp = dir.cross(V3::UP).norm() * side;
            let want = from.lerp(to, 0.55) + perp * self.rng.range(18.0, 30.0);
            let via = world.waypoints.iter().copied().filter(|w| w.dist_flat(to) < from.dist_flat(to)).min_by(|a, b| {
                a.dist_flat(want).partial_cmp(&b.dist_flat(want)).unwrap_or(std::cmp::Ordering::Equal)
            });
            self.via = via;
        }
        match self.via {
            Some(p) if p.dist_flat(from) > 4.0 => p,
            _ => {
                self.via_done = true;
                to
            }
        }
    }

    fn wants_use(&self, my_team: u8, v: &BotView, ps: &PState) -> bool {
        if v.mode != Mode::Sd || v.phase != Phase::Playing {
            return false;
        }
        let near = |p: V3| p.dist_flat(ps.pos) < 1.8;
        if v.bomb_state == bomb_state::NONE && my_team == v.attackers {
            return v.bomb_sites.iter().any(|s| near(*s));
        }
        v.bomb_state == bomb_state::PLANTED && my_team != v.attackers && near(v.bomb_sites[v.bomb_site as usize % 2])
    }

    /// Hacia dónde ir según el modo de juego.
    fn objective(&mut self, me: u8, team: u8, v: &BotView, world: &World, ps: &PState) -> V3 {
        let enemy_near = |v: &BotView| {
            v.players
                .iter()
                .filter(|p| p.alive && p.id != me && (!v.teams || p.team != team))
                .min_by(|a, b| a.pos.dist(ps.pos).partial_cmp(&b.pos.dist(ps.pos)).unwrap_or(std::cmp::Ordering::Equal))
                .map(|p| p.pos)
        };
        let wander = |b: &mut Brain, world: &World| -> V3 {
            b.wander_t -= 0.3;
            if b.wander.map(|w| w.dist_flat(ps.pos) < 2.0).unwrap_or(true) || b.wander_t <= 0.0 {
                b.wander = Some(b.random_waypoint(world));
                b.wander_t = 40.0;
            }
            b.wander.unwrap()
        };
        if let Some((lk, age)) = self.last_known {
            if age < 6.0 && !matches!(v.mode, Mode::Sd | Mode::Ctf | Mode::Hq) {
                return lk;
            }
        }
        match v.mode {
            Mode::Ctf => {
                let mine = v.flags[team as usize % 2];
                let theirs = v.flags[1 - team as usize % 2];
                let carrying = theirs.state == flag_state::CARRIED && theirs.carrier == me;
                if carrying {
                    return v.flag_bases[team as usize % 2];
                }
                // Recuperar la nuestra si se la llevan
                if mine.state != flag_state::HOME && (self.role < 2 || mine.state == flag_state::DROPPED) {
                    return mine.pos;
                }
                if self.role == 0 {
                    // Defensor: cerca de la base
                    let base = v.flag_bases[team as usize % 2];
                    return if ps.pos.dist_flat(base) > 10.0 { base } else { wander(self, world) };
                }
                if theirs.state == flag_state::CARRIED {
                    // Escoltar al compañero que la lleva
                    return v.players.iter().find(|p| p.id == theirs.carrier).map(|p| p.pos).unwrap_or(theirs.pos);
                }
                self.route(world, ps.pos, theirs.pos)
            }
            Mode::Sd => {
                let site = v.bomb_sites[(if v.bomb_state == bomb_state::PLANTED { v.bomb_site as usize } else { (self.role % 2) as usize }) % 2];
                if team == v.attackers && v.bomb_state != bomb_state::PLANTED {
                    self.route(world, ps.pos, site)
                } else if v.bomb_state == bomb_state::PLANTED {
                    site
                } else if ps.pos.dist_flat(site) > 9.0 {
                    site
                } else {
                    // Defender: patrullar alrededor
                    let a = self.t * 0.2 + self.role as f32;
                    site + v3(a.cos() * 6.0, 0.0, a.sin() * 6.0)
                }
            }
            Mode::Hq if v.hq_state != hq_state::NONE => {
                if ps.pos.dist_flat(v.hq_pos) < 2.5 {
                    let a = self.t * 0.5 + self.role as f32 * 1.6;
                    v.hq_pos + v3(a.cos() * 1.8, 0.0, a.sin() * 1.8)
                } else {
                    v.hq_pos
                }
            }
            _ => {
                // Ir a por el enemigo más cercano la mitad de las veces; si no, patrullar
                if self.role % 2 == 0 {
                    enemy_near(v).unwrap_or_else(|| wander(self, world))
                } else {
                    wander(self, world)
                }
            }
        }
    }
}

fn angle_between(a: V3, b: V3) -> f32 {
    a.dot(b).clamp(-1.0, 1.0).acos()
}

/// Ángulos para lanzar una granada (velocidad 15 m/s en la mirada y 2,5
/// hacia arriba) de modo que caiga cerca de `to`.
pub fn throw_angles(from: V3, to: V3) -> (f32, f32) {
    let d = (to - from).flat().len();
    let h = to.y - from.y;
    let yaw = (to - from).yaw();
    let mut best = (0.3f32, f32::MAX);
    let mut p = -0.3f32;
    while p < 1.2 {
        let vx = 15.0 * p.cos();
        let vy = 15.0 * p.sin() + 2.5;
        // y(t) = vy t - g/2 t^2 = h  →  t positiva mayor
        let disc = vy * vy + 2.0 * GRAVITY * -h;
        if disc >= 0.0 {
            let t = (vy + disc.sqrt()) / GRAVITY;
            // La granada rueda algo tras caer
            let range = vx * t * 0.92;
            let e = (range - d).abs();
            if e < best.1 {
                best = (p, e);
            }
        }
        p += 0.02;
    }
    (yaw, best.0)
}

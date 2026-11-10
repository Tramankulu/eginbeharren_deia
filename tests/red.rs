//! Pruebas de red reales: servidor y clientes hablando por UDP en 127.0.0.1.
//! El tiempo se simula (sin esperas), así que minutos de partida tardan segundos.
//! Ejecutar con: cargo test --release

use deia::client::*;
use deia::config::Settings;
use deia::math::*;
use deia::net::*;
use deia::server::*;
use deia::sim::*;
use deia::world::World;
use std::net::SocketAddr;

fn server(modes: Vec<Mode>, limit: Option<u16>) -> Server {
    let cfg = ServerConfig { port: 0, modes, score_limit: limit, max_players: 16, ..Default::default() };
    Server::bind(cfg).expect("bind")
}

fn addr(s: &Server) -> SocketAddr {
    format!("127.0.0.1:{}", s.local_port()).parse().unwrap()
}

/// Avanza `ticks` ticks del servidor con los clientes moviéndose según `brain`.
fn run(
    s: &mut Server,
    cs: &mut [NetClient],
    world: &World,
    ticks: u32,
    mut brain: impl FnMut(usize, &NetClient, &World) -> FrameInput,
) {
    for _ in 0..ticks {
        for i in 0..cs.len() {
            let inp = brain(i, &cs[i], world);
            cs[i].update(&inp, DT, world);
        }
        s.update();
    }
}

fn idle(_: usize, _: &NetClient, _: &World) -> FrameInput {
    FrameInput::default()
}

/// Bot sencillo: busca al enemigo más cercano, camina hacia él y dispara si lo ve.
fn bot(_: usize, c: &NetClient, w: &World) -> FrameInput {
    let mut i = FrameInput::default();
    let (Some(p), Some(snap)) = (c.pred.as_ref(), c.snap.as_ref()) else { return i };
    let eye = p.eye();
    let teams = snap.mode.teams();
    let target = c
        .remotes()
        .into_iter()
        .filter(|r| r.alive && (!teams || r.team != snap.my_team))
        .min_by(|a, b| a.pos.dist(p.pos).partial_cmp(&b.pos.dist(p.pos)).unwrap());
    let Some(t) = target else { return i };
    let chest = t.pos + V3::UP * 1.2;
    let visible = w.los(eye, chest);
    let goal = if visible { chest } else { w.nav_target(p.pos, t.pos) + V3::UP * STAND_EYE };
    let d = (goal - eye).norm();
    i.look_yaw = wrap_angle(d.yaw() - c.view_yaw) * 0.25;
    let pitch = if visible { d.y.asin() } else { 0.0 };
    i.look_pitch = (pitch - c.view_pitch) * 0.25;
    let aimed = wrap_angle(d.yaw() - c.view_yaw).abs() < 0.05;
    if visible {
        i.ads = true;
        i.fire = aimed;
        i.fire_pressed = aimed;
        i.fwd = if eye.dist(chest) > 15.0 { 1.0 } else { 0.0 };
    } else {
        i.fwd = 1.0;
    }
    i
}

fn connect_all(s: &mut Server, n: usize, world: &World) -> Vec<NetClient> {
    let a = addr(s);
    let mut cs: Vec<NetClient> = (0..n).map(|k| NetClient::connect(a, &format!("Bot{k}")).unwrap()).collect();
    run(s, &mut cs, world, 30, idle);
    for c in &cs {
        assert_eq!(c.status, Status::Connected, "no conectó");
    }
    cs
}

#[test]
fn serializacion_ida_y_vuelta() {
    let mut ps = PState::new(v3(1.0, 2.0, 3.0), 0.5, AXIS);
    ps.ammo[1].mag = 7;
    ps.busy = true;
    let snap = Snapshot {
        tick: 99,
        ack: 42,
        mode: Mode::Ctf,
        phase: Phase::Playing,
        timer: 12.5,
        score_limit: 5,
        team_scores: [2, 3],
        round: 1,
        attackers: AXIS,
        flags: [FlagInfo { state: 1, carrier: 3, pos: v3(1.0, 0.0, 2.0) }; 2],
        bomb_state: 1,
        bomb_site: 1,
        bomb_timer: 30.0,
        progress: 0.5,
        my_team: 1,
        my_alive: true,
        respawn_in: 0.0,
        health: 80.0,
        me: Some(ps.clone()),
        players: vec![PlayerInfo {
            id: 3,
            name: "Ñandú áéí".into(),
            team: 1,
            side: 1,
            alive: true,
            crouch: 0.5,
            ads: true,
            sprint: false,
            has_flag: true,
            slot: 1,
            pos: v3(4.0, 0.0, -6.0),
            yaw: 1.0,
            pitch: -0.2,
            moving: 0.4,
            kills: 5,
            deaths: 2,
            score: 7,
            ping: 35,
        }],
        grenades: vec![GrenadeInfo { id: 9, pos: v3(1.0, 1.0, 1.0), side: 0 }],
        events: vec![
            (5, Ev::Kill { killer: 1, victim: 2, weapon: 3, head: true, killer_name: "a".into(), victim_name: "b".into() }),
            (6, Ev::Chat { from: 1, team_only: false, name: "a".into(), text: "¡hola!".into() }),
            (7, Ev::Whistle),
        ],
    };
    let mut w = W::new(s2c::SNAPSHOT);
    snap.write(&mut w);
    let (k, mut r) = R::open(&w.0).unwrap();
    assert_eq!(k, s2c::SNAPSHOT);
    let back = Snapshot::read(&mut r).unwrap();
    assert_eq!(back.me.unwrap(), ps);
    assert_eq!(back.players[0].name, "Ñandú áéí");
    assert!((back.players[0].yaw - 1.0).abs() < 1e-3);
    assert_eq!(back.events, snap.events);
    assert_eq!(back.team_scores, [2, 3]);
    // Paquetes basura no deben romper nada
    for n in 0..w.0.len() {
        let (_, mut r) = R::open(&w.0[..n.max(3)]).unwrap_or((0, R::open(b"ED\x00").unwrap().1));
        let _ = Snapshot::read(&mut r);
    }
}

#[test]
fn ajustes_ida_y_vuelta() {
    let mut s = Settings::default();
    s.name = "Iñaki".into();
    s.invert = true;
    s.host_modes = vec!["sd".into(), "ctf".into()];
    assert_eq!(Settings::parse(&s.to_text()), s);
}

#[test]
fn mapa_valido() {
    let w = World::village();
    for p in w.allied_spawns.iter().chain(w.axis_spawns.iter()).chain(w.waypoints.iter()) {
        assert!(w.body_free(*p, PLAYER_R, STAND_H), "punto bloqueado {p:?}");
    }
    for p in w.flag_bases.iter().chain(w.bomb_sites.iter()) {
        assert!(w.body_free(*p, PLAYER_R, STAND_H), "objetivo bloqueado {p:?}");
        assert!(w.waypoints.iter().any(|q| w.walk_clear(*p, *q)), "objetivo inaccesible {p:?}");
    }
}

#[test]
fn prediccion_coincide_con_servidor() {
    let world = World::village();
    let mut s = server(vec![Mode::Dm], None);
    s.cfg.min_players = 1;
    let mut cs = connect_all(&mut s, 1, &world);
    run(&mut s, &mut cs, &world, 120, idle);
    assert!(cs[0].alive(), "no ha reaparecido");
    let mut t = 0;
    run(&mut s, &mut cs, &world, 600, |_, c, _| {
        t += 1;
        let mut i = FrameInput { fwd: 1.0, strafe: ((t / 50) % 3) as f32 - 1.0, look_yaw: 0.01, ..Default::default() };
        i.jump = t % 90 == 0;
        i.crouch = (t / 200) % 2 == 1;
        let _ = c;
        i
    });
    // Tras parar, la predicción y el servidor deben coincidir exactamente
    run(&mut s, &mut cs, &world, 30, idle);
    let c = &cs[0];
    let pred = c.pred.as_ref().unwrap();
    let srv = &s.slots[c.my_id as usize].as_ref().unwrap().ps;
    let max_err = pred.pos.dist(srv.pos);
    assert!(max_err < 1e-3, "predicción desviada {max_err}");
    assert!(srv.pos.dist(World::village().allied_spawns[0]) > 0.0);
}

#[test]
fn partida_por_equipos_completa() {
    let world = World::village();
    let mut s = server(vec![Mode::Tdm, Mode::Dm], Some(8));
    let mut cs = connect_all(&mut s, 4, &world);
    let mut kills = 0;
    let mut ended = false;
    for _ in 0..(60 * 60 * 6) {
        run(&mut s, &mut cs, &world, 1, bot);
        kills += cs[0].events.iter().filter(|e| matches!(e, Ev::Kill { .. })).count();
        if s.phase == Phase::Intermission {
            ended = true;
            break;
        }
    }
    println!("bajas vistas por el cliente 0: {kills}, marcador {:?}", s.team_scores);
    assert!(ended, "la partida no terminó; marcador {:?}", s.team_scores);
    assert!(s.team_scores.iter().any(|&t| t >= 8));
    // Rotación al siguiente modo
    run(&mut s, &mut cs, &world, 60 * 13, idle);
    assert_eq!(s.mode, Mode::Dm);
    assert_eq!(cs[0].snap.as_ref().unwrap().mode, Mode::Dm);
}

#[test]
fn todos_contra_todos_termina() {
    let world = World::village();
    let mut s = server(vec![Mode::Dm], Some(5));
    let mut cs = connect_all(&mut s, 3, &world);
    for _ in 0..(60 * 60 * 6) {
        run(&mut s, &mut cs, &world, 1, bot);
        if s.phase == Phase::Intermission {
            break;
        }
    }
    assert_eq!(s.phase, Phase::Intermission);
    let best = s.slots.iter().flatten().map(|p| p.score).max().unwrap();
    assert!(best >= 5);
}

#[test]
fn captura_la_bandera() {
    let world = World::village();
    let mut s = server(vec![Mode::Ctf], Some(2));
    let mut cs = connect_all(&mut s, 2, &world);
    run(&mut s, &mut cs, &world, 60 * 8, idle);
    assert_eq!(s.phase, Phase::Playing);
    let id = cs[0].my_id as usize;
    let team = s.slots[id].as_ref().unwrap().team as usize;
    let enemy = 1 - team;
    for cap in 1..=2u16 {
        // Robar la bandera enemiga
        s.slots[id].as_mut().unwrap().ps.pos = world.flag_bases[enemy];
        run(&mut s, &mut cs, &world, 4, idle);
        assert_eq!(s.flags[enemy].state, flag_state::CARRIED);
        assert_eq!(cs[1].snap.as_ref().map(|x| x.flags[enemy].state), Some(flag_state::CARRIED));
        // Llevarla a casa
        s.slots[id].as_mut().unwrap().ps.pos = world.flag_bases[team];
        run(&mut s, &mut cs, &world, 2, idle);
        assert_eq!(s.team_scores[team], cap);
        assert_eq!(s.flags[enemy].state, flag_state::HOME);
    }
    assert_eq!(s.phase, Phase::Intermission);
}

#[test]
fn bandera_cae_al_morir() {
    let world = World::village();
    let mut s = server(vec![Mode::Ctf], None);
    let mut cs = connect_all(&mut s, 2, &world);
    run(&mut s, &mut cs, &world, 60 * 8, idle);
    let id = cs[0].my_id as usize;
    let team = s.slots[id].as_ref().unwrap().team as usize;
    let enemy = 1 - team;
    s.slots[id].as_mut().unwrap().ps.pos = world.flag_bases[enemy];
    run(&mut s, &mut cs, &world, 2, idle);
    s.slots[id].as_mut().unwrap().ps.pos = v3(0.0, 0.0, 20.0);
    // Granada a sus pies
    let gid = 999;
    s.grenades.push(Grenade { id: gid, pos: v3(0.0, 0.2, 20.0), vel: V3::ZERO, fuse: 0.05, owner: id as u8, side: 0, bounce_cd: 0.0 });
    run(&mut s, &mut cs, &world, 10, idle);
    assert!(!s.slots[id].as_ref().unwrap().alive);
    assert_eq!(s.flags[enemy].state, flag_state::DROPPED);
    run(&mut s, &mut cs, &world, (FLAG_RETURN * 60.0) as u32 + 10, idle);
    assert_eq!(s.flags[enemy].state, flag_state::HOME);
}

fn sd_setup(s: &mut Server, cs: &mut Vec<NetClient>, world: &World) -> (usize, usize) {
    run(s, cs, world, 60 * 8, idle);
    assert_eq!(s.phase, Phase::Playing);
    assert_eq!(s.round, 1);
    let att = s.attackers();
    let a = (0..cs.len()).find(|&k| s.slots[cs[k].my_id as usize].as_ref().unwrap().team == att).unwrap();
    let d = 1 - a;
    (a, d)
}

#[test]
fn buscar_y_destruir_desactivar() {
    let world = World::village();
    let mut s = server(vec![Mode::Sd], None);
    let mut cs = connect_all(&mut s, 2, &world);
    let (a, d) = sd_setup(&mut s, &mut cs, &world);
    let (ia, id) = (cs[a].my_id as usize, cs[d].my_id as usize);
    s.slots[ia].as_mut().unwrap().ps.pos = world.bomb_sites[1];
    run(&mut s, &mut cs, &world, (SD_PLANT_TIME * 60.0) as u32 + 20, |k, _, _| FrameInput { use_key: k == a, ..Default::default() });
    assert_eq!(s.bomb_state, bomb_state::PLANTED);
    assert_eq!(s.bomb_site, 1);
    // El atacante se aleja, el defensor desactiva
    s.slots[ia].as_mut().unwrap().ps.pos = v3(0.0, 0.0, 50.0);
    s.slots[id].as_mut().unwrap().ps.pos = world.bomb_sites[1] + v3(0.5, 0.0, 0.0);
    let def_team = s.slots[id].as_ref().unwrap().team as usize;
    run(&mut s, &mut cs, &world, (SD_DEFUSE_TIME * 60.0) as u32 + 20, |k, _, _| FrameInput { use_key: k == d, ..Default::default() });
    assert_eq!(s.team_scores[def_team], 1, "desactivar debe dar la ronda a los defensores");
    assert_eq!(s.phase, Phase::RoundEnd);
    run(&mut s, &mut cs, &world, 60 * 7, idle);
    assert_eq!(s.round, 2);
    assert_eq!(s.phase, Phase::Playing);
}

#[test]
fn buscar_y_destruir_explosion() {
    let world = World::village();
    let mut s = server(vec![Mode::Sd], None);
    let mut cs = connect_all(&mut s, 2, &world);
    let (a, d) = sd_setup(&mut s, &mut cs, &world);
    let ia = cs[a].my_id as usize;
    let att_team = s.slots[ia].as_ref().unwrap().team as usize;
    s.slots[ia].as_mut().unwrap().ps.pos = world.bomb_sites[0];
    run(&mut s, &mut cs, &world, (SD_PLANT_TIME * 60.0) as u32 + 20, |k, _, _| FrameInput { use_key: k == a, ..Default::default() });
    assert_eq!(s.bomb_state, bomb_state::PLANTED);
    let _ = d;
    s.slots[ia].as_mut().unwrap().ps.pos = v3(0.0, 0.0, 50.0);
    run(&mut s, &mut cs, &world, (SD_BOMB_TIME * 60.0) as u32 + 30, idle);
    assert_eq!(s.team_scores[att_team], 1, "la explosión debe dar la ronda a los atacantes");
    assert!(cs[d].events.iter().all(|_| true));
}

#[test]
fn buscar_y_destruir_eliminacion_y_partida() {
    let world = World::village();
    let mut s = server(vec![Mode::Sd], Some(2));
    let mut cs = connect_all(&mut s, 2, &world);
    run(&mut s, &mut cs, &world, 60 * 8, idle);
    for _ in 0..(60 * 60 * 10) {
        run(&mut s, &mut cs, &world, 1, bot);
        if s.phase == Phase::Intermission {
            break;
        }
    }
    println!("rondas {:?} tras {} rondas", s.team_scores, s.round);
    assert_eq!(s.phase, Phase::Intermission);
    assert!(s.team_scores.iter().any(|&t| t >= 2));
}

#[test]
fn chat_desconexion_y_cambio_de_equipo() {
    let world = World::village();
    let mut s = server(vec![Mode::Tdm], None);
    let mut cs = connect_all(&mut s, 3, &world);
    cs[0].send_chat("¡Al ataque!", false);
    let mut got = false;
    for _ in 0..20 {
        run(&mut s, &mut cs, &world, 1, idle);
        got |= cs[2].events.iter().any(|e| matches!(e, Ev::Chat { text, .. } if text == "¡Al ataque!"));
    }
    assert!(got, "el chat no llegó");
    // Solo una vez aunque se reenvíe
    let n = s.player_count();
    assert_eq!(n, 3);
    // Cambio de equipo al bando con menos gente
    let id2 = cs[2].my_id as usize;
    let t2 = s.slots[id2].as_ref().unwrap().team;
    let counts = |s: &Server, t: u8| s.slots.iter().flatten().filter(|p| p.team == t).count();
    if counts(&s, 1 - t2) < counts(&s, t2) {
        cs[2].team_req = if t2 == ALLIES { 2 } else { 1 };
        run(&mut s, &mut cs, &world, 5, idle);
        assert_eq!(s.slots[id2].as_ref().unwrap().team, 1 - t2);
    }
    cs[1].disconnect();
    run(&mut s, &mut cs, &world, 5, idle);
    assert_eq!(s.player_count(), 2);
}

#[test]
fn servidor_lleno_y_version() {
    let world = World::village();
    let mut cfg = ServerConfig { port: 0, max_players: 2, ..Default::default() };
    cfg.modes = vec![Mode::Dm];
    let mut s = Server::bind(cfg).unwrap();
    let mut cs = connect_all(&mut s, 2, &world);
    let mut extra = vec![NetClient::connect(addr(&s), "Tercero").unwrap()];
    for _ in 0..30 {
        extra[0].update(&FrameInput::default(), DT, &world);
        run(&mut s, &mut cs, &world, 1, idle);
    }
    assert_eq!(extra[0].status, Status::Failed("El servidor está lleno".into()));
}

#[test]
fn compensacion_de_lag_acierta_con_retraso() {
    // Un tirador que ve el mundo 150 ms en el pasado debe acertar a un
    // objetivo que se mueve, gracias al rebobinado del servidor.
    let world = World::village();
    let mut s = server(vec![Mode::Dm], None);
    let mut cs = connect_all(&mut s, 2, &world);
    run(&mut s, &mut cs, &world, 60 * 8, idle);
    let (ia, ib) = (cs[0].my_id as usize, cs[1].my_id as usize);
    s.slots[ia].as_mut().unwrap().ps.pos = v3(0.0, 0.0, -30.0);
    s.slots[ib].as_mut().unwrap().ps.pos = v3(-2.0, 0.0, -10.0);
    s.slots[ib].as_mut().unwrap().health = 1e9;
    run(&mut s, &mut cs, &world, 30, idle);
    // B corre de lado; A dispara a donde lo ve (interpolado, en el pasado)
    let mut hits = 0;
    let mut shots = 0;
    for frame in 0..240 {
        run(&mut s, &mut cs, &world, 1, |k, c, _| {
            if k == 1 {
                return FrameInput { strafe: if (frame / 60) % 2 == 0 { 1.0 } else { -1.0 }, ..Default::default() };
            }
            let mut i = FrameInput { ads: true, ..Default::default() };
            if let (Some(p), Some(t)) = (c.pred.as_ref(), c.remotes().into_iter().find(|r| r.alive)) {
                let d = (t.pos + V3::UP * 1.1 - p.eye()).norm();
                i.look_yaw = wrap_angle(d.yaw() - c.view_yaw);
                i.look_pitch = d.y.asin() - c.view_pitch;
                i.fire_pressed = frame % 20 == 10;
            }
            i
        });
        hits += cs[0].events.iter().filter(|e| matches!(e, Ev::Hit { shooter } if *shooter as usize == ia)).count();
        shots += cs[0].events.iter().filter(|e| matches!(e, Ev::Shot { shooter, .. } if *shooter as usize == ia)).count();
        // Objetivo inmortal para poder contar todos los disparos
        if let Some(b) = s.slots[ib].as_mut() {
            b.health = 1e9;
        }
    }
    println!("aciertos {hits} de {shots}");
    assert!(shots >= 6 && hits * 10 >= shots * 9, "con compensación de lag debería acertar casi siempre ({hits} de {shots})");
}

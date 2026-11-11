//! Partidas enteras jugadas por bots (sin esperas reales) y reglas nuevas:
//! clases, Cuartel General y Zombis.

use deia::client::*;
use deia::net::*;
use deia::server::*;
use deia::sim::*;
use deia::world::World;
use std::net::SocketAddr;

fn server(modes: Vec<Mode>, bots: usize, limit: Option<u16>) -> Server {
    let cfg = ServerConfig { port: 0, modes, score_limit: limit, max_players: 20, bots, bot_skill: 0.6, ..Default::default() };
    Server::bind(cfg).expect("bind")
}

fn total_kills(s: &Server) -> u32 {
    s.slots.iter().flatten().map(|p| p.kills as u32).sum()
}

/// Avanza hasta `secs` segundos simulados o hasta que acabe la partida.
fn play(s: &mut Server, secs: f32) -> bool {
    for _ in 0..(secs * TICK_HZ as f32) as u32 {
        s.update();
        if s.phase == Phase::Intermission {
            return true;
        }
    }
    false
}

#[test]
fn clases_validas_para_cada_nacion() {
    for nat in [nation::US, nation::UK, nation::RU, nation::DE] {
        for (c, class) in CLASSES.iter().enumerate() {
            let opts = Loadout::options(c as u8, nat);
            assert!(!opts.is_empty(), "{} sin armas para {}", class.name, nation_name(nat));
            let (guns, nades) = Loadout { class: c as u8, primary: 255, perk: perk::GRENADIER, secondary: 255 }.resolve(nat);
            assert_eq!(gun(guns[1]).kind, GunKind::Pistol);
            assert_eq!(gun(guns[0]).nation, nat);
            assert!(nades >= 2);
        }
    }
    // Un arma de otra nación no se puede elegir
    let (guns, _) = Loadout { class: 1, primary: 3, perk: 0, secondary: 255 }.resolve(nation::RU);
    assert_eq!(gun(guns[0]).name, "PPSh-41");
    assert_eq!(level_of(0), 1);
    assert_eq!(level_of(xp_for_level(5)), 5);
    assert_eq!(level_of(u32::MAX), 20);
}

#[test]
fn bots_juegan_por_equipos() {
    let mut s = server(vec![Mode::Tdm], 10, Some(30));
    let ended = play(&mut s, 15.0 * 60.0);
    let kills = total_kills(&s);
    println!("por equipos: {:?} en {} s, {} bajas", s.team_scores, s.tick / TICK_HZ, kills);
    assert_eq!(s.player_count(), 10);
    assert!(kills >= 20, "los bots apenas combaten: {kills} bajas");
    assert!(ended, "la partida no terminó");
    // Equipos equilibrados
    let allies = s.slots.iter().flatten().filter(|p| p.team == ALLIES).count();
    assert_eq!(allies, 5);
}

#[test]
fn bots_juegan_buscar_y_destruir() {
    let mut s = server(vec![Mode::Sd], 8, Some(3));
    play(&mut s, 8.0 * 60.0);
    println!("buscar y destruir: ronda {} marcador {:?}", s.round, s.team_scores);
    assert!(s.round >= 3, "las rondas no avanzan");
}

#[test]
fn bots_juegan_cuartel_general() {
    let mut s = server(vec![Mode::Hq], 10, None);
    play(&mut s, 9.0 * 60.0);
    println!("cuartel general: {:?}", s.team_scores);
    assert!(s.team_scores[0] + s.team_scores[1] > 5, "nadie toma el cuartel");
}

#[test]
fn bots_juegan_captura_la_bandera() {
    let mut s = server(vec![Mode::Ctf], 10, Some(3));
    play(&mut s, 12.0 * 60.0);
    println!("bandera: {:?}, bajas {}", s.team_scores, total_kills(&s));
    assert!(total_kills(&s) > 10);
}

#[test]
fn zombis_por_oleadas() {
    let mut s = server(vec![Mode::Zombies], 4, None);
    let addr: SocketAddr = format!("127.0.0.1:{}", s.local_port()).parse().unwrap();
    let world = World::village();
    let mut c = NetClient::connect(addr, "Superviviente").unwrap();
    let mut max_wave = 0;
    let mut zombies_seen = 0;
    for _ in 0..(10 * 60 * TICK_HZ) {
        c.update(&FrameInput::default(), DT, &world);
        s.update();
        max_wave = max_wave.max(s.wave);
        zombies_seen = zombies_seen.max(s.slots.iter().flatten().filter(|p| p.zombie).count());
        if s.phase == Phase::Intermission {
            break;
        }
    }
    println!("zombis: oleada {max_wave}, máximo {zombies_seen} a la vez, fase {:?}", s.phase);
    assert!(max_wave >= 2, "los bots no superan ni la primera oleada");
    assert!(zombies_seen >= 4);
    // Los zombis no reciben instantáneas ni cuentan como humanos
    assert_eq!(s.human_count(), 1);
}

#[test]
fn ametralladora_montada_y_culatazo() {
    use deia::math::*;
    let w = World::village();
    assert_eq!(w.turrets.len(), 4);
    let t = w.turrets[0];
    let mut p = PState::equip(t.stand, t.yaw, ALLIES, nation::US, &Loadout::default());
    let mut rng = Rng::new(1);
    let mut cmd = Cmd { yaw: t.yaw, buttons: btn::MOUNT, ..Default::default() };
    let out = step(&w, &mut p, &cmd, &mut rng);
    assert_eq!(out.mount, Some(true));
    assert!(p.is_mounted());
    assert_eq!(gun(p.gun_id()).name, "M1919 Browning");
    // No se puede girar más allá del arco del nido
    cmd.buttons = 0;
    cmd.yaw = t.yaw + 2.5;
    for _ in 0..30 {
        step(&w, &mut p, &cmd, &mut rng);
    }
    assert!(wrap_angle(p.yaw - t.yaw).abs() <= MOUNT_ARC + 1e-3);
    // Fuego continuo hasta recalentar
    cmd.buttons = btn::FIRE;
    let mut shots = 0;
    for _ in 0..60 * 8 {
        if step(&w, &mut p, &cmd, &mut rng).shot.is_some() {
            shots += 1;
        }
        if p.overheated {
            break;
        }
    }
    assert!(p.overheated, "no se recalienta");
    assert!(shots > 20);
    // Soltar
    cmd.buttons = btn::MOUNT;
    let out = step(&w, &mut p, &cmd, &mut rng);
    assert_eq!(out.mount, Some(false));
    assert!(!p.is_mounted());
    // Culatazo y humo
    for _ in 0..40 {
        step(&w, &mut p, &Cmd { yaw: t.yaw, ..Default::default() }, &mut rng);
    }
    let out = step(&w, &mut p, &Cmd { yaw: t.yaw, buttons: btn::MELEE, ..Default::default() }, &mut rng);
    assert!(out.melee.is_some());
    for _ in 0..60 {
        step(&w, &mut p, &Cmd { yaw: t.yaw, ..Default::default() }, &mut rng);
    }
    let out = step(&w, &mut p, &Cmd { yaw: t.yaw, buttons: btn::SMOKE, ..Default::default() }, &mut rng);
    assert!(out.throw.is_some() && out.smoke);
    assert_eq!(p.smokes, 0);
    // Pistola a elegir en Alemania
    assert_eq!(Loadout::pistols(nation::DE).len(), 2);
    let (g, _) = Loadout { secondary: Loadout::pistols(nation::DE)[1], ..Default::default() }.resolve(nation::DE);
    assert_eq!(gun(g[1]).name, "Walther P38");
}

#[test]
fn las_copas_de_los_arboles_tapan_la_vista() {
    use deia::math::*;
    let w = World::village();
    assert!(!w.foliage.is_empty());
    let (c, r) = w.foliage[0];
    // A través de la copa no se ve; por debajo, sí (salvo que haya algo más)
    let a = c + v3(-r * 2.0, 0.0, 0.0);
    let b = c + v3(r * 2.0, 0.0, 0.0);
    assert!(!w.sight(a, b));
    assert!(w.los(a, b) || !w.sight(a, b));
}

#[test]
fn los_bots_no_disparan_desde_lejos_con_subfusil() {
    let mut s = server(vec![Mode::Tdm], 10, Some(200));
    let mut far = 0;
    let mut shots = 0;
    for _ in 0..(4 * 60 * TICK_HZ) {
        s.update();
        // Distancia al enemigo más cercano de cada bot que acaba de disparar
        for (i, p) in s.slots.iter().enumerate().filter_map(|(i, p)| p.as_ref().map(|p| (i, p))) {
            if p.alive && p.ps.cooldown > p.ps.gun().interval - deia::sim::DT * 1.5 && p.ps.cooldown > 0.0 {
                let d = s
                    .slots
                    .iter()
                    .enumerate()
                    .filter_map(|(j, o)| o.as_ref().filter(|o| j != i && o.alive && o.team != p.team).map(|o| o.ps.pos.dist(p.ps.pos)))
                    .fold(999.0f32, f32::min);
                shots += 1;
                if gun(p.ps.gun_id()).kind == GunKind::Smg && d > 60.0 {
                    far += 1;
                }
            }
        }
    }
    println!("disparos {shots}, de subfusil a más de 60 m: {far}, bajas {}", total_kills(&s));
    assert!(shots > 50);
    assert!(far * 20 < shots, "demasiados disparos de subfusil a distancia");
}

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
            let (guns, nades) = Loadout { class: c as u8, primary: 255, perk: perk::GRENADIER }.resolve(nat);
            assert_eq!(gun(guns[1]).kind, GunKind::Pistol);
            assert_eq!(gun(guns[0]).nation, nat);
            assert!(nades >= 2);
        }
    }
    // Un arma de otra nación no se puede elegir
    let (guns, _) = Loadout { class: 1, primary: 3, perk: 0 }.resolve(nation::RU);
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
    play(&mut s, 6.0 * 60.0);
    println!("cuartel general: {:?}", s.team_scores);
    assert!(s.team_scores[0] + s.team_scores[1] > 10, "nadie toma el cuartel");
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

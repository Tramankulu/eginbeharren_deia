//! Servidor dedicado de Eginbeharraren deia (sin ventana).
//!
//! Uso: eginbeharraren-deia-server [--nombre "Mi servidor"] [--puerto 28960] [--max 12]
//!                      [--modos tdm,dm,sd,ctf,hq,zombis] [--limite N] [--tiempo MINUTOS]
//!                      [--bots N] [--nivel-bots 0-3] [--aliados eeuu|britanicos|rusos]

use deia::net::{Mode, DEFAULT_PORT, MAX_PLAYERS};
use deia::server::{Server, ServerConfig};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn usage() -> ! {
    println!(
        "Servidor dedicado de Eginbeharraren deia\n\n\
         Opciones:\n  \
         --nombre TEXTO     Nombre que verán los jugadores\n  \
         --puerto N         Puerto UDP (por defecto {DEFAULT_PORT})\n  \
         --max N            Jugadores máximos (2-{MAX_PLAYERS}, por defecto 12)\n  \
         --modos LISTA      Rotación de modos: dm, tdm, sd, ctf, hq, zombis (por defecto tdm)\n  \
         --limite N         Límite de puntuación (sustituye al del modo)\n  \
         --tiempo MIN       Límite de tiempo en minutos\n  \
         --bots N           Rellenar con bots hasta N jugadores (por defecto 0)\n  \
         --nivel-bots N     0 novatos, 1 regulares, 2 veteranos, 3 élite\n  \
         --aliados NACIÓN   eeuu, britanicos o rusos (por defecto eeuu)\n\n\
         Para jugar por internet, abre el puerto UDP en el router hacia este equipo."
    );
    std::process::exit(0)
}

fn main() {
    let mut cfg = ServerConfig { log: true, ..Default::default() };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let val = |i: usize| -> String {
        args.get(i + 1).cloned().unwrap_or_else(|| {
            eprintln!("Falta el valor de {}", args[i]);
            std::process::exit(2)
        })
    };
    while i < args.len() {
        match args[i].as_str() {
            "--nombre" | "--name" => cfg.name = val(i),
            "--puerto" | "--port" => cfg.port = val(i).parse().unwrap_or(DEFAULT_PORT),
            "--max" => cfg.max_players = val(i).parse().unwrap_or(12),
            "--modos" | "--modes" => {
                cfg.modes = val(i).split(',').filter_map(Mode::parse).collect();
                if cfg.modes.is_empty() {
                    eprintln!("Ningún modo válido. Usa dm, tdm, sd, ctf, hq o zombis.");
                    std::process::exit(2);
                }
            }
            "--limite" => cfg.score_limit = val(i).parse().ok(),
            "--tiempo" => cfg.time_limit_min = val(i).parse().ok(),
            "--bots" => cfg.bots = val(i).parse().unwrap_or(0),
            "--nivel-bots" => cfg.bot_skill = deia::server::bot_skill_of(val(i).parse().unwrap_or(1)),
            "--aliados" => {
                cfg.allied_nation = deia::sim::parse_nation(&val(i)).unwrap_or_else(|| {
                    eprintln!("Nación desconocida: usa eeuu, britanicos o rusos");
                    std::process::exit(2)
                })
            }
            "-h" | "--help" | "--ayuda" => usage(),
            other => {
                eprintln!("Opción desconocida: {other} (usa --ayuda)");
                std::process::exit(2);
            }
        }
        i += 2;
    }
    let server = match Server::bind(cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("No se pudo abrir el puerto: {e}");
            std::process::exit(1);
        }
    };
    println!("Pulsa Ctrl+C para cerrar el servidor.");
    server.run(Arc::new(AtomicBool::new(false)));
}

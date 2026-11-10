//! Eginbeharraren deia — shooter multijugador en línea de la Segunda Guerra Mundial.
//! Este archivo es la capa gráfica (macroquad): menús, entrada, dibujado, HUD y audio.
//! La red, la simulación y el servidor viven en la biblioteca `eginbeharraren-deia`.

#![windows_subsystem = "windows"]

use deia::client::{resolve, FrameInput, LanScanner, LocalFx, NetClient, Status};
use deia::config::Settings;
use deia::geo::{self, Geo, SoldierLook, ViewParams};
use deia::math::{lerp, v3, Rng, V3};
use deia::net::*;
use deia::server::{spawn_listen_server, ServerConfig, SITE_RADIUS};
use deia::sfx;
use deia::sim::{gun_index, team_name, GUNS, MAX_HEALTH, STAND_EYE};
use deia::world::World;
use macroquad::audio::{load_sound_from_bytes, play_sound, PlaySoundParams, Sound};
use macroquad::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const SKY: [f32; 3] = [0.64, 0.67, 0.69];
const CREAM: Color = Color::new(0.95, 0.9, 0.78, 1.0);
const DIM: Color = Color::new(0.85, 0.82, 0.72, 0.85);
const GOLD: Color = Color::new(1.0, 0.82, 0.45, 1.0);
const ALLIED_C: Color = Color::new(0.55, 0.78, 0.45, 1.0);
const AXIS_C: Color = Color::new(0.95, 0.45, 0.4, 1.0);

fn window_conf() -> Conf {
    Conf {
        window_title: "Eginbeharraren deia".to_owned(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        sample_count: 4,
        ..Default::default()
    }
}

fn tv(v: V3) -> Vec3 {
    vec3(v.x, v.y, v.z)
}

fn team_color(t: u8) -> Color {
    if t == 0 {
        ALLIED_C
    } else {
        AXIS_C
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a)
}

// ----------------------------------------------------------------------
// Audio
// ----------------------------------------------------------------------

struct Sounds {
    guns: [Option<Sound>; 4],
    far_rifle: Option<Sound>,
    far_smg: Option<Sound>,
    explosion: Option<Sound>,
    ping: Option<Sound>,
    reload: Option<Sound>,
    bolt: Option<Sound>,
    dry: Option<Sound>,
    switch: Option<Sound>,
    hit: Option<Sound>,
    hurt: Option<Sound>,
    bounce: Option<Sound>,
    throw: Option<Sound>,
    whistle: Option<Sound>,
    volume: f32,
}

async fn snd(samples: Vec<f32>) -> Option<Sound> {
    load_sound_from_bytes(&sfx::to_wav(&samples)).await.ok()
}

impl Sounds {
    async fn load(volume: f32) -> Sounds {
        Sounds {
            guns: [snd(sfx::rifle()).await, snd(sfx::smg()).await, snd(sfx::kar98k()).await, snd(sfx::mp40()).await],
            far_rifle: snd(sfx::enemy_rifle()).await,
            far_smg: snd(sfx::enemy_smg()).await,
            explosion: snd(sfx::explosion()).await,
            ping: snd(sfx::ping()).await,
            reload: snd(sfx::reload()).await,
            bolt: snd(sfx::bolt()).await,
            dry: snd(sfx::dry()).await,
            switch: snd(sfx::switch_gun()).await,
            hit: snd(sfx::hitmarker()).await,
            hurt: snd(sfx::hurt()).await,
            bounce: snd(sfx::bounce()).await,
            throw: snd(sfx::throw()).await,
            whistle: snd(sfx::whistle()).await,
            volume,
        }
    }
    fn play(&self, s: &Option<Sound>, v: f32) {
        if let Some(s) = s {
            let volume = (v * self.volume).clamp(0.0, 1.0);
            if volume > 0.01 {
                play_sound(s, PlaySoundParams { looped: false, volume });
            }
        }
    }
}

// ----------------------------------------------------------------------
// Efectos visuales locales
// ----------------------------------------------------------------------

struct Fx {
    pos: V3,
    vel: V3,
    ttl: f32,
    max: f32,
    size: f32,
    grow: f32,
    gravity: f32,
    color: [f32; 3],
    alpha: f32,
}

#[allow(clippy::too_many_arguments)]
fn spray(v: &mut Vec<Fx>, rng: &mut Rng, pos: V3, n: usize, color: [f32; 3], speed: f32, up: f32, size: f32, ttl: f32, gravity: f32, grow: f32, alpha: f32) {
    for _ in 0..n {
        if v.len() > 900 {
            return;
        }
        let t = ttl * rng.range(0.6, 1.0);
        v.push(Fx {
            pos,
            vel: rng.dir() * speed * rng.range(0.3, 1.0) + V3::UP * up * rng.range(0.4, 1.0),
            ttl: t,
            max: t,
            size: size * rng.range(0.6, 1.3),
            grow,
            gravity,
            color,
            alpha,
        });
    }
}

struct Corpse {
    id: u8,
    pos: V3,
    yaw: f32,
    side: u8,
    t: f32,
}

struct FeedLine {
    killer: String,
    kteam: u8,
    victim: String,
    vteam: u8,
    weapon: u8,
    head: bool,
    ttl: f32,
}

struct Cam {
    eye: V3,
    fwd: V3,
    up: V3,
    fov: f32,
}

fn project(cam: &Cam, p: V3) -> Option<Vec2> {
    let (sw, sh) = (screen_width(), screen_height());
    let right = cam.fwd.cross(cam.up).norm();
    let up = right.cross(cam.fwd).norm();
    let d = p - cam.eye;
    let z = d.dot(cam.fwd);
    if z < 0.2 {
        return None;
    }
    let t = (cam.fov.to_radians() * 0.5).tan();
    let x = d.dot(right) / (z * t * (sw / sh));
    let y = d.dot(up) / (z * t);
    if x.abs() > 1.2 || y.abs() > 1.2 {
        return None;
    }
    Some(vec2(sw * 0.5 * (1.0 + x), sh * 0.5 * (1.0 - y)))
}

// ----------------------------------------------------------------------
// Interfaz: textos, botones y campos
// ----------------------------------------------------------------------

fn text_w(s: &str, size: f32) -> f32 {
    measure_text(s, None, size as u16, 1.0).width
}

fn txt(s: &str, x: f32, y: f32, size: f32, col: Color) {
    draw_text(s, x + 2.0, y + 2.0, size, Color::new(0.0, 0.0, 0.0, 0.55 * col.a));
    draw_text(s, x, y, size, col);
}

fn txt_c(s: &str, x: f32, y: f32, size: f32, col: Color) {
    txt(s, x - text_w(s, size) / 2.0, y, size, col);
}

fn txt_r(s: &str, x: f32, y: f32, size: f32, col: Color) {
    txt(s, x - text_w(s, size), y, size, col);
}

fn panel(r: Rect, a: f32) {
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.06, 0.06, 0.05, a));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, Color::new(0.95, 0.9, 0.78, 0.25));
}

fn button(r: Rect, label: &str) -> bool {
    let hover = r.contains(mouse_position().into());
    let bg = if hover { Color::new(0.55, 0.45, 0.25, 0.9) } else { Color::new(0.12, 0.12, 0.1, 0.85) };
    draw_rectangle(r.x, r.y, r.w, r.h, bg);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, with_alpha(CREAM, if hover { 0.9 } else { 0.35 }));
    txt_c(label, r.x + r.w / 2.0, r.y + r.h / 2.0 + 8.0, 24.0, CREAM);
    hover && is_mouse_button_pressed(MouseButton::Left)
}

fn small_button(r: Rect, label: &str) -> bool {
    let hover = r.contains(mouse_position().into());
    let bg = if hover { Color::new(0.55, 0.45, 0.25, 0.9) } else { Color::new(0.16, 0.15, 0.12, 0.9) };
    draw_rectangle(r.x, r.y, r.w, r.h, bg);
    txt_c(label, r.x + r.w / 2.0, r.y + r.h / 2.0 + 7.0, 22.0, CREAM);
    hover && is_mouse_button_pressed(MouseButton::Left)
}

/// Campo de texto. `focus` guarda qué campo tiene el foco.
fn text_field(r: Rect, label: &str, value: &mut String, id: u32, focus: &mut u32, max: usize, allowed: fn(char) -> bool) {
    let hover = r.contains(mouse_position().into());
    if is_mouse_button_pressed(MouseButton::Left) {
        if hover {
            *focus = id;
        } else if *focus == id {
            *focus = 0;
        }
    }
    let focused = *focus == id;
    txt(label, r.x, r.y - 8.0, 20.0, DIM);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.03, 0.03, 0.03, 0.85));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, if focused { 2.0 } else { 1.0 }, with_alpha(CREAM, if focused { 0.9 } else { 0.35 }));
    if focused {
        while let Some(c) = get_char_pressed() {
            if !c.is_control() && allowed(c) && value.chars().count() < max {
                value.push(c);
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            value.pop();
        }
    }
    let caret = if focused && (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
    txt(&format!("{value}{caret}"), r.x + 10.0, r.y + r.h / 2.0 + 8.0, 24.0, CREAM);
}

fn any_char(_: char) -> bool {
    true
}
fn addr_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '-'
}
fn digit_char(c: char) -> bool {
    c.is_ascii_digit()
}

fn stepper(x: f32, y: f32, label: &str, value: &str) -> i32 {
    txt(label, x, y + 24.0, 22.0, DIM);
    let mut d = 0;
    if small_button(Rect::new(x + 260.0, y, 36.0, 34.0), "-") {
        d = -1;
    }
    txt_c(value, x + 340.0, y + 25.0, 24.0, CREAM);
    if small_button(Rect::new(x + 384.0, y, 36.0, 34.0), "+") {
        d = 1;
    }
    d
}

// ----------------------------------------------------------------------
// Aplicación
// ----------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum Screen {
    Main,
    Join,
    Host,
    Options,
    Connecting,
    Game,
    Error(String),
}

struct Host {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    port: u16,
}

struct App {
    settings: Settings,
    screen: Screen,
    world: World,
    sg: Geo,
    dg: Geo,
    pg: Geo,
    sounds: Sounds,
    client: Option<NetClient>,
    host: Option<Host>,
    scanner: Option<LanScanner>,
    focus: u32,
    port_text: String,
    // En partida
    last_mouse: Option<Vec2>,
    crouch: bool,
    pause: bool,
    team_menu: bool,
    chat: Option<(String, bool)>,
    feed: Vec<FeedLine>,
    chat_log: Vec<(String, Color, f32)>,
    announces: Vec<(String, u8, f32)>,
    tracers: Vec<(V3, V3, f32)>,
    fx: Vec<Fx>,
    indicators: Vec<(V3, f32)>,
    hitmarker: f32,
    shake: f32,
    corpses: Vec<Corpse>,
    alive_prev: HashMap<u8, bool>,
    walk: HashMap<u8, (V3, f32)>,
    fired: HashMap<u8, f32>,
    flash: HashMap<u8, f32>,
    my_death: Option<(V3, f32, f32)>,
    killed_by: Option<String>,
    spectate: usize,
    rng: Rng,
    clock: f32,
}

impl App {
    fn new(settings: Settings, sounds: Sounds) -> App {
        let world = World::village();
        let sg = geo::build_world(&world);
        let port_text = settings.host_port.to_string();
        App {
            settings,
            screen: Screen::Main,
            world,
            sg,
            dg: Geo::new(),
            pg: Geo::new(),
            sounds,
            client: None,
            host: None,
            scanner: None,
            focus: 0,
            port_text,
            last_mouse: None,
            crouch: false,
            pause: false,
            team_menu: false,
            chat: None,
            feed: Vec::new(),
            chat_log: Vec::new(),
            announces: Vec::new(),
            tracers: Vec::new(),
            fx: Vec::new(),
            indicators: Vec::new(),
            hitmarker: 0.0,
            shake: 0.0,
            corpses: Vec::new(),
            alive_prev: HashMap::new(),
            walk: HashMap::new(),
            fired: HashMap::new(),
            flash: HashMap::new(),
            my_death: None,
            killed_by: None,
            spectate: 0,
            rng: Rng::new((get_time() * 1e6) as u64 ^ 0xF44),
            clock: 0.0,
        }
    }

    fn grab(&self, on: bool) {
        set_cursor_grab(on);
        show_mouse(!on);
    }

    fn reset_game_state(&mut self) {
        self.last_mouse = None;
        self.crouch = false;
        self.pause = false;
        self.team_menu = false;
        self.chat = None;
        self.feed.clear();
        self.chat_log.clear();
        self.announces.clear();
        self.tracers.clear();
        self.fx.clear();
        self.indicators.clear();
        self.corpses.clear();
        self.alive_prev.clear();
        self.walk.clear();
        self.fired.clear();
        self.flash.clear();
        self.my_death = None;
        self.killed_by = None;
    }

    fn connect_to(&mut self, addr: std::net::SocketAddr) {
        self.reset_game_state();
        match NetClient::connect(addr, &self.settings.name) {
            Ok(c) => {
                self.client = Some(c);
                self.screen = Screen::Connecting;
            }
            Err(e) => self.screen = Screen::Error(format!("No se pudo abrir la red: {e}")),
        }
    }

    fn leave(&mut self, reason: Option<String>) {
        if let Some(c) = self.client.as_mut() {
            if c.status == Status::Connected {
                c.disconnect();
            }
        }
        self.client = None;
        if let Some(mut h) = self.host.take() {
            h.stop.store(true, Ordering::Relaxed);
            if let Some(t) = h.thread.take() {
                let _ = t.join();
            }
        }
        self.grab(false);
        self.screen = match reason {
            Some(r) => Screen::Error(r),
            None => Screen::Main,
        };
    }

    fn start_host(&mut self) {
        let modes: Vec<Mode> = self.settings.host_modes.iter().filter_map(|m| Mode::parse(m)).collect();
        let cfg = ServerConfig {
            name: clean_name(&self.settings.host_name, 32),
            port: self.settings.host_port,
            max_players: self.settings.host_max as usize,
            modes: if modes.is_empty() { vec![Mode::Tdm] } else { modes },
            ..Default::default()
        };
        match spawn_listen_server(cfg) {
            Ok((port, stop, thread)) => {
                self.host = Some(Host { stop, thread: Some(thread), port });
                let addr = format!("127.0.0.1:{port}").parse().unwrap();
                self.connect_to(addr);
            }
            Err(e) => {
                self.screen = Screen::Error(format!(
                    "No se pudo abrir el puerto {}: {e}\n¿Hay otro servidor abierto en este equipo? Prueba con otro puerto.",
                    self.settings.host_port
                ))
            }
        }
    }

    // ------------------------------------------------------------------
    // Menús
    // ------------------------------------------------------------------

    fn menu_background(&mut self) {
        let a = self.clock * 0.05;
        let eye = v3(a.cos() * 58.0, 24.0, a.sin() * 58.0);
        let cam = Cam { eye, fwd: (v3(0.0, 2.0, 6.0) - eye).norm(), up: V3::UP, fov: 55.0 };
        self.begin_3d(&cam);
        let mut dg = std::mem::take(&mut self.dg);
        dg.clear();
        for (k, s) in self.world.bomb_sites.iter().enumerate() {
            let _ = k;
            geo::bomb_site(&mut dg, *s);
        }
        for t in 0..2u8 {
            geo::flag(&mut dg, self.world.flag_bases[t as usize], t, self.clock, true);
        }
        draw_geo(&dg, cam.eye);
        self.dg = dg;
        set_default_camera();
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.05, 0.05, 0.04, 0.5));
    }

    fn title(&self) {
        let cx = screen_width() / 2.0;
        let size = (screen_width() / 13.0).min(80.0);
        txt_c("EGINBEHARRAREN DEIA", cx, 110.0, size, CREAM);
        txt_c("Multijugador · Saint-Aubin, Normandía, 1944", cx, 148.0, 26.0, DIM);
    }

    fn screen_main(&mut self) {
        self.menu_background();
        self.title();
        let cx = screen_width() / 2.0;
        let w = 360.0;
        let mut y = 220.0;
        let mut b = |label: &str| {
            let r = Rect::new(cx - w / 2.0, y, w, 52.0);
            y += 66.0;
            button(r, label)
        };
        let join = b("Unirse a una partida");
        let host = b("Crear partida");
        let opts = b("Ajustes");
        let quit = b("Salir");
        txt_c(&format!("Jugador: {}", self.settings.name), cx, y + 30.0, 22.0, DIM);
        if join {
            self.scanner = Some(LanScanner::new());
            self.focus = 0;
            self.screen = Screen::Join;
        }
        if host {
            self.port_text = self.settings.host_port.to_string();
            self.focus = 0;
            self.screen = Screen::Host;
        }
        if opts {
            self.focus = 0;
            self.screen = Screen::Options;
        }
        if quit {
            std::process::exit(0);
        }
    }

    fn screen_join(&mut self) {
        self.menu_background();
        self.title();
        if let Some(s) = self.scanner.as_mut() {
            s.poll();
        }
        let cx = screen_width() / 2.0;
        let w = 720.0f32.min(screen_width() - 40.0);
        let x = cx - w / 2.0;
        panel(Rect::new(x - 20.0, 180.0, w + 40.0, 470.0), 0.8);
        text_field(Rect::new(x, 220.0, 300.0, 40.0), "Tu nombre", &mut self.settings.name, 1, &mut self.focus, 20, any_char);

        txt("Partidas en tu red local", x, 300.0, 24.0, CREAM);
        let found: Vec<(std::net::SocketAddr, ServerInfo)> =
            self.scanner.as_ref().map(|s| s.found.iter().map(|f| (f.0, f.1.clone())).collect()).unwrap_or_default();
        let mut chosen = None;
        if found.is_empty() {
            txt("Buscando… (las partidas de internet se añaden por dirección)", x, 334.0, 20.0, DIM);
        }
        for (k, (addr, info)) in found.iter().take(5).enumerate() {
            let r = Rect::new(x, 314.0 + k as f32 * 44.0, w, 38.0);
            let hover = r.contains(mouse_position().into());
            draw_rectangle(r.x, r.y, r.w, r.h, if hover { Color::new(0.5, 0.42, 0.25, 0.8) } else { Color::new(0.1, 0.1, 0.08, 0.8) });
            txt(&info.name, r.x + 10.0, r.y + 26.0, 22.0, CREAM);
            txt_r(&format!("{} · {}/{} · {}", info.mode.name(), info.players, info.max, addr), r.x + r.w - 10.0, r.y + 26.0, 20.0, DIM);
            if hover && is_mouse_button_pressed(MouseButton::Left) {
                chosen = Some(*addr);
            }
        }
        text_field(
            Rect::new(x, 560.0, 420.0, 40.0),
            "Dirección del servidor (IP o IP:puerto)",
            &mut self.settings.last_address,
            2,
            &mut self.focus,
            60,
            addr_char,
        );
        let go = small_button(Rect::new(x + 440.0, 560.0, 130.0, 40.0), "Conectar")
            || (self.focus == 2 && is_key_pressed(KeyCode::Enter));
        let back = small_button(Rect::new(x + w - 120.0, 560.0, 120.0, 40.0), "Volver") || is_key_pressed(KeyCode::Escape);
        if go {
            match resolve(&self.settings.last_address) {
                Some(a) => chosen = Some(a),
                None => self.screen = Screen::Error(format!("No se reconoce la dirección «{}»", self.settings.last_address)),
            }
        }
        if let Some(a) = chosen {
            self.settings.name = clean_name(&self.settings.name, 20);
            self.settings.save();
            self.scanner = None;
            self.connect_to(a);
        } else if back {
            self.settings.save();
            self.scanner = None;
            self.screen = Screen::Main;
        }
    }

    fn screen_host(&mut self) {
        self.menu_background();
        self.title();
        let cx = screen_width() / 2.0;
        let w = 720.0f32.min(screen_width() - 40.0);
        let x = cx - w / 2.0;
        panel(Rect::new(x - 20.0, 180.0, w + 40.0, 500.0), 0.8);
        text_field(Rect::new(x, 220.0, 340.0, 40.0), "Nombre de la partida", &mut self.settings.host_name, 3, &mut self.focus, 32, any_char);
        text_field(Rect::new(x + 370.0, 220.0, 150.0, 40.0), "Puerto UDP", &mut self.port_text, 4, &mut self.focus, 5, digit_char);
        let d = stepper(x, 280.0, "Jugadores máximos", &self.settings.host_max.to_string());
        self.settings.host_max = (self.settings.host_max as i32 + d).clamp(2, MAX_PLAYERS as i32) as u8;

        txt("Modos (se juegan en rotación, en este orden):", x, 350.0, 22.0, CREAM);
        for (k, m) in Mode::ALL.iter().enumerate() {
            let on = self.settings.host_modes.iter().any(|s| s == m.short());
            let r = Rect::new(x + (k % 2) as f32 * 350.0, 366.0 + (k / 2) as f32 * 46.0, 330.0, 38.0);
            let label = format!("{} {}", if on { "[x]" } else { "[ ]" }, m.name());
            if small_button(r, &label) {
                if on {
                    if self.settings.host_modes.len() > 1 {
                        self.settings.host_modes.retain(|s| s != m.short());
                    }
                } else {
                    self.settings.host_modes.push(m.short().to_string());
                }
            }
        }
        let port: Option<u16> = self.port_text.parse().ok().filter(|p| *p > 0);
        let info = format!(
            "Por internet: abre el puerto UDP {} en tu router hacia este PC y pasa tu IP pública a tus amigos.",
            port.map(|p| p.to_string()).unwrap_or_else(|| "?".into())
        );
        txt(&info, x, 492.0, 18.0, DIM);
        txt("En la misma red local te verán automáticamente en «Unirse a una partida».", x, 516.0, 18.0, DIM);
        let go = button(Rect::new(x, 560.0, 300.0, 52.0), "Crear y entrar");
        let back = button(Rect::new(x + w - 200.0, 560.0, 200.0, 52.0), "Volver") || is_key_pressed(KeyCode::Escape);
        if go {
            match port {
                Some(p) => {
                    self.settings.host_port = p;
                    self.settings.save();
                    self.start_host();
                }
                None => self.screen = Screen::Error("El puerto debe ser un número entre 1 y 65535".into()),
            }
        } else if back {
            self.settings.save();
            self.screen = Screen::Main;
        }
    }

    fn screen_options(&mut self) {
        self.menu_background();
        self.title();
        let cx = screen_width() / 2.0;
        let x = cx - 260.0;
        panel(Rect::new(x - 20.0, 180.0, 560.0, 440.0), 0.8);
        text_field(Rect::new(x, 220.0, 300.0, 40.0), "Tu nombre", &mut self.settings.name, 5, &mut self.focus, 20, any_char);
        let s = &mut self.settings;
        let d = stepper(x, 280.0, "Sensibilidad del ratón", &format!("{:.1}", s.sensitivity));
        s.sensitivity = (s.sensitivity + d as f32 * 0.2).clamp(0.2, 10.0);
        let d = stepper(x, 330.0, "Campo de visión", &format!("{:.0}°", s.fov));
        s.fov = (s.fov + d as f32 * 5.0).clamp(60.0, 90.0);
        let d = stepper(x, 380.0, "Volumen", &format!("{:.0} %", s.volume * 100.0));
        s.volume = (s.volume + d as f32 * 0.1).clamp(0.0, 1.0);
        txt("Invertir eje vertical", x, 454.0, 22.0, DIM);
        if small_button(Rect::new(x + 260.0, 430.0, 160.0, 34.0), if s.invert { "Sí" } else { "No" }) {
            s.invert = !s.invert;
        }
        self.sounds.volume = self.settings.volume;
        if button(Rect::new(x, 540.0, 260.0, 52.0), "Guardar y volver") || is_key_pressed(KeyCode::Escape) {
            self.settings.name = clean_name(&self.settings.name, 20);
            self.settings.save();
            self.screen = Screen::Main;
        }
    }

    fn screen_message(&mut self, title: &str, msg: &str, cancel: &str) -> bool {
        self.menu_background();
        self.title();
        let cx = screen_width() / 2.0;
        panel(Rect::new(cx - 360.0, 230.0, 720.0, 240.0), 0.85);
        txt_c(title, cx, 280.0, 32.0, GOLD);
        for (k, line) in msg.lines().enumerate() {
            txt_c(line, cx, 330.0 + k as f32 * 28.0, 22.0, CREAM);
        }
        button(Rect::new(cx - 120.0, 400.0, 240.0, 50.0), cancel) || is_key_pressed(KeyCode::Escape)
    }

    // ------------------------------------------------------------------
    // Partida
    // ------------------------------------------------------------------

    fn read_input(&mut self, ads: f32, typing: bool) -> FrameInput {
        let mp: Vec2 = mouse_position().into();
        let md = self.last_mouse.map(|l| mp - l).unwrap_or(Vec2::ZERO).clamp(vec2(-250.0, -250.0), vec2(250.0, 250.0));
        self.last_mouse = Some(mp);
        let mut i = FrameInput::default();
        if self.pause || self.team_menu || typing {
            return i;
        }
        let snap = self.client.as_ref().and_then(|c| c.snap.as_ref());
        let fov_k = snap
            .and_then(|_| self.client.as_ref()?.pred.as_ref().map(|p| p.gun().ads_fov / self.settings.fov))
            .unwrap_or(1.0);
        let s = self.settings.sensitivity * 0.001 * lerp(1.0, fov_k, ads);
        i.look_yaw = md.x * s;
        i.look_pitch = -md.y * s * if self.settings.invert { -1.0 } else { 1.0 };
        let k = |a: KeyCode, b: KeyCode| is_key_down(a) || is_key_down(b);
        i.fwd = k(KeyCode::W, KeyCode::Up) as i32 as f32 - k(KeyCode::S, KeyCode::Down) as i32 as f32;
        i.strafe = k(KeyCode::D, KeyCode::Right) as i32 as f32 - k(KeyCode::A, KeyCode::Left) as i32 as f32;
        if is_key_pressed(KeyCode::C) || is_key_pressed(KeyCode::LeftControl) {
            self.crouch = !self.crouch;
        }
        i.jump = is_key_pressed(KeyCode::Space);
        if i.jump {
            self.crouch = false;
        }
        i.sprint = is_key_down(KeyCode::LeftShift);
        if i.sprint && i.fwd > 0.5 {
            self.crouch = false;
        }
        i.crouch = self.crouch;
        i.fire = is_mouse_button_down(MouseButton::Left);
        i.fire_pressed = is_mouse_button_pressed(MouseButton::Left);
        i.ads = is_mouse_button_down(MouseButton::Right);
        i.reload = is_key_pressed(KeyCode::R);
        i.grenade = is_key_pressed(KeyCode::G);
        i.use_key = is_key_down(KeyCode::F);
        if is_key_pressed(KeyCode::Key1) {
            i.slot = Some(0);
        }
        if is_key_pressed(KeyCode::Key2) {
            i.slot = Some(1);
        }
        let (_, wy) = mouse_wheel();
        i.next_slot = wy.abs() > 0.1 || is_key_pressed(KeyCode::Q);
        i
    }

    fn game_frame(&mut self, dt: f32) {
        // Teclas de interfaz
        let typing = self.chat.is_some();
        if typing {
            let (mut text, team) = self.chat.take().unwrap();
            while let Some(c) = get_char_pressed() {
                if !c.is_control() && text.chars().count() < 100 {
                    text.push(c);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                text.pop();
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                if let Some(c) = self.client.as_mut() {
                    c.send_chat(&text, team);
                }
            } else if !is_key_pressed(KeyCode::Escape) {
                self.chat = Some((text, team));
            }
        } else if !self.pause && !self.team_menu {
            if is_key_pressed(KeyCode::T) {
                while get_char_pressed().is_some() {}
                self.chat = Some((String::new(), false));
            } else if is_key_pressed(KeyCode::Y) {
                while get_char_pressed().is_some() {}
                self.chat = Some((String::new(), true));
            } else if is_key_pressed(KeyCode::M) {
                self.team_menu = true;
                self.grab(false);
            } else if is_key_pressed(KeyCode::Escape) {
                self.pause = true;
                self.grab(false);
            }
        } else if is_key_pressed(KeyCode::Escape) {
            self.pause = false;
            self.team_menu = false;
            self.last_mouse = None;
            self.grab(true);
        }

        let ads = self.client.as_ref().and_then(|c| c.pred.as_ref().map(|p| p.ads)).unwrap_or(0.0);
        let inp = self.read_input(ads, self.chat.is_some());
        let mut client = self.client.take().unwrap();
        client.update(&inp, dt, &self.world);
        if let Status::Failed(why) = client.status.clone() {
            self.client = Some(client);
            self.leave(Some(why));
            return;
        }
        self.process_events(&client, dt);
        self.client = Some(client);

        // Espectador: clic para cambiar de jugador
        let c = self.client.as_ref().unwrap();
        if !c.alive() && !self.pause && !self.team_menu && is_mouse_button_pressed(MouseButton::Left) {
            self.spectate += 1;
        }

        let cam = self.game_camera();
        self.render_game(&cam);
        set_default_camera();
        self.hud(&cam);
        if self.team_menu {
            self.draw_team_menu();
        }
        if self.pause {
            self.draw_pause();
        }
    }

    fn process_events(&mut self, c: &NetClient, dt: f32) {
        let me = c.my_id;
        let snap = c.snap.as_ref();
        let my_team = snap.map(|s| s.my_team).unwrap_or(0);
        let teams = snap.map(|s| s.mode.teams()).unwrap_or(false);
        let ear = c.eye().unwrap_or_else(|| self.my_death.map(|d| d.0 + V3::UP).unwrap_or(V3::ZERO));
        let near = |p: V3, range: f32| (1.0 - p.dist(ear) / range).clamp(0.0, 1.0);
        let team_of = |id: u8| snap.and_then(|s| s.players.iter().find(|p| p.id == id)).map(|p| p.team).unwrap_or(0);

        for fx in &c.local {
            match *fx {
                LocalFx::Shot { gun } => {
                    self.sounds.play(&self.sounds.guns[gun as usize % 4], 0.75);
                    if gun == 2 {
                        self.sounds.play(&self.sounds.bolt, 0.35);
                    }
                }
                LocalFx::Reload { .. } => self.sounds.play(&self.sounds.reload, 0.6),
                LocalFx::Ping => self.sounds.play(&self.sounds.ping, 0.7),
                LocalFx::Dry => self.sounds.play(&self.sounds.dry, 0.6),
                LocalFx::Throw => self.sounds.play(&self.sounds.throw, 0.6),
                LocalFx::Switch => self.sounds.play(&self.sounds.switch, 0.5),
            }
        }

        for e in &c.events {
            match e {
                Ev::Shot { shooter, from, to, gun } => {
                    self.tracers.push((*from, *to, 0.05));
                    spray(&mut self.fx, &mut self.rng, *to, 5, [0.55, 0.5, 0.42], 2.2, 1.4, 0.07, 0.5, 9.0, 0.0, 1.0);
                    if *shooter != me {
                        self.flash.insert(*shooter, 0.05);
                        self.fired.insert(*shooter, 0.0);
                        let d = from.dist(ear);
                        let g = *gun as usize % 4;
                        if d < 25.0 {
                            self.sounds.play(&self.sounds.guns[g], 0.55 * near(*from, 60.0).max(0.3));
                        } else {
                            let far = if GUNS[g].auto { &self.sounds.far_smg } else { &self.sounds.far_rifle };
                            self.sounds.play(far, 0.6 * near(*from, 160.0).max(0.08));
                        }
                    }
                }
                Ev::Hit { shooter } if *shooter == me => {
                    self.hitmarker = 0.2;
                    self.sounds.play(&self.sounds.hit, 0.6);
                }
                Ev::Hit { .. } => {}
                Ev::Hurt { victim, from } => {
                    if *victim == me {
                        self.indicators.push((*from, 1.6));
                        self.shake = (self.shake + 0.15).min(1.0);
                        self.sounds.play(&self.sounds.hurt, 0.7);
                    } else if let Some(p) = c.remotes().iter().find(|p| p.id == *victim) {
                        spray(&mut self.fx, &mut self.rng, p.pos + V3::UP * 1.2, 8, [0.55, 0.05, 0.04], 2.0, 1.0, 0.06, 0.5, 9.0, 0.0, 1.0);
                    }
                }
                Ev::Kill { killer, victim, weapon, head, killer_name, victim_name } => {
                    self.feed.push(FeedLine {
                        killer: killer_name.clone(),
                        kteam: team_of(*killer),
                        victim: victim_name.clone(),
                        vteam: team_of(*victim),
                        weapon: *weapon,
                        head: *head,
                        ttl: 6.0,
                    });
                    if self.feed.len() > 5 {
                        self.feed.remove(0);
                    }
                    if *victim == me {
                        self.killed_by = if killer != victim { Some(killer_name.clone()) } else { None };
                    }
                    if *killer == me && *victim != me {
                        let t = if *head { format!("¡Disparo a la cabeza a {victim_name}!") } else { format!("Has abatido a {victim_name}") };
                        self.announces.push((t, 99, 2.5));
                    }
                }
                Ev::Explosion { pos, big } => {
                    let k = if *big { 2.0 } else { 1.0 };
                    spray(&mut self.fx, &mut self.rng, *pos, (14.0 * k) as usize, [1.0, 0.62, 0.2], 7.0 * k, 2.0, 0.35 * k, 0.35, 0.0, 1.5, 0.95);
                    spray(&mut self.fx, &mut self.rng, *pos, (26.0 * k) as usize, [0.32, 0.31, 0.29], 3.0 * k, 2.5, 0.7 * k, 2.6, -0.4, 1.3, 0.75);
                    spray(&mut self.fx, &mut self.rng, *pos, (22.0 * k) as usize, [0.35, 0.28, 0.18], 9.0, 6.0, 0.12, 1.2, 16.0, 0.0, 1.0);
                    self.sounds.play(&self.sounds.explosion, near(*pos, 150.0 * k).max(0.1));
                    self.shake = (self.shake + (1.0 - pos.dist(ear) / (30.0 * k)).max(0.0)).min(1.2);
                }
                Ev::Bounce { pos } => self.sounds.play(&self.sounds.bounce, 0.5 * near(*pos, 30.0)),
                Ev::Throw { who, pos } if *who != me => self.sounds.play(&self.sounds.throw, 0.4 * near(*pos, 25.0)),
                Ev::Reload { who, pos } if *who != me => self.sounds.play(&self.sounds.reload, 0.35 * near(*pos, 20.0)),
                Ev::Ping { who, pos } if *who != me => self.sounds.play(&self.sounds.ping, 0.6 * near(*pos, 40.0)),
                Ev::Throw { .. } | Ev::Reload { .. } | Ev::Ping { .. } => {}
                Ev::Chat { from, team_only, name, text } => {
                    let t = team_of(*from);
                    if *team_only && (!teams || t != my_team) {
                        continue;
                    }
                    let col = if teams { team_color(t) } else { CREAM };
                    let prefix = if *team_only { "(equipo) " } else { "" };
                    self.chat_log.push((format!("{prefix}{name}: {text}"), col, 10.0));
                    if self.chat_log.len() > 8 {
                        self.chat_log.remove(0);
                    }
                }
                Ev::Announce { text, tone } => {
                    self.announces.push((text.clone(), *tone, 4.5));
                    if self.announces.len() > 4 {
                        self.announces.remove(0);
                    }
                }
                Ev::Whistle => self.sounds.play(&self.sounds.whistle, 0.6),
            }
        }

        // Cadáveres y animación de pasos de los demás
        let remotes = c.remotes();
        for p in &remotes {
            let was = self.alive_prev.insert(p.id, p.alive).unwrap_or(p.alive);
            if was && !p.alive {
                self.corpses.push(Corpse { id: p.id, pos: p.pos, yaw: p.yaw, side: p.side, t: 0.0 });
            }
            if p.alive {
                self.corpses.retain(|k| k.id != p.id);
            }
            let e = self.walk.entry(p.id).or_insert((p.pos, 0.0));
            e.1 += e.0.dist_flat(p.pos) * 2.6;
            e.0 = p.pos;
        }
        // Mi propia muerte
        match (c.pred.as_ref(), self.my_death) {
            (None, None) => {
                if let Some(info) = c.me_info() {
                    if c.snap.is_some() && self.alive_prev.get(&me).copied().unwrap_or(false) {
                        self.my_death = Some((info.pos, c.view_yaw, 0.0));
                    }
                }
            }
            (Some(_), Some(_)) => {
                self.my_death = None;
                self.killed_by = None;
            }
            _ => {}
        }
        self.alive_prev.insert(me, c.pred.is_some());
        if let Some(d) = self.my_death.as_mut() {
            d.2 += dt;
        }

        // Temporizadores
        for k in self.corpses.iter_mut() {
            k.t += dt;
        }
        self.corpses.retain(|k| k.t < 12.0);
        for f in self.fx.iter_mut() {
            f.vel.y -= f.gravity * dt;
            f.pos += f.vel * dt;
            if f.pos.y < 0.02 {
                f.pos.y = 0.02;
                f.vel *= 0.3;
            }
            f.size += f.grow * dt;
            f.ttl -= dt;
        }
        self.fx.retain(|f| f.ttl > 0.0);
        for t in self.tracers.iter_mut() {
            t.2 -= dt;
        }
        self.tracers.retain(|t| t.2 > 0.0);
        for i in self.indicators.iter_mut() {
            i.1 -= dt;
        }
        self.indicators.retain(|i| i.1 > 0.0);
        for f in self.feed.iter_mut() {
            f.ttl -= dt;
        }
        self.feed.retain(|f| f.ttl > 0.0);
        for l in self.chat_log.iter_mut() {
            l.2 -= dt;
        }
        for a in self.announces.iter_mut() {
            a.2 -= dt;
        }
        self.announces.retain(|a| a.2 > 0.0);
        for v in self.fired.values_mut() {
            *v += dt;
        }
        for v in self.flash.values_mut() {
            *v -= dt;
        }
        self.hitmarker = (self.hitmarker - dt).max(0.0);
        self.shake = (self.shake - dt * 1.8).max(0.0);
    }

    fn game_camera(&self) -> Cam {
        let c = self.client.as_ref().unwrap();
        let t = self.clock;
        let snap = c.snap.as_ref();
        if snap.map(|s| s.phase == Phase::Intermission).unwrap_or(false) {
            let a = t * 0.06;
            let eye = v3(a.cos() * 50.0, 22.0, a.sin() * 50.0);
            return Cam { eye, fwd: (v3(0.0, 2.0, 4.0) - eye).norm(), up: V3::UP, fov: 55.0 };
        }
        if let (Some(p), Some(eye)) = (c.pred.as_ref(), c.eye()) {
            let fwd = V3::from_yaw_pitch(c.view_yaw, c.view_pitch);
            let right = V3::right_of_yaw(c.view_yaw);
            let mut e = eye;
            if self.shake > 0.0 {
                let s = self.shake * 0.05;
                e += V3::UP * ((t * 41.0).sin() * s) + right * ((t * 37.0).cos() * s);
            }
            let fov = lerp(self.settings.fov, p.gun().ads_fov, p.ads);
            return Cam { eye: e, fwd, up: V3::UP, fov };
        }
        // Muerto: en Buscar y destruir se sigue a un compañero; si no, cámara de muerte
        if let Some(s) = snap {
            let sd_wait = s.mode == Mode::Sd && (s.phase == Phase::Playing || s.phase == Phase::RoundEnd);
            let watch = self.my_death.map(|d| d.2 > 2.5).unwrap_or(true);
            if sd_wait && watch {
                let mates: Vec<PlayerInfo> = c.remotes().into_iter().filter(|p| p.alive && p.team == s.my_team).collect();
                if !mates.is_empty() {
                    let m = &mates[self.spectate % mates.len()];
                    let head = m.pos + V3::UP * STAND_EYE;
                    let back = -V3::from_yaw(m.yaw);
                    let dir = (back + V3::UP * 0.25).norm();
                    let dist = self.world.raycast(head, dir, 3.2).map(|h| (h.t - 0.3).max(0.3)).unwrap_or(3.2);
                    let eye = head + dir * dist;
                    return Cam { eye, fwd: (head + V3::from_yaw(m.yaw) * 4.0 - eye).norm(), up: V3::UP, fov: self.settings.fov };
                }
            }
        }
        match self.my_death {
            Some((pos, yaw, dt)) => {
                let k = (dt / 1.2).min(1.0);
                let eye = pos + V3::UP * lerp(STAND_EYE, 0.3, k);
                let roll = 0.55 * k;
                let right = V3::right_of_yaw(yaw);
                Cam { eye, fwd: V3::from_yaw_pitch(yaw, -0.1 * k), up: V3::UP * roll.cos() + right * roll.sin(), fov: self.settings.fov }
            }
            None => {
                let base = c.me_info().map(|p| p.pos).unwrap_or(v3(0.0, 0.0, 0.0));
                let eye = base + v3(0.0, 30.0, -25.0);
                Cam { eye, fwd: (base - eye).norm(), up: V3::UP, fov: 60.0 }
            }
        }
    }

    fn begin_3d(&self, cam: &Cam) {
        clear_background(Color::new(SKY[0], SKY[1], SKY[2], 1.0));
        set_camera(&Camera3D {
            position: tv(cam.eye),
            target: tv(cam.eye + cam.fwd),
            up: tv(cam.up),
            fovy: cam.fov.to_radians(),
            ..Default::default()
        });
        draw_geo(&self.sg, cam.eye);
    }

    fn render_game(&mut self, cam: &Cam) {
        self.begin_3d(cam);
        let c = self.client.as_ref().unwrap();
        let snap = c.snap.as_ref();
        let mut dg = std::mem::take(&mut self.dg);
        let mut pg = std::mem::take(&mut self.pg);
        dg.clear();
        pg.clear();

        if let Some(s) = snap {
            match s.mode {
                Mode::Ctf => {
                    for t in 0..2 {
                        let f = s.flags[t];
                        geo::flag(&mut dg, self.world.flag_bases[t], t as u8, self.clock, true);
                        if f.state == flag_state::DROPPED {
                            geo::flag(&mut dg, f.pos, t as u8, self.clock, false);
                        }
                    }
                }
                Mode::Sd => {
                    for site in self.world.bomb_sites.iter() {
                        geo::bomb_site(&mut dg, *site);
                    }
                    if s.bomb_state == bomb_state::PLANTED {
                        geo::planted_bomb(&mut dg, self.world.bomb_sites[s.bomb_site as usize % 2], self.clock);
                    }
                }
                _ => {}
            }
        }

        let flag_of = |id: u8| -> Option<geo::Col> {
            let s = snap?;
            (0..2).find(|&t| s.flags[t].state == flag_state::CARRIED && s.flags[t].carrier == id).map(|t| geo::FLAG_COLORS[t])
        };
        for p in c.remotes().iter().filter(|p| p.alive) {
            let look = SoldierLook {
                side: p.side,
                smg: p.slot == 1,
                crouch: p.crouch,
                pitch: p.pitch,
                flash: 0.0,
                has_flag: if p.has_flag { flag_of(p.id) } else { None },
            };
            let walk = self.walk.get(&p.id).map(|w| w.1).unwrap_or(0.0);
            geo::soldier(&mut dg, p.pos, p.yaw, None, walk, p.moving, &look);
            if self.flash.get(&p.id).copied().unwrap_or(-1.0) > 0.0 {
                let f = V3::from_yaw_pitch(p.yaw, p.pitch);
                let m = p.pos + V3::UP * (lerp(1.3, 0.85, p.crouch)) + f * 1.05 + V3::right_of_yaw(p.yaw) * 0.08;
                dg.cube(m, 0.14, [1.0, 0.85, 0.45, 0.9]);
            }
        }
        for k in &self.corpses {
            let look = SoldierLook { side: k.side, smg: false, crouch: 0.0, pitch: 0.0, flash: 0.0, has_flag: None };
            geo::soldier(&mut dg, k.pos, k.yaw, Some(k.t), 0.0, 0.0, &look);
        }
        for g in c.grenades() {
            geo::grenade(&mut dg, g.pos, self.clock * 8.0 + g.id as f32, g.side == 0);
        }
        if let Some(p) = c.pred.as_ref() {
            let right = V3::right_of_yaw(c.view_yaw);
            let up = right.cross(cam.fwd).norm();
            let vp = ViewParams {
                gun: gun_index(p.side, p.slot) as u8,
                ads: p.ads,
                bob: p.bob,
                moving: p.moving,
                recoil: c.recoil,
                lowered: p.lowered(),
                sprint: p.sprinting,
                muzzle: c.muzzle,
                flash_seed: self.clock,
            };
            geo::viewmodel(&mut dg, cam.eye, cam.fwd, right, up, &vp);
        }
        draw_geo(&dg, cam.eye);
        for f in &self.fx {
            let a = f.alpha * (f.ttl / f.max).clamp(0.0, 1.0);
            pg.cube(f.pos, f.size, [f.color[0], f.color[1], f.color[2], a]);
        }
        draw_geo(&pg, cam.eye);
        for (a, b, t) in &self.tracers {
            draw_line_3d(tv(*a), tv(*b), Color::new(1.0, 0.88, 0.55, 0.8 * (t / 0.05).clamp(0.0, 1.0)));
        }
        self.dg = dg;
        self.pg = pg;
    }

    // ------------------------------------------------------------------
    // HUD
    // ------------------------------------------------------------------

    fn hud(&mut self, cam: &Cam) {
        let c = self.client.as_ref().unwrap();
        let Some(s) = c.snap.as_ref() else {
            txt_c("Recibiendo datos del servidor…", screen_width() / 2.0, screen_height() / 2.0, 28.0, CREAM);
            return;
        };
        let (sw, sh) = (screen_width(), screen_height());
        let (cx, cy) = (sw / 2.0, sh / 2.0);
        let teams = s.mode.teams();
        let me = c.my_id;

        // Viñeta de daño
        if s.my_alive {
            let hurt = 1.0 - s.health / MAX_HEALTH;
            if hurt > 0.01 {
                draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.5, 0.0, 0.0, hurt * 0.18));
                for k in 0..10 {
                    let inset = k as f32 * sh * 0.018;
                    let th = sh * 0.018;
                    let col = Color::new(0.55, 0.0, 0.0, hurt * 0.4 * (1.0 - k as f32 / 10.0));
                    draw_rectangle(inset, inset, sw - 2.0 * inset, th, col);
                    draw_rectangle(inset, sh - inset - th, sw - 2.0 * inset, th, col);
                    draw_rectangle(inset, inset + th, th, sh - 2.0 * inset - 2.0 * th, col);
                    draw_rectangle(sw - inset - th, inset + th, th, sh - 2.0 * inset - 2.0 * th, col);
                }
            }
        }

        // Nombres sobre las cabezas
        let remotes = c.remotes();
        for p in remotes.iter().filter(|p| p.alive) {
            let head = p.pos + V3::UP * (lerp(2.05, 1.5, p.crouch));
            let mate = teams && p.team == s.my_team;
            let Some(sp) = project(cam, head) else { continue };
            let dist = head.dist(cam.eye);
            if mate && dist < 80.0 {
                txt_c(&p.name, sp.x, sp.y, 18.0, with_alpha(ALLIED_C, 0.9));
            } else if !mate && (sp - vec2(cx, cy)).length() < 40.0 && self.world.los(cam.eye, head - V3::UP * 0.3) {
                txt_c(&p.name, sp.x, sp.y, 18.0, AXIS_C);
            }
        }
        // Objetivos en pantalla
        if s.mode == Mode::Sd {
            for (k, site) in self.world.bomb_sites.iter().enumerate() {
                if let Some(sp) = project(cam, *site + V3::UP * 2.6) {
                    let planted = s.bomb_state == bomb_state::PLANTED && s.bomb_site as usize == k;
                    let col = if planted { Color::new(1.0, 0.3, 0.2, 1.0) } else { GOLD };
                    txt_c(if k == 0 { "A" } else { "B" }, sp.x, sp.y, 30.0, col);
                }
            }
        }
        if s.mode == Mode::Ctf {
            for t in 0..2 {
                let f = s.flags[t];
                if f.state == flag_state::CARRIED && f.carrier == me {
                    continue;
                }
                if let Some(sp) = project(cam, f.pos + V3::UP * 3.2) {
                    let label = if t as u8 == s.my_team { "Tu bandera" } else { "Bandera enemiga" };
                    txt_c(label, sp.x, sp.y, 18.0, team_color(t as u8));
                }
            }
        }

        if let Some(p) = c.pred.as_ref() {
            // Retícula
            if p.ads < 0.4 && !p.sprinting && !p.busy {
                let gap = 5.0 + p.spread() / (cam.fov.to_radians() * 0.5).tan() * sh * 0.5;
                let len = 9.0;
                let col = Color::new(1.0, 1.0, 1.0, 0.85 * (1.0 - p.ads * 2.0).max(0.0));
                draw_line(cx - gap - len, cy, cx - gap, cy, 2.0, col);
                draw_line(cx + gap, cy, cx + gap + len, cy, 2.0, col);
                draw_line(cx, cy - gap - len, cx, cy - gap, 2.0, col);
                draw_line(cx, cy + gap, cx, cy + gap + len, 2.0, col);
            }
            if self.hitmarker > 0.0 {
                let col = Color::new(1.0, 1.0, 1.0, self.hitmarker / 0.2);
                for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    draw_line(cx + sx * 6.0, cy + sy * 6.0, cx + sx * 14.0, cy + sy * 14.0, 2.0, col);
                }
            }
            // Indicadores de daño
            let f = V3::from_yaw(c.view_yaw);
            let r = V3::right_of_yaw(c.view_yaw);
            let ang_to = |from: V3| {
                let d = (from - p.pos).flat();
                d.dot(r).atan2(d.dot(f))
            };
            for (from, ttl) in &self.indicators {
                let ang = ang_to(*from);
                let d = vec2(ang.sin(), -ang.cos());
                let perp = vec2(ang.cos(), ang.sin());
                let base = vec2(cx, cy) + d * (sh * 0.17);
                draw_triangle(base + perp * 30.0, base - perp * 30.0, vec2(cx, cy) + d * (sh * 0.17 + 28.0), Color::new(0.85, 0.05, 0.05, 0.75 * (ttl / 1.6).min(1.0)));
            }
            // Granadas cercanas (también las propias: también hacen daño)
            let mut warn = false;
            for g in c.grenades() {
                if g.pos.dist(p.pos) < 8.0 {
                    warn = true;
                    let ang = ang_to(g.pos);
                    let dir = vec2(ang.sin(), -ang.cos());
                    let at = vec2(cx, cy) + dir * (sh * 0.12);
                    draw_circle(at.x, at.y, 15.0, Color::new(0.1, 0.1, 0.1, 0.75));
                    draw_circle_lines(at.x, at.y, 15.0, 2.0, GOLD);
                    txt_c("!", at.x, at.y + 8.0, 26.0, GOLD);
                }
            }
            if warn && (self.clock * 6.0).sin() > -0.3 {
                txt_c("¡GRANADA!", cx, sh * 0.3, 34.0, GOLD);
            }
            if p.reload_t > 0.0 {
                let prog = 1.0 - p.reload_t / p.reload_total;
                txt_c("Recargando", cx, cy + 58.0, 20.0, CREAM);
                draw_rectangle(cx - 70.0, cy + 66.0, 140.0, 5.0, Color::new(0.0, 0.0, 0.0, 0.5));
                draw_rectangle(cx - 70.0, cy + 66.0, 140.0 * prog, 5.0, CREAM);
            }
            // Munición
            let def = p.gun();
            let a = p.ammo[p.slot as usize];
            let (x, y) = (sw - 40.0, sh - 40.0);
            let small = format!(" / {}", a.reserve);
            let ws = text_w(&small, 28.0);
            let big = a.mag.to_string();
            let low = a.mag * 4 <= def.mag;
            txt(&big, x - ws - text_w(&big, 54.0), y, 54.0, if low { Color::new(1.0, 0.45, 0.35, 1.0) } else { CREAM });
            txt(&small, x - ws, y, 28.0, DIM);
            txt_r(def.name, x, y - 50.0, 24.0, CREAM);
            for k in 0..p.grenades {
                let gx = x - 10.0 - k as f32 * 22.0;
                draw_circle(gx, y - 88.0, 7.0, Color::new(0.3, 0.36, 0.2, 1.0));
                draw_circle_lines(gx, y - 88.0, 7.0, 1.5, CREAM);
            }
            if a.mag == 0 && a.reserve == 0 {
                txt_c("Sin munición: cambia de arma (Q)", cx, cy + 90.0, 20.0, Color::new(1.0, 0.6, 0.4, 1.0));
            }
            // Pista de objetivo
            let mut hint: Option<String> = None;
            if s.mode == Mode::Sd && s.phase == Phase::Playing {
                let at_site = |k: usize| self.world.bomb_sites[k].dist_flat(p.pos) < SITE_RADIUS;
                if s.bomb_state == bomb_state::NONE && s.my_team == s.attackers && (at_site(0) || at_site(1)) {
                    hint = Some("Mantén F para colocar la bomba".into());
                }
                if s.bomb_state == bomb_state::PLANTED && s.my_team != s.attackers && at_site(s.bomb_site as usize % 2) {
                    hint = Some("Mantén F para desactivar la bomba".into());
                }
            }
            if s.mode == Mode::Ctf && s.flags.iter().any(|f| f.state == flag_state::CARRIED && f.carrier == me) {
                hint = Some("¡Llevas la bandera! Vuelve a tu base".into());
            }
            if s.progress > 0.0 {
                let w = 260.0;
                draw_rectangle(cx - w / 2.0, cy + 100.0, w, 12.0, Color::new(0.0, 0.0, 0.0, 0.6));
                draw_rectangle(cx - w / 2.0, cy + 100.0, w * s.progress.min(1.0), 12.0, GOLD);
            }
            if let Some(h) = hint {
                txt_c(&h, cx, cy + 140.0, 24.0, GOLD);
            }
            let stance = if p.crouching { "Agachado" } else if p.sprinting { "Corriendo" } else { "De pie" };
            txt(stance, 185.0, sh - 40.0, 20.0, DIM);
        } else if s.phase != Phase::Intermission {
            // Muerto
            let line = if s.mode == Mode::Sd && s.phase == Phase::Playing {
                "Estás fuera de combate hasta la próxima ronda · Clic: cambiar de cámara".to_string()
            } else if s.respawn_in > 0.0 {
                format!("Reaparecerás en {:.0}", s.respawn_in.ceil())
            } else {
                "Esperando…".to_string()
            };
            txt_c(&line, cx, sh * 0.72, 26.0, CREAM);
            if let Some(k) = &self.killed_by {
                txt_c(&format!("Te ha abatido {k}"), cx, sh * 0.72 + 32.0, 22.0, AXIS_C);
            }
        }

        self.hud_top(s, me);
        self.hud_compass(c, s);

        // Muertes (arriba a la derecha)
        let mut y = 100.0;
        for f in &self.feed {
            let a = (f.ttl / 0.6).min(1.0);
            let w_name = match f.weapon {
                0..=3 => GUNS[f.weapon as usize].name,
                weapon::GRENADE => "granada",
                weapon::BOMB => "bomba",
                _ => "",
            };
            let mid = if f.weapon == weapon::SUICIDE {
                " se ha suicidado".to_string()
            } else {
                format!(" [{}{}] ", w_name, if f.head { " · cabeza" } else { "" })
            };
            let kc = if teams { team_color(f.kteam) } else { CREAM };
            let vc = if teams { team_color(f.vteam) } else { CREAM };
            if f.weapon == weapon::SUICIDE || f.killer.is_empty() {
                let x = sw - 20.0 - text_w(&f.victim, 20.0) - text_w(&mid, 20.0);
                txt(&f.victim, x, y, 20.0, with_alpha(vc, a));
                txt(&mid, x + text_w(&f.victim, 20.0), y, 20.0, with_alpha(DIM, a));
            } else {
                let total = text_w(&f.killer, 20.0) + text_w(&mid, 20.0) + text_w(&f.victim, 20.0);
                let x = sw - 20.0 - total;
                txt(&f.killer, x, y, 20.0, with_alpha(kc, a));
                txt(&mid, x + text_w(&f.killer, 20.0), y, 20.0, with_alpha(DIM, a));
                txt(&f.victim, x + text_w(&f.killer, 20.0) + text_w(&mid, 20.0), y, 20.0, with_alpha(vc, a));
            }
            y += 24.0;
        }

        // Avisos centrales
        for (k, (text, tone_id, ttl)) in self.announces.iter().enumerate() {
            let a = (ttl / 0.6).min(1.0);
            let col = match *tone_id {
                tone::ALLIES => ALLIED_C,
                tone::AXIS => AXIS_C,
                tone::ALERT => GOLD,
                99 => CREAM,
                _ => DIM,
            };
            txt_c(text, cx, sh * 0.2 + k as f32 * 28.0, 22.0, with_alpha(col, a));
        }

        // Chat
        let base_y = sh - 200.0;
        let typing = self.chat.is_some();
        let lines: Vec<&(String, Color, f32)> = self.chat_log.iter().filter(|l| typing || l.2 > 0.0).collect();
        for (k, (text, col, ttl)) in lines.iter().rev().take(6).enumerate() {
            let a = if typing { 1.0 } else { (ttl / 1.0).min(1.0) };
            txt(text, 20.0, base_y - k as f32 * 22.0, 20.0, with_alpha(*col, a));
        }
        if let Some((text, team)) = &self.chat {
            let label = if *team { "Equipo: " } else { "Todos: " };
            draw_rectangle(16.0, base_y + 10.0, 560.0, 30.0, Color::new(0.0, 0.0, 0.0, 0.6));
            let caret = if (self.clock * 2.0) as i64 % 2 == 0 { "|" } else { "" };
            txt(&format!("{label}{text}{caret}"), 24.0, base_y + 32.0, 20.0, CREAM);
        }

        // Marcador
        if s.phase == Phase::Intermission || (is_key_down(KeyCode::Tab) && !typing) {
            self.scoreboard(s, me);
        }

        let ping = c.me_info().map(|p| p.ping).unwrap_or(0);
        txt_r(&format!("{} fps · {} ms", get_fps(), ping), sw - 10.0, 22.0, 16.0, Color::new(1.0, 1.0, 1.0, 0.45));
        if !typing && !self.pause && !self.team_menu && s.phase != Phase::Intermission {
            let help = if teams { "Tab: marcador · T/Y: chat · M: equipo · Esc: menú" } else { "Tab: marcador · T: chat · Esc: menú" };
            txt(help, 20.0, 24.0, 14.0, Color::new(1.0, 1.0, 1.0, 0.35));
        }
    }

    fn hud_top(&self, s: &Snapshot, me: u8) {
        let cx = screen_width() / 2.0;
        let t = s.timer.max(0.0) as u32;
        let clock = format!("{}:{:02}", t / 60, t % 60);
        match s.phase {
            Phase::Warmup => {
                let msg = if s.timer > 0.0 {
                    format!("La partida empieza en {:.0}", s.timer.ceil())
                } else if s.mode.teams() {
                    "Calentamiento: hace falta al menos un jugador en cada equipo".to_string()
                } else {
                    "Calentamiento: esperando a más jugadores".to_string()
                };
                txt_c(s.mode.name(), cx, 34.0, 24.0, CREAM);
                txt_c(&msg, cx, 62.0, 20.0, GOLD);
                return;
            }
            Phase::RoundEnd => txt_c(&format!("Siguiente ronda en {:.0}", s.timer.ceil()), cx, 92.0, 20.0, GOLD),
            _ => {}
        }
        if s.mode.teams() {
            let left = format!("Aliados {}", s.team_scores[0]);
            let right = format!("{} Eje", s.team_scores[1]);
            txt_r(&left, cx - 50.0, 38.0, 30.0, ALLIED_C);
            txt(&right, cx + 50.0, 38.0, 30.0, AXIS_C);
            let mid = if s.mode == Mode::Sd && s.bomb_state == bomb_state::PLANTED {
                format!("{:.0}", s.bomb_timer.ceil())
            } else {
                clock
            };
            let col = if s.mode == Mode::Sd && s.bomb_state == bomb_state::PLANTED { Color::new(1.0, 0.3, 0.2, 1.0) } else { CREAM };
            txt_c(&mid, cx, 38.0, 26.0, col);
            let sub = match s.mode {
                Mode::Sd => format!(
                    "Ronda {} · atacan los {} · gana quien llegue a {}{}",
                    s.round,
                    team_name(s.attackers),
                    s.score_limit,
                    if s.bomb_state == bomb_state::PLANTED { " · ¡BOMBA COLOCADA!" } else { "" }
                ),
                Mode::Ctf => format!("Captura la bandera · {} capturas para ganar", s.score_limit),
                _ => format!("Por equipos · {} bajas para ganar", s.score_limit),
            };
            txt_c(&sub, cx, 64.0, 18.0, DIM);
            let mine = format!("Tu equipo: {}", team_name(s.my_team));
            txt_c(&mine, cx, 84.0, 16.0, team_color(s.my_team));
        } else {
            let mut ps: Vec<&PlayerInfo> = s.players.iter().collect();
            ps.sort_by_key(|p| -(p.score as i32));
            let my = s.players.iter().find(|p| p.id == me).map(|p| p.score).unwrap_or(0);
            let lead = ps.first().map(|p| format!("Líder: {} ({})", p.name, p.score)).unwrap_or_default();
            txt_c(&format!("Tú: {my}   ·   {lead}   ·   {clock}"), cx, 38.0, 24.0, CREAM);
            txt_c(&format!("Todos contra todos · {} bajas para ganar", s.score_limit), cx, 62.0, 18.0, DIM);
        }
    }

    fn hud_compass(&self, c: &NetClient, s: &Snapshot) {
        let sh = screen_height();
        let (bx, by, br) = (100.0, sh - 100.0, 72.0);
        draw_circle(bx, by, br, Color::new(0.0, 0.0, 0.0, 0.35));
        draw_circle_lines(bx, by, br, 2.0, Color::new(1.0, 1.0, 1.0, 0.45));
        let center = c
            .pred
            .as_ref()
            .map(|p| p.pos)
            .or_else(|| self.my_death.map(|d| d.0))
            .unwrap_or(V3::ZERO);
        let f = V3::from_yaw(c.view_yaw);
        let r = V3::right_of_yaw(c.view_yaw);
        let scale = br / 45.0;
        let to = |d: V3| {
            let v = vec2(d.dot(r), -d.dot(f)) * scale;
            if v.length() > br - 6.0 {
                v.normalize() * (br - 6.0)
            } else {
                v
            }
        };
        let n = to(V3::Z * 1000.0);
        txt_c("N", bx + n.x, by + n.y + 7.0, 20.0, CREAM);
        let teams = s.mode.teams();
        for p in c.remotes().iter().filter(|p| p.alive) {
            let v = to(p.pos - center);
            if teams && p.team == s.my_team {
                draw_circle(bx + v.x, by + v.y, 4.0, ALLIED_C);
            } else if let Some(t) = self.fired.get(&p.id) {
                if *t < 2.5 {
                    draw_circle(bx + v.x, by + v.y, 4.5, Color::new(0.95, 0.15, 0.1, 1.0 - t / 2.5));
                }
            }
        }
        match s.mode {
            Mode::Sd => {
                for (k, site) in self.world.bomb_sites.iter().enumerate() {
                    let v = to(*site - center);
                    txt_c(if k == 0 { "A" } else { "B" }, bx + v.x, by + v.y + 7.0, 20.0, GOLD);
                }
            }
            Mode::Ctf => {
                for t in 0..2 {
                    let v = to(s.flags[t].pos - center);
                    draw_rectangle(bx + v.x - 5.0, by + v.y - 5.0, 10.0, 10.0, team_color(t as u8));
                }
            }
            _ => {}
        }
        draw_triangle(vec2(bx, by - 9.0), vec2(bx - 6.0, by + 6.0), vec2(bx + 6.0, by + 6.0), CREAM);
    }

    fn scoreboard(&self, s: &Snapshot, me: u8) {
        let (sw, sh) = (screen_width(), screen_height());
        let w = 760.0f32.min(sw - 40.0);
        let x = sw / 2.0 - w / 2.0;
        let y0 = 120.0;
        let rows = s.players.len().max(1) as f32;
        panel(Rect::new(x - 10.0, y0 - 40.0, w + 20.0, ((rows + 4.0) * 28.0 + 60.0).min(sh - 140.0)), 0.85);
        if s.phase == Phase::Intermission {
            txt_c("FIN DE LA PARTIDA", sw / 2.0, y0 - 10.0, 30.0, GOLD);
        } else {
            txt_c(&format!("{} · {}", c_name(&self.client), s.mode.name()), sw / 2.0, y0 - 10.0, 22.0, CREAM);
        }
        let header = |y: f32| {
            txt("Jugador", x + 10.0, y, 18.0, DIM);
            txt_r("Puntos", x + w - 250.0, y, 18.0, DIM);
            txt_r("Bajas", x + w - 170.0, y, 18.0, DIM);
            txt_r("Muertes", x + w - 80.0, y, 18.0, DIM);
            txt_r("Ping", x + w - 10.0, y, 18.0, DIM);
        };
        let row = |p: &PlayerInfo, y: f32, col: Color| {
            if p.id == me {
                draw_rectangle(x, y - 19.0, w, 26.0, Color::new(1.0, 1.0, 1.0, 0.08));
            }
            let name = if p.alive || s.mode != Mode::Sd { p.name.clone() } else { format!("{} (caído)", p.name) };
            txt(&name, x + 10.0, y, 20.0, col);
            txt_r(&p.score.to_string(), x + w - 250.0, y, 20.0, CREAM);
            txt_r(&p.kills.to_string(), x + w - 170.0, y, 20.0, CREAM);
            txt_r(&p.deaths.to_string(), x + w - 80.0, y, 20.0, CREAM);
            txt_r(&p.ping.to_string(), x + w - 10.0, y, 20.0, DIM);
        };
        let mut y = y0 + 24.0;
        if s.mode.teams() {
            for t in [0u8, 1u8] {
                let mut ps: Vec<&PlayerInfo> = s.players.iter().filter(|p| p.team == t).collect();
                ps.sort_by_key(|p| (-(p.score as i32), -(p.kills as i32)));
                txt(&format!("{} — {}", team_name(t), s.team_scores[t as usize]), x + 10.0, y, 24.0, team_color(t));
                y += 26.0;
                header(y);
                y += 26.0;
                for p in ps {
                    row(p, y, team_color(t));
                    y += 26.0;
                }
                y += 12.0;
            }
        } else {
            let mut ps: Vec<&PlayerInfo> = s.players.iter().collect();
            ps.sort_by_key(|p| (-(p.score as i32), -(p.kills as i32)));
            header(y);
            y += 26.0;
            for p in ps {
                row(p, y, CREAM);
                y += 26.0;
            }
        }
        if s.phase == Phase::Intermission {
            txt_c(&format!("Siguiente partida en {:.0} s", s.timer.ceil()), sw / 2.0, y + 20.0, 20.0, DIM);
        }
    }

    fn draw_team_menu(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.4));
        let cx = sw / 2.0;
        let teams = self.client.as_ref().and_then(|c| c.snap.as_ref()).map(|s| s.mode.teams()).unwrap_or(false);
        panel(Rect::new(cx - 200.0, sh * 0.3 - 50.0, 400.0, 300.0), 0.9);
        txt_c("Elegir equipo", cx, sh * 0.3 - 10.0, 30.0, CREAM);
        if !teams {
            txt_c("En este modo no hay equipos", cx, sh * 0.3 + 40.0, 22.0, DIM);
        }
        let mut pick = None;
        if teams {
            if button(Rect::new(cx - 160.0, sh * 0.3 + 20.0, 320.0, 48.0), "Aliados") {
                pick = Some(1);
            }
            if button(Rect::new(cx - 160.0, sh * 0.3 + 80.0, 320.0, 48.0), "Eje") {
                pick = Some(2);
            }
        }
        let close = button(Rect::new(cx - 160.0, sh * 0.3 + 160.0, 320.0, 48.0), "Cerrar");
        if let Some(t) = pick {
            if let Some(c) = self.client.as_mut() {
                c.team_req = t;
            }
        }
        if pick.is_some() || close {
            self.team_menu = false;
            self.last_mouse = None;
            self.grab(true);
        }
    }

    fn draw_pause(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.45));
        let cx = sw / 2.0;
        panel(Rect::new(cx - 220.0, sh * 0.25 - 50.0, 440.0, 400.0), 0.9);
        txt_c("MENÚ", cx, sh * 0.25 - 10.0, 34.0, CREAM);
        let hosting = self.host.as_ref().map(|h| h.port);
        if let Some(p) = hosting {
            txt_c(&format!("Estás alojando la partida (puerto UDP {p})"), cx, sh * 0.25 + 18.0, 18.0, DIM);
        }
        let back = button(Rect::new(cx - 180.0, sh * 0.25 + 40.0, 360.0, 48.0), "Volver a la partida");
        let team = button(Rect::new(cx - 180.0, sh * 0.25 + 100.0, 360.0, 48.0), "Cambiar de equipo");
        let d = stepper(cx - 210.0, sh * 0.25 + 170.0, "  Sensibilidad", &format!("{:.1}", self.settings.sensitivity));
        if d != 0 {
            self.settings.sensitivity = (self.settings.sensitivity + d as f32 * 0.2).clamp(0.2, 10.0);
            self.settings.save();
        }
        let label = if hosting.is_some() { "Cerrar la partida" } else { "Desconectar" };
        let quit = button(Rect::new(cx - 180.0, sh * 0.25 + 240.0, 360.0, 48.0), label);
        if back {
            self.pause = false;
            self.last_mouse = None;
            self.grab(true);
        }
        if team {
            self.pause = false;
            self.team_menu = true;
        }
        if quit {
            self.leave(None);
        }
    }

    fn frame(&mut self) {
        let dt = get_frame_time().min(0.1);
        self.clock += dt;
        match self.screen.clone() {
            Screen::Main => self.screen_main(),
            Screen::Join => self.screen_join(),
            Screen::Host => self.screen_host(),
            Screen::Options => self.screen_options(),
            Screen::Connecting => {
                let mut c = self.client.take().unwrap();
                c.update(&FrameInput::default(), dt, &self.world);
                let target = c.server.to_string();
                match c.status.clone() {
                    Status::Connected => {
                        self.client = Some(c);
                        self.screen = Screen::Game;
                        self.last_mouse = None;
                        self.grab(true);
                    }
                    Status::Failed(why) => {
                        self.client = Some(c);
                        self.leave(Some(why));
                    }
                    Status::Connecting => {
                        self.client = Some(c);
                        if self.screen_message("Conectando…", &format!("Contactando con {target}"), "Cancelar") {
                            self.leave(None);
                        }
                    }
                }
            }
            Screen::Game => {
                if is_mouse_button_pressed(MouseButton::Left) && !self.pause && !self.team_menu {
                    self.grab(true);
                }
                self.game_frame(dt);
            }
            Screen::Error(msg) => {
                if self.screen_message("Aviso", &msg, "Volver") {
                    self.screen = Screen::Main;
                }
            }
        }
    }
}

fn c_name(c: &Option<NetClient>) -> String {
    c.as_ref().map(|c| c.server_name.clone()).unwrap_or_default()
}

/// Convierte la geometría a mallas de macroquad (en trozos para no pasar
/// el límite de vértices por lote) aplicando niebla según la distancia.
fn draw_geo(g: &Geo, cam: V3) {
    const PER: usize = 600;
    let quads = g.quads();
    let mut q = 0;
    while q < quads {
        let n = (quads - q).min(PER);
        let mut vertices = Vec::with_capacity(n * 4);
        let mut indices: Vec<u16> = Vec::with_capacity(n * 6);
        for k in 0..n {
            let base = (k * 4) as u16;
            for j in 0..4 {
                let (p, c) = g.verts[(q + k) * 4 + j];
                let d = (p - cam).len();
                let f = ((d - 22.0) / 120.0).clamp(0.0, 0.75);
                let col = Color::new(lerp(c[0], SKY[0], f), lerp(c[1], SKY[1], f), lerp(c[2], SKY[2], f), c[3]);
                vertices.push(Vertex::new(p.x, p.y, p.z, 0.0, 0.0, col));
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        draw_mesh(&Mesh { vertices, indices, texture: None });
        q += n;
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let settings = Settings::load();
    let sounds = Sounds::load(settings.volume).await;
    let mut app = App::new(settings, sounds);
    prevent_quit();
    loop {
        if is_quit_requested() {
            app.leave(None);
            app.settings.save();
            break;
        }
        app.frame();
        next_frame().await;
    }
}

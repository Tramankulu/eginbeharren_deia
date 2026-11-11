//! Simulación compartida entre servidor y cliente.
//! El cliente ejecuta exactamente el mismo `step` para predecir su propio
//! movimiento; el servidor lo ejecuta de forma autoritativa.

use crate::math::*;
use crate::world::*;
use std::f32::consts::PI;

pub const TICK_HZ: u32 = 60;
pub const DT: f32 = 1.0 / TICK_HZ as f32;
pub const GRAVITY: f32 = 16.0;
pub const PLAYER_R: f32 = 0.33;
pub const STAND_H: f32 = 1.8;
pub const CROUCH_H: f32 = 1.2;
pub const STAND_EYE: f32 = 1.65;
pub const CROUCH_EYE: f32 = 1.05;
pub const MAX_HEALTH: f32 = 100.0;
pub const START_GRENADES: u8 = 2;

pub const ALLIES: u8 = 0;
pub const AXIS: u8 = 1;

pub fn team_name(t: u8) -> &'static str {
    if t == ALLIES {
        "Aliados"
    } else {
        "Eje"
    }
}

// ----------------------------------------------------------------------
// Naciones
// ----------------------------------------------------------------------

pub mod nation {
    pub const US: u8 = 0;
    pub const UK: u8 = 1;
    pub const RU: u8 = 2;
    pub const DE: u8 = 3;
    /// Bando de los zombis (modo Zombis)
    pub const ZOMBIE: u8 = 4;
}

pub fn nation_name(n: u8) -> &'static str {
    match n {
        nation::US => "Estados Unidos",
        nation::UK => "Reino Unido",
        nation::RU => "Unión Soviética",
        nation::DE => "Alemania",
        _ => "Zombis",
    }
}

pub fn nation_short(n: u8) -> &'static str {
    match n {
        nation::US => "eeuu",
        nation::UK => "britanicos",
        nation::RU => "rusos",
        nation::DE => "alemanes",
        _ => "zombis",
    }
}

pub fn parse_nation(s: &str) -> Option<u8> {
    match s.trim().to_lowercase().as_str() {
        "eeuu" | "us" | "usa" | "americanos" | "estadounidenses" => Some(nation::US),
        "uk" | "britanicos" | "británicos" | "ingleses" => Some(nation::UK),
        "ru" | "rusos" | "sovieticos" | "soviéticos" | "urss" => Some(nation::RU),
        _ => None,
    }
}

// ----------------------------------------------------------------------
// Armas
// ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GunKind {
    /// Fusil de cerrojo
    Rifle,
    Smg,
    /// Semiautomático
    Semi,
    /// Ametralladora ligera o fusil de asalto
    Support,
    /// Fusil con mira telescópica
    Sniper,
    Pistol,
    /// Garras (zombis)
    Melee,
    /// Ametralladora pesada montada en un nido
    Mounted,
}

pub struct GunDef {
    pub name: &'static str,
    pub kind: GunKind,
    pub nation: u8,
    pub mag: u32,
    pub start_reserve: u32,
    pub interval: f32,
    pub auto: bool,
    pub damage: f32,
    pub head_mult: f32,
    pub spread_hip: f32,
    pub spread_ads: f32,
    pub spread_move: f32,
    pub bloom: f32,
    /// Retroceso vertical (radianes por disparo)
    pub kick: f32,
    /// Retroceso lateral máximo (radianes por disparo)
    pub kick_side: f32,
    pub reload: f32,
    pub reload_empty: f32,
    /// Multiplicador de daño más allá de 35 m.
    pub falloff: f32,
    pub ads_fov: f32,
    /// Nivel necesario para usarla
    pub unlock: u8,
}

impl GunDef {
    pub fn scoped(&self) -> bool {
        self.kind == GunKind::Sniper
    }
}

macro_rules! gun {
    ($name:expr, $kind:ident, $nat:expr, mag $mag:expr, res $res:expr, int $int:expr, auto $auto:expr, dmg $dmg:expr, head $head:expr,
     hip $hip:expr, ads $ads:expr, mov $mov:expr, bloom $bloom:expr, kick $kick:expr, side $side:expr, rel $rel:expr, rele $rele:expr,
     fall $fall:expr, fov $fov:expr, lvl $lvl:expr) => {
        GunDef {
            name: $name,
            kind: GunKind::$kind,
            nation: $nat,
            mag: $mag,
            start_reserve: $res,
            interval: $int,
            auto: $auto,
            damage: $dmg,
            head_mult: $head,
            spread_hip: $hip,
            spread_ads: $ads,
            spread_move: $mov,
            bloom: $bloom,
            kick: $kick,
            kick_side: $side,
            reload: $rel,
            reload_empty: $rele,
            falloff: $fall,
            ads_fov: $fov,
            unlock: $lvl,
        }
    };
}

use nation::{DE, RU, UK, US};

/// Arsenal completo. Los cuatro primeros conservan su número de siempre.
pub const GUNS: [GunDef; 34] = [
    gun!("M1 Garand", Rifle, US, mag 8, res 48, int 0.2, auto false, dmg 105.0, head 2.0, hip 0.045, ads 0.0015, mov 0.03, bloom 0.0, kick 0.045, side 0.012, rel 2.3, rele 2.0, fall 1.0, fov 45.0, lvl 1),
    gun!("Thompson", Smg, US, mag 30, res 120, int 0.086, auto true, dmg 34.0, head 2.2, hip 0.04, ads 0.012, mov 0.025, bloom 0.004, kick 0.012, side 0.010, rel 2.6, rele 3.0, fall 0.7, fov 55.0, lvl 1),
    gun!("Kar98k", Rifle, DE, mag 5, res 30, int 1.15, auto false, dmg 125.0, head 2.0, hip 0.06, ads 0.0008, mov 0.035, bloom 0.0, kick 0.07, side 0.014, rel 2.4, rele 2.8, fall 1.0, fov 40.0, lvl 1),
    gun!("MP40", Smg, DE, mag 32, res 128, int 0.105, auto true, dmg 31.0, head 2.2, hip 0.037, ads 0.011, mov 0.024, bloom 0.0035, kick 0.011, side 0.008, rel 2.5, rele 2.9, fall 0.7, fov 55.0, lvl 1),
    gun!("Lee-Enfield", Rifle, UK, mag 10, res 50, int 0.95, auto false, dmg 120.0, head 2.0, hip 0.058, ads 0.0009, mov 0.034, bloom 0.0, kick 0.066, side 0.014, rel 2.6, rele 3.2, fall 1.0, fov 40.0, lvl 1),
    gun!("Sten", Smg, UK, mag 32, res 128, int 0.11, auto true, dmg 30.0, head 2.2, hip 0.042, ads 0.013, mov 0.026, bloom 0.004, kick 0.012, side 0.010, rel 2.4, rele 2.8, fall 0.7, fov 55.0, lvl 1),
    gun!("Mosin-Nagant", Rifle, RU, mag 5, res 30, int 1.25, auto false, dmg 130.0, head 2.0, hip 0.062, ads 0.0008, mov 0.036, bloom 0.0, kick 0.075, side 0.016, rel 2.5, rele 3.0, fall 1.0, fov 40.0, lvl 1),
    gun!("PPSh-41", Smg, RU, mag 71, res 142, int 0.067, auto true, dmg 27.0, head 2.1, hip 0.046, ads 0.016, mov 0.028, bloom 0.005, kick 0.010, side 0.012, rel 3.2, rele 3.6, fall 0.65, fov 55.0, lvl 1),
    gun!("BAR", Support, US, mag 20, res 100, int 0.13, auto true, dmg 48.0, head 2.0, hip 0.055, ads 0.009, mov 0.04, bloom 0.006, kick 0.026, side 0.012, rel 2.8, rele 3.2, fall 0.85, fov 50.0, lvl 3),
    gun!("Bren", Support, UK, mag 30, res 90, int 0.12, auto true, dmg 46.0, head 2.0, hip 0.055, ads 0.008, mov 0.04, bloom 0.006, kick 0.022, side 0.010, rel 3.0, rele 3.4, fall 0.85, fov 50.0, lvl 3),
    gun!("DP-28", Support, RU, mag 47, res 94, int 0.11, auto true, dmg 44.0, head 2.0, hip 0.06, ads 0.010, mov 0.042, bloom 0.006, kick 0.022, side 0.012, rel 3.6, rele 4.0, fall 0.85, fov 50.0, lvl 3),
    gun!("StG 44", Smg, DE, mag 30, res 120, int 0.1, auto true, dmg 40.0, head 2.0, hip 0.048, ads 0.008, mov 0.032, bloom 0.005, kick 0.019, side 0.011, rel 2.6, rele 3.0, fall 0.8, fov 50.0, lvl 4),
    gun!("M1 Carbine", Semi, US, mag 15, res 75, int 0.14, auto false, dmg 55.0, head 2.0, hip 0.04, ads 0.004, mov 0.028, bloom 0.004, kick 0.024, side 0.010, rel 2.0, rele 2.3, fall 0.8, fov 50.0, lvl 6),
    gun!("Gewehr 43", Semi, DE, mag 10, res 60, int 0.2, auto false, dmg 80.0, head 2.0, hip 0.048, ads 0.0025, mov 0.03, bloom 0.004, kick 0.04, side 0.012, rel 2.6, rele 3.0, fall 0.95, fov 45.0, lvl 6),
    gun!("SVT-40", Semi, RU, mag 10, res 60, int 0.19, auto false, dmg 80.0, head 2.0, hip 0.05, ads 0.0025, mov 0.03, bloom 0.004, kick 0.042, side 0.014, rel 2.6, rele 3.0, fall 0.95, fov 45.0, lvl 6),
    gun!("Springfield (mira)", Sniper, US, mag 5, res 25, int 1.3, auto false, dmg 140.0, head 3.0, hip 0.09, ads 0.0, mov 0.05, bloom 0.0, kick 0.09, side 0.012, rel 2.8, rele 3.4, fall 1.0, fov 16.0, lvl 5),
    gun!("Kar98k (mira)", Sniper, DE, mag 5, res 25, int 1.3, auto false, dmg 140.0, head 3.0, hip 0.09, ads 0.0, mov 0.05, bloom 0.0, kick 0.09, side 0.012, rel 2.8, rele 3.4, fall 1.0, fov 16.0, lvl 5),
    gun!("Lee-Enfield (mira)", Sniper, UK, mag 10, res 30, int 1.1, auto false, dmg 135.0, head 3.0, hip 0.09, ads 0.0, mov 0.05, bloom 0.0, kick 0.085, side 0.012, rel 3.0, rele 3.6, fall 1.0, fov 16.0, lvl 5),
    gun!("Mosin-Nagant (mira)", Sniper, RU, mag 5, res 25, int 1.4, auto false, dmg 145.0, head 3.0, hip 0.09, ads 0.0, mov 0.05, bloom 0.0, kick 0.095, side 0.014, rel 2.9, rele 3.5, fall 1.0, fov 16.0, lvl 5),
    gun!("Colt M1911", Pistol, US, mag 7, res 28, int 0.17, auto false, dmg 45.0, head 2.0, hip 0.035, ads 0.012, mov 0.02, bloom 0.006, kick 0.03, side 0.012, rel 1.6, rele 1.9, fall 0.6, fov 62.0, lvl 1),
    gun!("Webley", Pistol, UK, mag 6, res 24, int 0.28, auto false, dmg 55.0, head 2.0, hip 0.035, ads 0.012, mov 0.02, bloom 0.006, kick 0.04, side 0.014, rel 2.4, rele 2.6, fall 0.6, fov 62.0, lvl 1),
    gun!("Tokarev TT-33", Pistol, RU, mag 8, res 32, int 0.16, auto false, dmg 40.0, head 2.0, hip 0.034, ads 0.011, mov 0.02, bloom 0.006, kick 0.028, side 0.012, rel 1.6, rele 1.9, fall 0.6, fov 62.0, lvl 1),
    gun!("Luger P08", Pistol, DE, mag 8, res 32, int 0.15, auto false, dmg 40.0, head 2.0, hip 0.033, ads 0.011, mov 0.02, bloom 0.006, kick 0.026, side 0.010, rel 1.6, rele 1.9, fall 0.6, fov 62.0, lvl 1),
    gun!("Garras", Melee, nation::ZOMBIE, mag 0, res 0, int 1.0, auto false, dmg 0.0, head 1.0, hip 0.0, ads 0.0, mov 0.0, bloom 0.0, kick 0.0, side 0.0, rel 1.0, rele 1.0, fall 1.0, fov 70.0, lvl 1),
    gun!("Walther P38", Pistol, DE, mag 8, res 32, int 0.16, auto false, dmg 42.0, head 2.0, hip 0.034, ads 0.011, mov 0.02, bloom 0.006, kick 0.028, side 0.011, rel 1.7, rele 2.0, fall 0.6, fov 62.0, lvl 1),
    gun!("M3 Grease Gun", Smg, US, mag 30, res 120, int 0.133, auto true, dmg 36.0, head 2.2, hip 0.036, ads 0.010, mov 0.022, bloom 0.003, kick 0.009, side 0.007, rel 2.7, rele 3.1, fall 0.7, fov 55.0, lvl 4),
    gun!("Sten Mk V", Smg, UK, mag 32, res 128, int 0.11, auto true, dmg 31.0, head 2.2, hip 0.038, ads 0.011, mov 0.024, bloom 0.0035, kick 0.010, side 0.008, rel 2.5, rele 2.9, fall 0.7, fov 55.0, lvl 4),
    gun!("PPS-43", Smg, RU, mag 35, res 140, int 0.086, auto true, dmg 29.0, head 2.1, hip 0.04, ads 0.012, mov 0.025, bloom 0.004, kick 0.011, side 0.010, rel 2.4, rele 2.8, fall 0.68, fov 55.0, lvl 4),
    gun!("FG 42", Support, DE, mag 20, res 100, int 0.08, auto true, dmg 44.0, head 2.0, hip 0.058, ads 0.009, mov 0.04, bloom 0.007, kick 0.028, side 0.016, rel 2.7, rele 3.1, fall 0.85, fov 48.0, lvl 8),
    gun!("M1903 Springfield", Rifle, US, mag 5, res 30, int 1.1, auto false, dmg 125.0, head 2.0, hip 0.06, ads 0.0008, mov 0.035, bloom 0.0, kick 0.07, side 0.014, rel 2.4, rele 2.8, fall 1.0, fov 40.0, lvl 2),
    gun!("MG42", Mounted, DE, mag 999, res 0, int 0.05, auto true, dmg 45.0, head 2.0, hip 0.014, ads 0.008, mov 0.0, bloom 0.002, kick 0.006, side 0.008, rel 1.0, rele 1.0, fall 0.9, fov 50.0, lvl 1),
    gun!("M1919 Browning", Mounted, US, mag 999, res 0, int 0.11, auto true, dmg 48.0, head 2.0, hip 0.012, ads 0.007, mov 0.0, bloom 0.002, kick 0.005, side 0.006, rel 1.0, rele 1.0, fall 0.9, fov 50.0, lvl 1),
    gun!("Vickers", Mounted, UK, mag 999, res 0, int 0.12, auto true, dmg 48.0, head 2.0, hip 0.012, ads 0.007, mov 0.0, bloom 0.002, kick 0.005, side 0.006, rel 1.0, rele 1.0, fall 0.9, fov 50.0, lvl 1),
    gun!("Maxim", Mounted, RU, mag 999, res 0, int 0.1, auto true, dmg 48.0, head 2.0, hip 0.012, ads 0.007, mov 0.0, bloom 0.002, kick 0.005, side 0.006, rel 1.0, rele 1.0, fall 0.9, fov 50.0, lvl 1),
];

/// Ametralladora montada de cada nación.
pub fn mounted_gun(n: u8) -> u8 {
    nation_gun(if n == nation::ZOMBIE { nation::DE } else { n }, GunKind::Mounted)
}

/// Distancia a la que se puede coger una ametralladora montada o dar un culatazo.
pub const MOUNT_RANGE: f32 = 1.4;
pub const MELEE_RANGE: f32 = 1.8;
pub const MELEE_DAMAGE: f32 = 135.0;
/// Giro máximo de una ametralladora montada a cada lado (radianes)
pub const MOUNT_ARC: f32 = 1.05;

pub const CLAWS: u8 = 23;

pub fn gun(id: u8) -> &'static GunDef {
    &GUNS[(id as usize).min(GUNS.len() - 1)]
}

/// Primera arma de una nación con ese tipo.
pub fn nation_gun(n: u8, kind: GunKind) -> u8 {
    GUNS.iter().position(|g| g.nation == n && g.kind == kind).unwrap_or(0) as u8
}

/// Compatibilidad: el arma «de siempre» (fusil o subfusil) de un bando.
pub fn gun_index(side: u8, slot: u8) -> usize {
    (side.min(1) as usize) * 2 + slot.min(1) as usize
}

pub fn gun_def(side: u8, slot: u8) -> &'static GunDef {
    &GUNS[gun_index(side, slot)]
}

// ----------------------------------------------------------------------
// Clases, ventajas y rangos
// ----------------------------------------------------------------------

pub struct ClassDef {
    pub name: &'static str,
    pub desc: &'static str,
    /// Tipos de arma principal que puede llevar
    pub kinds: &'static [GunKind],
    pub grenades: u8,
}

pub const CLASSES: [ClassDef; 4] = [
    ClassDef { name: "Fusilero", desc: "Fusil de cerrojo o semiautomático: preciso a media y larga distancia", kinds: &[GunKind::Rifle, GunKind::Semi], grenades: 2 },
    ClassDef { name: "Asalto", desc: "Subfusil: domina el cuerpo a cuerpo y los interiores", kinds: &[GunKind::Smg], grenades: 2 },
    ClassDef { name: "Apoyo", desc: "Ametralladora ligera: fuego de supresión, lenta de recargar", kinds: &[GunKind::Support], grenades: 1 },
    ClassDef { name: "Francotirador", desc: "Fusil con mira telescópica: un disparo, una baja", kinds: &[GunKind::Sniper], grenades: 1 },
];

pub mod perk {
    pub const NONE: u8 = 0;
    pub const GRENADIER: u8 = 1;
    pub const LUNGS: u8 = 2;
    pub const BANDOLIER: u8 = 3;
    pub const QUICK_HANDS: u8 = 4;
    pub const STEADY: u8 = 5;
    pub const TOUGH: u8 = 6;
    pub const COUNT: u8 = 7;
}

/// (nombre, descripción, nivel necesario)
pub fn perk_info(p: u8) -> (&'static str, &'static str, u8) {
    match p {
        perk::GRENADIER => ("Granadero", "Una granada más", 1),
        perk::LUNGS => ("Pulmones de acero", "Corres el doble de tiempo", 2),
        perk::BANDOLIER => ("Cartucheras", "La mitad más de munición", 4),
        perk::QUICK_HANDS => ("Manos rápidas", "Recargas un 30 % más rápido", 7),
        perk::STEADY => ("Pulso firme", "Menos dispersión al moverte y mira más quieta", 9),
        perk::TOUGH => ("Piel dura", "Las balas te hacen un 20 % menos de daño", 12),
        _ => ("Sin ventaja", "", 1),
    }
}

pub const RANKS: [&str; 20] = [
    "Soldado raso",
    "Soldado",
    "Soldado de primera",
    "Cabo",
    "Cabo primero",
    "Sargento",
    "Sargento primero",
    "Brigada",
    "Subteniente",
    "Teniente",
    "Teniente primero",
    "Capitán",
    "Comandante",
    "Teniente coronel",
    "Coronel",
    "General de brigada",
    "General de división",
    "Teniente general",
    "General",
    "Mariscal de campo",
];

/// Experiencia total necesaria para llegar al nivel `level` (1..=20).
pub fn xp_for_level(level: u8) -> u32 {
    let l = level.clamp(1, 20) as u32;
    75 * (l - 1) * l
}

pub fn level_of(xp: u32) -> u8 {
    (1..=20u8).rev().find(|&l| xp >= xp_for_level(l)).unwrap_or(1)
}

pub mod xp {
    pub const KILL: u16 = 25;
    pub const HEADSHOT: u16 = 10;
    pub const OBJECTIVE: u16 = 50;
    pub const WIN: u16 = 100;
    pub const MATCH: u16 = 40;
    pub const WAVE: u16 = 30;
    pub const MELEE: u16 = 15;
}

/// Equipamiento elegido por el jugador antes de aparecer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loadout {
    pub class: u8,
    /// Arma principal concreta (si no casa con la clase y la nación, se elige la primera válida)
    pub primary: u8,
    pub perk: u8,
    /// Pistola elegida (255 = la de siempre de la nación)
    pub secondary: u8,
}

impl Default for Loadout {
    fn default() -> Self {
        Loadout { class: 0, primary: 255, perk: perk::NONE, secondary: 255 }
    }
}

impl Loadout {
    /// Armas válidas de la clase para una nación (en orden).
    pub fn options(class: u8, nat: u8) -> Vec<u8> {
        let c = &CLASSES[(class as usize).min(CLASSES.len() - 1)];
        (0..GUNS.len() as u8).filter(|&g| GUNS[g as usize].nation == nat && c.kinds.contains(&GUNS[g as usize].kind)).collect()
    }
    /// Pistolas de una nación.
    pub fn pistols(nat: u8) -> Vec<u8> {
        (0..GUNS.len() as u8).filter(|&g| GUNS[g as usize].nation == nat && GUNS[g as usize].kind == GunKind::Pistol).collect()
    }
    /// Armas (principal, pistola) y granadas para una nación.
    pub fn resolve(&self, nat: u8) -> ([u8; 2], u8) {
        if nat == nation::ZOMBIE {
            return ([CLAWS, CLAWS], 0);
        }
        let opts = Loadout::options(self.class, nat);
        let primary = if opts.contains(&self.primary) { self.primary } else { opts.first().copied().unwrap_or(nation_gun(nat, GunKind::Rifle)) };
        let pistols = Loadout::pistols(nat);
        let pistol = if pistols.contains(&self.secondary) { self.secondary } else { nation_gun(nat, GunKind::Pistol) };
        let mut g = CLASSES[(self.class as usize).min(CLASSES.len() - 1)].grenades;
        if self.perk == perk::GRENADIER {
            g += 1;
        }
        ([primary, pistol], g)
    }
}

/// Botones de un comando. Los marcados "flanco" solo están activos en el
/// comando en que se pulsaron.
pub mod btn {
    pub const FIRE: u16 = 1;
    pub const FIRE_PRESSED: u16 = 2; // flanco
    pub const ADS: u16 = 4;
    pub const JUMP: u16 = 8; // flanco
    pub const CROUCH: u16 = 16;
    pub const SPRINT: u16 = 32;
    pub const RELOAD: u16 = 64; // flanco
    pub const GRENADE: u16 = 128; // flanco
    pub const USE: u16 = 256;
    /// Culatazo o cuchillada (flanco)
    pub const MELEE: u16 = 512;
    /// Granada de humo (flanco)
    pub const SMOKE: u16 = 1024;
    /// Coger o soltar una ametralladora montada (flanco)
    pub const MOUNT: u16 = 2048;
}

/// Un paso de entrada del jugador (uno por tick de 1/60 s).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cmd {
    pub seq: u32,
    /// Tick del servidor que el cliente estaba viendo (para compensar el lag).
    pub view_tick: u32,
    pub yaw: f32,
    pub pitch: f32,
    pub fwd: i8,
    pub strafe: i8,
    pub buttons: u16,
    pub slot: u8,
}

impl Cmd {
    pub fn has(&self, b: u16) -> bool {
        self.buttons & b != 0
    }
    pub fn fwd_f(&self) -> f32 {
        self.fwd as f32 / 127.0
    }
    pub fn strafe_f(&self) -> f32 {
        self.strafe as f32 / 127.0
    }
    pub fn axis(v: f32) -> i8 {
        (v.clamp(-1.0, 1.0) * 127.0).round() as i8
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ammo {
    pub mag: u32,
    pub reserve: u32,
}

/// Estado de un jugador que se simula (y se predice en su propio cliente).
#[derive(Clone, Debug, PartialEq)]
pub struct PState {
    pub pos: V3,
    pub vel: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub side: u8,
    pub grounded: bool,
    pub crouching: bool,
    pub crouch: f32,
    pub sprinting: bool,
    pub stamina: f32,
    pub ammo: [Ammo; 2],
    /// Armas de las dos ranuras (principal y pistola), índices de `GUNS`
    pub guns: [u8; 2],
    pub nation: u8,
    pub perk: u8,
    pub slot: u8,
    pub cooldown: f32,
    pub reload_t: f32,
    pub reload_total: f32,
    pub switch_t: f32,
    pub throw_t: f32,
    pub grenades: u8,
    pub smokes: u8,
    /// Tiempo que falta del culatazo en curso
    pub melee_t: f32,
    /// Ametralladora montada que se está usando (255 = ninguna)
    pub mounted: u8,
    /// Calor del cañón de la ametralladora montada (1 = recalentada)
    pub heat: f32,
    pub overheated: bool,
    pub ads: f32,
    pub bloom: f32,
    pub moving: f32,
    pub bob: f32,
    /// Ocupado colocando/desactivando la bomba: no se puede mover ni disparar.
    pub busy: bool,
}

impl PState {
    /// Jugador con el arma de siempre de su bando (fusil y subfusil).
    pub fn new(pos: V3, yaw: f32, side: u8) -> PState {
        let n = if side == AXIS { nation::DE } else { nation::US };
        let mut p = PState::equip(pos, yaw, side, n, &Loadout::default());
        p.guns = [gun_index(side, 0) as u8, gun_index(side, 1) as u8];
        p.ammo = [0, 1].map(|k| {
            let d = gun(p.guns[k]);
            Ammo { mag: d.mag, reserve: d.start_reserve }
        });
        p.grenades = START_GRENADES;
        p
    }

    /// Jugador con el equipamiento elegido.
    pub fn equip(pos: V3, yaw: f32, side: u8, nat: u8, lo: &Loadout) -> PState {
        let (guns, grenades) = lo.resolve(nat);
        let extra = if lo.perk == perk::BANDOLIER { 1.5 } else { 1.0 };
        let a = |k: usize| {
            let d = gun(guns[k]);
            Ammo { mag: d.mag, reserve: (d.start_reserve as f32 * extra) as u32 }
        };
        PState {
            pos,
            vel: V3::ZERO,
            yaw,
            pitch: 0.0,
            side,
            grounded: true,
            crouching: false,
            crouch: 0.0,
            sprinting: false,
            stamina: 4.0,
            ammo: [a(0), a(1)],
            guns,
            nation: nat,
            perk: lo.perk,
            slot: 0,
            cooldown: 0.0,
            reload_t: 0.0,
            reload_total: 1.0,
            switch_t: 0.0,
            throw_t: 0.0,
            grenades,
            smokes: if nat == nation::ZOMBIE { 0 } else { 1 },
            melee_t: 0.0,
            mounted: NO_TURRET,
            heat: 0.0,
            overheated: false,
            ads: 0.0,
            bloom: 0.0,
            moving: 0.0,
            bob: 0.0,
            busy: false,
        }
    }
    pub fn gun_id(&self) -> u8 {
        if self.mounted != NO_TURRET {
            return mounted_gun(self.nation);
        }
        self.guns[(self.slot as usize).min(1)]
    }
    pub fn is_mounted(&self) -> bool {
        self.mounted != NO_TURRET
    }
    pub fn max_stamina(&self) -> f32 {
        if self.perk == perk::LUNGS {
            8.0
        } else {
            4.0
        }
    }
    pub fn eye(&self) -> V3 {
        self.pos + V3::UP * lerp(STAND_EYE, CROUCH_EYE, self.crouch)
    }
    pub fn height(&self) -> f32 {
        lerp(STAND_H, CROUCH_H, self.crouch)
    }
    pub fn forward(&self) -> V3 {
        V3::from_yaw_pitch(self.yaw, self.pitch)
    }
    pub fn right(&self) -> V3 {
        V3::right_of_yaw(self.yaw)
    }
    pub fn gun(&self) -> &'static GunDef {
        gun(self.gun_id())
    }
    pub fn spread(&self) -> f32 {
        let d = self.gun();
        let mut s = lerp(d.spread_hip, d.spread_ads, self.ads)
            + self.moving * d.spread_move * (1.0 - 0.7 * self.ads) * if self.perk == perk::STEADY { 0.5 } else { 1.0 }
            + self.bloom;
        if !self.grounded {
            s += 0.06;
        }
        if self.crouching {
            s *= 0.75;
        }
        s
    }
    pub fn lowered(&self) -> f32 {
        let mut l = 0.0f32;
        if self.reload_t > 0.0 {
            let p = 1.0 - self.reload_t / self.reload_total;
            l = l.max((p * PI).sin());
        }
        if self.switch_t > 0.0 {
            l = l.max((self.switch_t / 0.5 * PI).sin().abs());
        }
        if self.throw_t > 0.0 {
            l = l.max((self.throw_t / 0.8 * PI).sin());
        }
        if self.is_mounted() {
            l = 0.0;
        }
        if self.busy {
            l = 1.0;
        }
        l
    }
}

pub const NO_TURRET: u8 = 255;

#[derive(Clone, Copy, Debug, Default)]
pub struct Outcome {
    /// (origen, dirección con dispersión aplicada)
    pub shot: Option<(V3, V3)>,
    pub throw: Option<(V3, V3)>,
    /// La granada lanzada es de humo
    pub smoke: bool,
    /// Culatazo: (ojo, dirección)
    pub melee: Option<(V3, V3)>,
    /// Ha cogido (Some(true)) o soltado (Some(false)) una ametralladora
    pub mount: Option<bool>,
    pub reload: bool,
    pub ping: bool,
    pub dry: bool,
    pub switched: bool,
    /// Retroceso vertical a aplicar a la vista (radianes).
    pub kick: f32,
}

/// Avanza un jugador un tick con un comando. Determinista salvo la dispersión.
pub fn step(world: &World, p: &mut PState, cmd: &Cmd, rng: &mut Rng) -> Outcome {
    let dt = DT;
    let mut out = Outcome::default();
    p.yaw = wrap_angle(cmd.yaw);
    p.pitch = cmd.pitch.clamp(-1.45, 1.45);
    p.bloom = (p.bloom - dt * 0.08).max(0.0);
    p.cooldown -= dt;
    p.switch_t = (p.switch_t - dt).max(0.0);
    p.throw_t = (p.throw_t - dt).max(0.0);
    p.melee_t = (p.melee_t - dt).max(0.0);
    p.heat = (p.heat - dt * 0.12).max(0.0);
    if p.overheated && p.heat < 0.35 {
        p.overheated = false;
    }

    // Ametralladora montada: coger y soltar
    if cmd.has(btn::MOUNT) && !p.busy {
        if p.is_mounted() {
            let t = world.turrets.get(p.mounted as usize).copied();
            p.mounted = NO_TURRET;
            if let Some(t) = t {
                p.pos = t.stand - V3::from_yaw(t.yaw) * 0.15;
            }
            p.switch_t = 0.3;
            out.mount = Some(false);
        } else if let Some(k) = world.turrets.iter().position(|t| t.stand.dist_flat(p.pos) < MOUNT_RANGE && (t.stand.y - p.pos.y).abs() < 0.8) {
            p.mounted = k as u8;
            p.reload_t = 0.0;
            p.crouching = false;
            p.switch_t = 0.35;
            out.mount = Some(true);
        }
    }
    if let Some(t) = world.turrets.get(p.mounted as usize).copied() {
        // Quieto detrás del arma, con el giro limitado
        p.pos = t.stand;
        p.vel = V3::ZERO;
        p.grounded = true;
        p.sprinting = false;
        p.crouch = approach(p.crouch, 0.0, dt * 7.0);
        let rel = wrap_angle(p.yaw - t.yaw).clamp(-MOUNT_ARC, MOUNT_ARC);
        p.yaw = wrap_angle(t.yaw + rel);
        p.pitch = p.pitch.clamp(-0.5, 0.45);
        p.ads = approach(p.ads, if cmd.has(btn::ADS) { 1.0 } else { 0.0 }, dt * 6.0);
        p.moving = 0.0;
        let def = p.gun();
        if cmd.has(btn::FIRE) && p.cooldown <= 0.0 && !p.overheated && p.switch_t <= 0.0 {
            p.cooldown = def.interval;
            p.heat += def.interval * 0.32 + 0.012;
            if p.heat >= 1.0 {
                p.heat = 1.0;
                p.overheated = true;
            }
            let spread = lerp(def.spread_hip, def.spread_ads, p.ads) + p.heat * 0.01;
            let f = p.forward();
            let r = p.right();
            let up = r.cross(f).norm();
            let a = rng.range(0.0, 2.0 * PI);
            let rr = spread * rng.f().sqrt();
            let dir = (f + r * (rr * a.cos()) + up * (rr * a.sin())).norm();
            out.shot = Some((p.eye(), dir));
            out.kick = def.kick;
        }
        if cmd.has(btn::JUMP) {
            p.mounted = NO_TURRET;
            p.pos = t.stand - V3::from_yaw(t.yaw) * 0.15;
            out.mount = Some(false);
        }
        return out;
    }

    let fwd_in = if p.busy { 0.0 } else { cmd.fwd_f() };
    let strafe_in = if p.busy { 0.0 } else { cmd.strafe_f() };

    // Agacharse
    let can_stand = world.body_free(p.pos, PLAYER_R, STAND_H);
    if cmd.has(btn::CROUCH) {
        p.crouching = true;
    } else if p.crouching && can_stand {
        p.crouching = false;
    }
    p.crouch = approach(p.crouch, if p.crouching { 1.0 } else { 0.0 }, dt * 7.0);

    // Correr
    p.sprinting = cmd.has(btn::SPRINT)
        && fwd_in > 0.5
        && !p.crouching
        && p.stamina > 0.05
        && p.reload_t <= 0.0
        && !cmd.has(btn::ADS)
        && !cmd.has(btn::FIRE);
    if p.sprinting {
        p.stamina = (p.stamina - dt).max(0.0);
    } else {
        p.stamina = (p.stamina + dt * 0.6).min(p.max_stamina());
    }
    let ads_target = if cmd.has(btn::ADS) && p.reload_t <= 0.0 && p.switch_t <= 0.0 && !p.sprinting && !p.busy {
        1.0
    } else {
        0.0
    };
    p.ads = approach(p.ads, ads_target, dt * 6.0);

    // Movimiento
    let fwd = V3::from_yaw(p.yaw);
    let right = p.right();
    let mut wish = fwd * fwd_in + right * strafe_in;
    if wish.len() > 1.0 {
        wish = wish.norm();
    }
    let speed = if p.sprinting {
        6.8
    } else if p.crouching {
        2.4
    } else {
        4.4
    } * (1.0 - 0.45 * p.ads)
        * if p.gun().kind == GunKind::Sniper && p.ads > 0.5 { 0.7 } else { 1.0 };
    let target = wish * speed;
    let k = ((if p.grounded { 14.0 } else { 2.5 }) * dt).min(1.0);
    p.vel.x += (target.x - p.vel.x) * k;
    p.vel.z += (target.z - p.vel.z) * k;
    if cmd.has(btn::JUMP) && p.grounded && !p.busy {
        if p.crouching {
            if can_stand {
                p.crouching = false;
            }
        } else {
            p.vel.y = 5.6;
            p.grounded = false;
        }
    }
    p.vel.y -= GRAVITY * dt;
    let delta = p.vel * dt;
    let (np, grounded, _) = world.move_body(p.pos, PLAYER_R, p.height(), delta);
    if delta.y > 0.0 && np.y - p.pos.y < delta.y * 0.5 {
        p.vel.y = 0.0;
    }
    let actual = (np - p.pos) / dt;
    p.vel.x = actual.x;
    p.vel.z = actual.z;
    p.grounded = grounded;
    if grounded && p.vel.y < 0.0 {
        p.vel.y = 0.0;
    }
    p.pos = np;
    let hs = p.vel.flat().len();
    p.moving = (hs / 4.4).min(1.0);
    if p.grounded {
        p.bob += hs * dt * 1.8;
    }

    // Cambio de arma
    let want_slot = cmd.slot.min(1);
    if want_slot != p.slot && p.throw_t <= 0.0 && !p.busy {
        p.slot = want_slot;
        p.switch_t = 0.5;
        p.reload_t = 0.0;
        p.cooldown = p.cooldown.max(0.25);
        out.switched = true;
    }

    // Recarga en curso
    if p.reload_t > 0.0 {
        p.reload_t -= dt;
        if p.reload_t <= 0.0 {
            p.reload_t = 0.0;
            let def = p.gun();
            let a = &mut p.ammo[p.slot as usize];
            let take = (def.mag - a.mag).min(a.reserve);
            a.mag += take;
            a.reserve -= take;
        }
    }

    let def = p.gun();
    let ready = p.reload_t <= 0.0 && p.switch_t <= 0.0 && p.throw_t <= 0.0 && p.melee_t <= 0.0 && !p.busy;
    let ammo = p.ammo[p.slot as usize];
    if ready
        && ammo.reserve > 0
        && ammo.mag < def.mag
        && (cmd.has(btn::RELOAD) || (ammo.mag == 0 && p.cooldown <= 0.0))
    {
        p.reload_total = if ammo.mag == 0 { def.reload_empty } else { def.reload } * if p.perk == perk::QUICK_HANDS { 0.7 } else { 1.0 };
        p.reload_t = p.reload_total;
        out.reload = true;
        return out;
    }

    let trigger = if def.auto { cmd.has(btn::FIRE) } else { cmd.has(btn::FIRE_PRESSED) };
    if trigger && ready && p.cooldown <= 0.0 {
        if ammo.mag > 0 {
            p.sprinting = false;
            p.ammo[p.slot as usize].mag -= 1;
            p.cooldown = def.interval;
            let spread = p.spread();
            p.bloom = (p.bloom + def.bloom).min(0.035);
            let f = p.forward();
            let r = p.right();
            let up = r.cross(f).norm();
            let a = rng.range(0.0, 2.0 * PI);
            let rr = spread * rng.f().sqrt();
            let dir = (f + r * (rr * a.cos()) + up * (rr * a.sin())).norm();
            out.shot = Some((p.eye(), dir));
            out.kick = def.kick * (1.0 - 0.4 * p.ads) * if p.crouching { 0.8 } else { 1.0 };
            if def.name == "M1 Garand" && p.ammo[p.slot as usize].mag == 0 {
                out.ping = true;
            }
        } else if cmd.has(btn::FIRE_PRESSED) {
            out.dry = true;
        }
    }

    let smoke = cmd.has(btn::SMOKE) && p.smokes > 0;
    if (cmd.has(btn::GRENADE) && p.grenades > 0 || smoke) && ready {
        if smoke {
            p.smokes -= 1;
        } else {
            p.grenades -= 1;
        }
        p.throw_t = 0.8;
        let f = p.forward();
        let origin = p.eye() + f * 0.4 + p.right() * 0.2;
        let vel = f * 15.0 + V3::UP * 2.5 + p.vel.flat() * 0.5;
        out.throw = Some((origin, vel));
        out.smoke = smoke;
        return out;
    }

    // Culatazo o cuchillada
    if cmd.has(btn::MELEE) && p.melee_t <= 0.0 && p.throw_t <= 0.0 && !p.busy && def.kind != GunKind::Melee {
        p.melee_t = 0.75;
        p.reload_t = 0.0;
        p.cooldown = p.cooldown.max(0.55);
        out.melee = Some((p.eye(), V3::from_yaw_pitch(p.yaw, p.pitch)));
    }
    out
}

/// (cuerpo, cabeza) de un jugador en una posición dada.
pub fn hitboxes(pos: V3, crouch: f32) -> (Aabb, Aabb) {
    let body_h = lerp(1.45, 0.95, crouch);
    let head_y = lerp(1.62, 1.12, crouch);
    (
        Aabb::cs(pos + V3::UP * (body_h * 0.5), v3(0.6, body_h, 0.6)),
        Aabb::cs(pos + V3::UP * head_y, v3(0.32, 0.34, 0.32)),
    )
}

pub struct Grenade {
    pub id: u16,
    pub pos: V3,
    pub vel: V3,
    pub fuse: f32,
    pub owner: u8,
    pub side: u8,
    pub bounce_cd: f32,
    /// Nación de quien la lanzó (decide el modelo) y si es de humo
    pub nation: u8,
    pub smoke: bool,
}

/// Física de una granada. Devuelve true si ha rebotado con fuerza.
pub fn grenade_step(world: &World, g: &mut Grenade, dt: f32) -> bool {
    let mut bounced = false;
    g.fuse -= dt;
    g.bounce_cd -= dt;
    g.vel.y -= GRAVITY * dt;
    for axis in 0..3 {
        let mut np = g.pos;
        np.set(axis, np.get(axis) + g.vel.get(axis) * dt);
        let bb = Aabb::cs(np, v3(0.16, 0.16, 0.16));
        let hit = (axis == 1 && np.y < 0.08) || world.blocks.iter().any(|b| b.b.overlaps(&bb));
        if hit {
            let speed = g.vel.len();
            g.vel.set(axis, -g.vel.get(axis) * 0.35);
            for o in 0..3 {
                if o != axis {
                    g.vel.set(o, g.vel.get(o) * 0.7);
                }
            }
            if speed > 2.5 && g.bounce_cd <= 0.0 {
                g.bounce_cd = 0.15;
                bounced = true;
            }
        } else {
            g.pos = np;
        }
    }
    if g.pos.y < 0.08 {
        g.pos.y = 0.08;
    }
    bounced
}

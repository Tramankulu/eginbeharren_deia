//! Ajustes del jugador guardados en un archivo de texto sencillo.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub name: String,
    pub sensitivity: f32,
    pub invert: bool,
    pub volume: f32,
    pub fov: f32,
    pub last_address: String,
    pub host_name: String,
    pub host_port: u16,
    pub host_max: u8,
    /// Modos de la rotación al crear partida (identificadores cortos).
    pub host_modes: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            name: "Soldado".into(),
            sensitivity: 2.2,
            invert: false,
            volume: 0.8,
            fov: 70.0,
            last_address: String::new(),
            host_name: "Partida de Eginbeharraren deia".into(),
            host_port: crate::net::DEFAULT_PORT,
            host_max: 12,
            host_modes: vec!["tdm".into()],
        }
    }
}

/// Carpeta de configuración según el sistema operativo.
pub fn config_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "windows") {
        env("APPDATA").map(|p| p.join("EginbeharrarenDeia"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|p| p.join("Library/Application Support/EginbeharrarenDeia"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config"))).map(|p| p.join("eginbeharraren-deia"))
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        config_dir().map(|d| d.join("ajustes.txt"))
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "nombre" => s.name = crate::net::clean_name(v, 20),
                "sensibilidad" => s.sensitivity = v.parse().unwrap_or(s.sensitivity),
                "invertir" => s.invert = v == "si",
                "volumen" => s.volume = v.parse().unwrap_or(s.volume),
                "campo_vision" => s.fov = v.parse().unwrap_or(s.fov),
                "ultima_direccion" => s.last_address = v.to_string(),
                "servidor_nombre" => s.host_name = crate::net::clean_name(v, 32),
                "servidor_puerto" => s.host_port = v.parse().unwrap_or(s.host_port),
                "servidor_max" => s.host_max = v.parse().unwrap_or(s.host_max),
                "servidor_modos" => {
                    let m: Vec<String> = v
                        .split(',')
                        .filter(|x| crate::net::Mode::parse(x).is_some())
                        .map(|x| x.trim().to_lowercase())
                        .collect();
                    if !m.is_empty() {
                        s.host_modes = m;
                    }
                }
                _ => {}
            }
        }
        s.sensitivity = s.sensitivity.clamp(0.2, 10.0);
        s.volume = s.volume.clamp(0.0, 1.0);
        s.fov = s.fov.clamp(60.0, 90.0);
        s.host_max = s.host_max.clamp(2, crate::net::MAX_PLAYERS as u8);
        s
    }

    pub fn to_text(&self) -> String {
        format!(
            "nombre={}\nsensibilidad={:.2}\ninvertir={}\nvolumen={:.2}\ncampo_vision={:.0}\nultima_direccion={}\nservidor_nombre={}\nservidor_puerto={}\nservidor_max={}\nservidor_modos={}\n",
            self.name,
            self.sensitivity,
            if self.invert { "si" } else { "no" },
            self.volume,
            self.fov,
            self.last_address,
            self.host_name,
            self.host_port,
            self.host_max,
            self.host_modes.join(","),
        )
    }

    pub fn load() -> Settings {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Settings::parse(&t))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, self.to_text());
        }
    }
}

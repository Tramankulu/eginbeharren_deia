//! Avisos de versión nueva y actualización desde el propio juego.
//!
//! Al arrancar se pregunta a GitHub cuál es la última versión publicada (con
//! `curl`, que traen Windows 10/11 y Ubuntu, o `wget`). Si hay una más nueva:
//! - En Ubuntu, con el juego instalado desde el .deb, se descarga el paquete y
//!   se instala con `pkexec apt-get install`, que pide la contraseña en una
//!   ventana y sí sustituye la versión anterior.
//! - En Windows, con el instalador, se descarga y se abre el instalador nuevo.
//! - En los demás casos (zip portable, compilado a mano) se abre la página de
//!   descargas en el navegador.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

pub const REPO: &str = "Tramankulu/eginbeharren_deia";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Checking,
    UpToDate,
    /// Hay una versión nueva (sin la «v»)
    Available(String),
    Working,
    /// Instalada: hay que volver a abrir el juego
    Installed,
    /// Se ha abierto el instalador de Windows: el juego se cierra
    Handed,
    Failed(String),
}

/// Compara versiones «1.2.3». Devuelve true si `new` es mayor que `cur`.
pub fn newer(new: &str, cur: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.trim().trim_start_matches('v').split('.').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    let (a, b) = (parse(new), parse(cur));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Saca `"tag_name": "v0.8.1"` de la respuesta de la API de GitHub.
pub fn tag_from_json(json: &str) -> Option<String> {
    let i = json.find("\"tag_name\"")?;
    let rest = &json[i + 10..];
    let rest = &rest[rest.find(':')? + 1..];
    let start = rest.find('"')? + 1;
    let end = rest[start..].find('"')? + start;
    Some(rest[start..end].trim_start_matches('v').to_string())
}

fn command(prog: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(prog);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // sin ventana de consola
    }
    c.stdin(Stdio::null());
    c
}

/// Descarga una URL a texto (o a un archivo, si se da `out`).
fn fetch(url: &str, out: Option<&PathBuf>) -> Result<String, String> {
    let ua = format!("eginbeharraren-deia/{VERSION}");
    let mut tries: Vec<Command> = Vec::new();
    let mut c = command("curl");
    c.args(["-fsSL", "--max-time", "300", "-A", &ua]);
    if let Some(o) = out {
        c.arg("-o").arg(o);
    }
    c.arg(url);
    tries.push(c);
    if !cfg!(windows) {
        let mut w = command("wget");
        w.args(["-q", "--timeout=60", "-U", &ua]);
        match out {
            Some(o) => w.arg("-O").arg(o),
            None => w.args(["-O", "-"]),
        };
        w.arg(url);
        tries.push(w);
    }
    let mut last = String::from("no hay curl ni wget");
    for mut c in tries {
        match c.output() {
            Ok(o) if o.status.success() => return Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
            Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_string(),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

pub fn releases_page() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

fn open_in_browser(url: &str) -> bool {
    if cfg!(windows) {
        command("cmd").args(["/C", "start", "", url]).spawn().is_ok()
    } else {
        command("xdg-open").arg(url).spawn().is_ok()
    }
}

/// Cómo está instalado este ejecutable.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Install {
    Deb,
    WindowsSetup,
    Other,
}

fn install_kind() -> Install {
    let exe = std::env::current_exe().ok();
    if cfg!(target_os = "linux") {
        let deb = exe.as_ref().map(|p| p.starts_with("/usr/bin")).unwrap_or(false)
            && std::path::Path::new("/var/lib/dpkg/info/eginbeharraren-deia.list").exists();
        return if deb { Install::Deb } else { Install::Other };
    }
    if cfg!(windows) {
        // El instalador deja su desinstalador junto al ejecutable
        let setup = exe.as_ref().and_then(|p| p.parent().map(|d| d.join("unins000.exe").exists())).unwrap_or(false);
        return if setup { Install::WindowsSetup } else { Install::Other };
    }
    Install::Other
}

pub struct Updater {
    pub state: Arc<Mutex<State>>,
}

impl Updater {
    /// Empieza a comprobar en segundo plano (no bloquea el arranque).
    pub fn start() -> Updater {
        let state = Arc::new(Mutex::new(State::Checking));
        let s = state.clone();
        let _ = std::thread::Builder::new().name("actualizaciones".into()).spawn(move || {
            let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
            let next = match fetch(&url, None).ok().and_then(|j| tag_from_json(&j)) {
                Some(tag) if newer(&tag, VERSION) => State::Available(tag),
                Some(_) => State::UpToDate,
                // Sin conexión: no se molesta al jugador
                None => State::UpToDate,
            };
            *s.lock().unwrap() = next;
        });
        Updater { state }
    }

    pub fn get(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    /// Descarga e instala la versión `ver` en segundo plano.
    pub fn install(&self, ver: String) {
        let s = self.state.clone();
        *s.lock().unwrap() = State::Working;
        let _ = std::thread::Builder::new().name("actualizar".into()).spawn(move || {
            let result = do_install(&ver);
            *s.lock().unwrap() = result;
        });
    }
}

fn do_install(ver: &str) -> State {
    let base = format!("https://github.com/{REPO}/releases/download/v{ver}");
    match install_kind() {
        Install::Deb => {
            let file = std::env::temp_dir().join(format!("eginbeharraren-deia_{ver}_amd64.deb"));
            if let Err(e) = fetch(&format!("{base}/eginbeharraren-deia_{ver}_amd64.deb"), Some(&file)) {
                return State::Failed(format!("No se pudo descargar: {e}"));
            }
            // pkexec pide la contraseña en una ventana; apt-get sí reemplaza la versión instalada
            let ok = command("pkexec")
                .args(["apt-get", "install", "-y", "--allow-downgrades"])
                .arg(&file)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            let _ = std::fs::remove_file(&file);
            if ok {
                State::Installed
            } else {
                State::Failed(format!("sudo apt install ./eginbeharraren-deia_{ver}_amd64.deb"))
            }
        }
        Install::WindowsSetup => {
            let file = std::env::temp_dir().join(format!("EginbeharrarenDeia-{ver}-Windows-Instalador.exe"));
            if let Err(e) = fetch(&format!("{base}/EginbeharrarenDeia-{ver}-Windows-Instalador.exe"), Some(&file)) {
                return State::Failed(format!("No se pudo descargar: {e}"));
            }
            // Con «start» Windows pide permiso de administrador si el instalador lo necesita
            match command("cmd").arg("/C").arg("start").arg("").arg(&file).spawn() {
                Ok(_) => State::Handed,
                Err(e) => State::Failed(e.to_string()),
            }
        }
        Install::Other => {
            // Versión portable o compilada a mano: se abre la página de descargas
            let _ = open_in_browser(&releases_page());
            State::Failed(releases_page())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versiones() {
        assert!(newer("0.8.1", "0.8.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.8.0", "0.8.0"));
        assert!(!newer("0.7.9", "0.8.0"));
        let j = r#"{"url":"x","tag_name": "v0.8.1","name":"y"}"#;
        assert_eq!(tag_from_json(j).as_deref(), Some("0.8.1"));
    }
}

//! Idiomas de la interfaz.
//!
//! El juego está escrito en castellano y los textos se traducen al dibujarlos:
//! `tr` busca la frase en la tabla `assets/idiomas/textos.txt` (una línea por
//! frase, columnas separadas por `|`). Las frases con datos variables llevan
//! `{}` en castellano; en las traducciones, `{}` toma los datos en orden y
//! `{0}`, `{1}`… permiten cambiarlos de sitio. Una casilla vacía usa el
//! castellano (o el catalán, en valenciano).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// Código y nombre propio de cada idioma, en el orden en que se ofrecen.
pub const LANGS: [(&str, &str); 10] = [
    ("eu", "Euskara"),
    ("es", "Español"),
    ("en", "English"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("ru", "Русский"),
    ("ca", "Català"),
    ("gl", "Galego"),
    ("val", "Valencià"),
    ("br", "Brezhoneg"),
];

/// Columnas de la tabla (la primera es el castellano original).
const COLUMNS: [&str; 10] = ["es", "eu", "en", "fr", "de", "ru", "ca", "gl", "val", "br"];

pub const DEFAULT: &str = "eu";

static CURRENT: AtomicUsize = AtomicUsize::new(1); // columna del euskera

const TABLE_SRC: &str = include_str!("../assets/idiomas/textos.txt");

struct Table {
    rows: Vec<Vec<&'static str>>,
    exact: HashMap<&'static str, usize>,
    /// Plantillas: trozos fijos alrededor de cada `{}` y fila, de más a menos texto fijo.
    templates: Vec<(Vec<&'static str>, usize)>,
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| parse(TABLE_SRC))
}

fn parse(src: &'static str) -> Table {
    let mut rows = Vec::new();
    let mut exact = HashMap::new();
    let mut templates = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&'static str> = line.split('|').map(str::trim).collect();
        if cols.len() < 2 || cols[0].is_empty() {
            continue;
        }
        let row = rows.len();
        if cols[0].contains("{}") {
            let lits: Vec<&'static str> = cols[0].split("{}").collect();
            templates.push((lits, row));
        } else {
            exact.insert(cols[0], row);
        }
        rows.push(cols);
    }
    templates.sort_by_key(|(lits, _)| std::cmp::Reverse(lits.iter().map(|l| l.len()).sum::<usize>()));
    Table { rows, exact, templates }
}

/// Elige el idioma por su código (`eu`, `en`…). Los desconocidos se ignoran.
pub fn set(code: &str) {
    if let Some(i) = COLUMNS.iter().position(|c| *c == code) {
        if CURRENT.swap(i, Ordering::Relaxed) != i {
            CACHE.with(|c| c.borrow_mut().clear());
        }
    }
}

/// Código del idioma actual.
pub fn current() -> &'static str {
    COLUMNS[CURRENT.load(Ordering::Relaxed)]
}

/// Siguiente (o anterior) idioma de la lista, para el selector de los ajustes.
pub fn cycle(code: &str, step: i32) -> &'static str {
    let i = LANGS.iter().position(|l| l.0 == code).unwrap_or(0) as i32;
    LANGS[(i + step).rem_euclid(LANGS.len() as i32) as usize].0
}

pub fn display_name(code: &str) -> &'static str {
    LANGS.iter().find(|l| l.0 == code).map(|l| l.1).unwrap_or("Euskara")
}

thread_local! {
    static CACHE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Traduce un texto de la interfaz al idioma actual.
pub fn tr(s: &str) -> String {
    let col = CURRENT.load(Ordering::Relaxed);
    if col == 0 || !s.chars().any(char::is_alphabetic) {
        return s.to_string();
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(s).cloned()) {
        return hit;
    }
    let out = translate(s, col, 0).unwrap_or_else(|| s.to_string());
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 4000 {
            c.clear();
        }
        c.insert(s.to_string(), out.clone());
    });
    out
}

/// Celda de una fila en la columna pedida, con los repuestos de las vacías.
fn cell(row: &[&'static str], col: usize) -> Option<&'static str> {
    let get = |c: usize| row.get(c).copied().filter(|x| !x.is_empty());
    get(col).or_else(|| if COLUMNS[col] == "val" { get(6) } else { None })
}

fn translate(s: &str, col: usize, depth: u32) -> Option<String> {
    // Los espacios de los extremos se conservan tal cual
    let core = s.trim();
    if core.is_empty() {
        return None;
    }
    let start = s.find(core).unwrap_or(0);
    let (pre, post) = (&s[..start], &s[start + core.len()..]);
    let t = table();
    let done = |body: String| Some(format!("{pre}{body}{post}"));
    if let Some(&row) = t.exact.get(core) {
        return done(cell(&t.rows[row], col)?.to_string());
    }
    if depth > 1 {
        return None;
    }
    for (lits, row) in &t.templates {
        if let Some(args) = match_template(lits, core) {
            let Some(target) = cell(&t.rows[*row], col) else { continue };
            let args: Vec<String> = args.iter().map(|a| translate(a, col, depth + 1).unwrap_or_else(|| a.to_string())).collect();
            return done(fill(target, &args));
        }
    }
    None
}

/// Si `s` encaja con los trozos fijos, devuelve lo que hay entre ellos.
fn match_template<'a>(lits: &[&str], s: &'a str) -> Option<Vec<&'a str>> {
    let first = lits[0];
    let last = lits[lits.len() - 1];
    if !s.starts_with(first) || s.len() < first.len() + last.len() || !s.ends_with(last) {
        return None;
    }
    let end = s.len() - last.len();
    let mut pos = first.len();
    let mut args = Vec::with_capacity(lits.len() - 1);
    for lit in &lits[1..lits.len() - 1] {
        if lit.is_empty() {
            return None;
        }
        let found = s[pos..end].find(lit)? + pos;
        args.push(&s[pos..found]);
        pos = found + lit.len();
    }
    if pos > end {
        return None;
    }
    args.push(&s[pos..end]);
    Some(args)
}

/// Pone los datos en la traducción: `{}` en orden, `{n}` por posición.
fn fill(target: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(target.len() + 16);
    let mut next = 0;
    let mut rest = target;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) if after[..j].chars().all(|c| c.is_ascii_digit()) => {
                let k = if j == 0 {
                    next += 1;
                    next - 1
                } else {
                    after[..j].parse().unwrap_or(0)
                };
                out.push_str(args.get(k).map(String::as_str).unwrap_or(""));
                rest = &after[j + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Para las pruebas: todas las filas con su número de columnas.
pub fn rows() -> Vec<Vec<&'static str>> {
    table().rows.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plantillas() {
        let lits: Vec<&str> = "Ronda {} · atacan los Aliados · gana quien llegue a {}".split("{}").collect();
        let a = match_template(&lits, "Ronda 2 · atacan los Aliados · gana quien llegue a 4").unwrap();
        assert_eq!(a, vec!["2", "4"]);
        assert_eq!(fill("{1} eta {0}", &["a".into(), "b".into()]), "b eta a");
        assert_eq!(fill("{} eta {}", &["a".into(), "b".into()]), "a eta b");
    }
}

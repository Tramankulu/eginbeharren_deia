//! Cargador de modelos glTF 2.0 (`.glb` y `.gltf`), sin dependencias.
//!
//! Lee mallas, materiales (color base y textura), la jerarquía de nodos,
//! esqueletos y animaciones. La pose y el «skinning» se calculan en la CPU:
//! así el mismo código sirve al juego y al servidor dedicado (que solo
//! necesita la geometría de los escenarios para las colisiones).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ----------------------------------------------------------------------
// JSON mínimo
// ----------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

static NULL: Json = Json::Null;

impl Json {
    pub fn parse(s: &str) -> Result<Json, String> {
        let b = s.as_bytes();
        let mut i = 0;
        let v = parse_value(b, &mut i)?;
        skip_ws(b, &mut i);
        if i != b.len() {
            return Err(format!("JSON: texto sobrante en {i}"));
        }
        Ok(v)
    }
    pub fn get(&self, k: &str) -> &Json {
        match self {
            Json::Obj(v) => v.iter().find(|(a, _)| a == k).map(|(_, b)| b).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
    pub fn at(&self, i: usize) -> &Json {
        match self {
            Json::Arr(v) => v.get(i).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => &[],
        }
    }
    pub fn f(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn u(&self) -> Option<usize> {
        self.f().filter(|n| *n >= 0.0).map(|n| n as usize)
    }
    pub fn s(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn b(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
    fn floats(&self) -> Vec<f32> {
        self.arr().iter().filter_map(|x| x.f()).map(|x| x as f32).collect()
    }
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && (b[*i] as char).is_ascii_whitespace() {
        *i += 1;
    }
}

fn parse_value(b: &[u8], i: &mut usize) -> Result<Json, String> {
    skip_ws(b, i);
    let Some(&c) = b.get(*i) else { return Err("JSON: fin inesperado".into()) };
    match c {
        b'{' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                skip_ws(b, i);
                if b.get(*i) == Some(&b'}') {
                    *i += 1;
                    break;
                }
                let k = match parse_value(b, i)? {
                    Json::Str(s) => s,
                    _ => return Err("JSON: clave no válida".into()),
                };
                skip_ws(b, i);
                if b.get(*i) != Some(&b':') {
                    return Err("JSON: falta ':'".into());
                }
                *i += 1;
                v.push((k, parse_value(b, i)?));
                skip_ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b'}') => {
                        *i += 1;
                        break;
                    }
                    _ => return Err("JSON: objeto mal cerrado".into()),
                }
            }
            Ok(Json::Obj(v))
        }
        b'[' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                skip_ws(b, i);
                if b.get(*i) == Some(&b']') {
                    *i += 1;
                    break;
                }
                v.push(parse_value(b, i)?);
                skip_ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b']') => {
                        *i += 1;
                        break;
                    }
                    _ => return Err("JSON: lista mal cerrada".into()),
                }
            }
            Ok(Json::Arr(v))
        }
        b'"' => {
            *i += 1;
            let mut out: Vec<u8> = Vec::new();
            loop {
                let Some(&c) = b.get(*i) else { return Err("JSON: texto sin cerrar".into()) };
                *i += 1;
                match c {
                    b'"' => break,
                    b'\\' => {
                        let Some(&e) = b.get(*i) else { return Err("JSON: escape".into()) };
                        *i += 1;
                        match e {
                            b'n' => out.push(b'\n'),
                            b't' => out.push(b'\t'),
                            b'r' => out.push(b'\r'),
                            b'b' => out.push(8),
                            b'f' => out.push(12),
                            b'u' => {
                                let hex = std::str::from_utf8(b.get(*i..*i + 4).unwrap_or(b"0000")).unwrap_or("0000");
                                *i += 4;
                                let mut cp = u32::from_str_radix(hex, 16).unwrap_or(63);
                                if (0xD800..0xDC00).contains(&cp) && b.get(*i) == Some(&b'\\') {
                                    let hex2 = std::str::from_utf8(b.get(*i + 2..*i + 6).unwrap_or(b"0000")).unwrap_or("0000");
                                    let lo = u32::from_str_radix(hex2, 16).unwrap_or(0);
                                    *i += 6;
                                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                                }
                                let ch = char::from_u32(cp).unwrap_or('?');
                                let mut buf = [0u8; 4];
                                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                            }
                            other => out.push(other),
                        }
                    }
                    c => out.push(c),
                }
            }
            Ok(Json::Str(String::from_utf8_lossy(&out).into_owned()))
        }
        b't' if b[*i..].starts_with(b"true") => {
            *i += 4;
            Ok(Json::Bool(true))
        }
        b'f' if b[*i..].starts_with(b"false") => {
            *i += 5;
            Ok(Json::Bool(false))
        }
        b'n' if b[*i..].starts_with(b"null") => {
            *i += 4;
            Ok(Json::Null)
        }
        _ => {
            let s = *i;
            while *i < b.len() && matches!(b[*i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                *i += 1;
            }
            std::str::from_utf8(&b[s..*i])
                .ok()
                .and_then(|t| t.parse::<f64>().ok())
                .map(Json::Num)
                .ok_or_else(|| format!("JSON: valor no válido en {s}"))
        }
    }
}

// ----------------------------------------------------------------------
// Matemáticas: matrices 4x4 (por columnas) y cuaterniones
// ----------------------------------------------------------------------

pub type M4 = [f32; 16];

pub const IDENTITY: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for row in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + row] * b[c * 4 + k];
            }
            r[c * 4 + row] = s;
        }
    }
    r
}

pub fn trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> M4 {
    let [x, y, z, w] = q;
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    [
        (1.0 - 2.0 * (yy + zz)) * s[0],
        (2.0 * (xy + wz)) * s[0],
        (2.0 * (xz - wy)) * s[0],
        0.0,
        (2.0 * (xy - wz)) * s[1],
        (1.0 - 2.0 * (xx + zz)) * s[1],
        (2.0 * (yz + wx)) * s[1],
        0.0,
        (2.0 * (xz + wy)) * s[2],
        (2.0 * (yz - wx)) * s[2],
        (1.0 - 2.0 * (xx + yy)) * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

pub fn xform(m: &M4, p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

pub fn xform_dir(m: &M4, p: [f32; 3]) -> [f32; 3] {
    [m[0] * p[0] + m[4] * p[1] + m[8] * p[2], m[1] * p[0] + m[5] * p[1] + m[9] * p[2], m[2] * p[0] + m[6] * p[1] + m[10] * p[2]]
}

pub fn translation(t: [f32; 3]) -> M4 {
    let mut m = IDENTITY;
    m[12] = t[0];
    m[13] = t[1];
    m[14] = t[2];
    m
}

/// Giro de `a` radianes alrededor del eje (unitario) `ax`.
pub fn rotation(ax: [f32; 3], a: f32) -> M4 {
    let h = a * 0.5;
    let s = h.sin();
    trs([0.0; 3], [ax[0] * s, ax[1] * s, ax[2] * s, h.cos()], [1.0; 3])
}

fn nlerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let s = if d < 0.0 { -1.0 } else { 1.0 };
    let mut r = [0.0; 4];
    for k in 0..4 {
        r[k] = a[k] * (1.0 - t) + b[k] * s * t;
    }
    let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3]).sqrt().max(1e-9);
    r.map(|x| x / l)
}

fn norm3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-9 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 1.0, 0.0]
    }
}

// ----------------------------------------------------------------------
// Estructuras del modelo
// ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alpha {
    Opaque,
    Mask(f32),
    Blend,
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub color: [f32; 4],
    /// Índice en `Model::images`
    pub image: Option<usize>,
    pub alpha: Alpha,
    pub double_sided: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Prim {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub col: Vec<[f32; 4]>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
    pub material: usize,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub t: [f32; 3],
    pub r: [f32; 4],
    pub s: [f32; 3],
    /// Matriz fija (si el nodo la trae en vez de TRS)
    pub matrix: Option<M4>,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Skin {
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<M4>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Path3 {
    T,
    R,
    S,
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub node: usize,
    pub path: Path3,
    pub times: Vec<f32>,
    pub values: Vec<[f32; 4]>,
    pub step: bool,
}

#[derive(Clone, Debug)]
pub struct Anim {
    pub name: String,
    pub channels: Vec<Channel>,
    pub duration: f32,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub nodes: Vec<Node>,
    /// Mallas: cada una es una lista de primitivas
    pub meshes: Vec<Vec<Prim>>,
    pub materials: Vec<Material>,
    /// Imágenes sin decodificar (PNG o JPEG)
    pub images: Vec<Vec<u8>>,
    pub skins: Vec<Skin>,
    pub anims: Vec<Anim>,
    /// Nodos raíz de la escena
    pub roots: Vec<usize>,
}

/// Geometría ya colocada (en el espacio del modelo) de una primitiva.
#[derive(Clone, Debug, Default)]
pub struct Posed {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
}

// ----------------------------------------------------------------------
// Lectura
// ----------------------------------------------------------------------

fn data_uri(uri: &str) -> Option<Vec<u8>> {
    let rest = uri.strip_prefix("data:")?;
    let comma = rest.find(',')?;
    let (head, data) = (&rest[..comma], &rest[comma + 1..]);
    if !head.ends_with(";base64") {
        return Some(data.as_bytes().to_vec());
    }
    base64(data)
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in s.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Reader<'a> {
    j: &'a Json,
    buffers: Vec<Vec<u8>>,
}

impl<'a> Reader<'a> {
    fn view(&self, i: usize) -> Result<(&[u8], usize), String> {
        let v = self.j.get("bufferViews").at(i);
        let buf = self.buffers.get(v.get("buffer").u().unwrap_or(0)).ok_or("bufferView sin buffer")?;
        let off = v.get("byteOffset").u().unwrap_or(0);
        let len = v.get("byteLength").u().unwrap_or(0);
        let data = buf.get(off..off + len).ok_or("bufferView fuera de rango")?;
        Ok((data, v.get("byteStride").u().unwrap_or(0)))
    }

    /// Lee un «accessor» como lista de vectores de hasta 4 componentes en f32
    /// (normalizando enteros si el accessor lo pide).
    fn accessor(&self, i: usize) -> Result<(Vec<[f32; 4]>, usize), String> {
        let a = self.j.get("accessors").at(i);
        let count = a.get("count").u().unwrap_or(0);
        let ncomp = match a.get("type").s().unwrap_or("SCALAR") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            t => return Err(format!("tipo de accessor no admitido: {t}")),
        };
        let ct = a.get("componentType").u().unwrap_or(5126);
        let norm = a.get("normalized").b().unwrap_or(false);
        let csize = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(format!("componente no admitido: {ct}")),
        };
        let read = |d: &[u8], o: usize| -> f32 {
            match ct {
                5120 => {
                    let v = d[o] as i8 as f32;
                    if norm { (v / 127.0).max(-1.0) } else { v }
                }
                5121 => {
                    let v = d[o] as f32;
                    if norm { v / 255.0 } else { v }
                }
                5122 => {
                    let v = i16::from_le_bytes([d[o], d[o + 1]]) as f32;
                    if norm { (v / 32767.0).max(-1.0) } else { v }
                }
                5123 => {
                    let v = u16::from_le_bytes([d[o], d[o + 1]]) as f32;
                    if norm { v / 65535.0 } else { v }
                }
                5125 => u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]) as f32,
                _ => f32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]),
            }
        };
        let mut out = vec![[0.0f32; 4]; if ncomp == 16 { count * 4 } else { count }];
        if let Some(bv) = a.get("bufferView").u() {
            let (data, stride) = self.view(bv)?;
            let base = a.get("byteOffset").u().unwrap_or(0);
            let elem = csize * ncomp;
            let stride = if stride == 0 { elem } else { stride };
            if count > 0 && base + stride * (count - 1) + elem > data.len() {
                return Err("accessor fuera de rango".into());
            }
            for k in 0..count {
                let o = base + stride * k;
                if ncomp == 16 {
                    for col in 0..4 {
                        for row in 0..4 {
                            out[k * 4 + col][row] = read(data, o + (col * 4 + row) * csize);
                        }
                    }
                } else {
                    for c in 0..ncomp {
                        out[k][c] = read(data, o + c * csize);
                    }
                }
            }
        }
        // Datos dispersos (sparse): sustituyen elementos concretos
        let sp = a.get("sparse");
        if !sp.is_null() && ncomp != 16 {
            let n = sp.get("count").u().unwrap_or(0);
            let ind = sp.get("indices");
            let val = sp.get("values");
            let (idata, _) = self.view(ind.get("bufferView").u().unwrap_or(0))?;
            let (vdata, _) = self.view(val.get("bufferView").u().unwrap_or(0))?;
            let ioff = ind.get("byteOffset").u().unwrap_or(0);
            let voff = val.get("byteOffset").u().unwrap_or(0);
            let ict = ind.get("componentType").u().unwrap_or(5125);
            for k in 0..n {
                let ix = match ict {
                    5121 => idata.get(ioff + k).copied().unwrap_or(0) as usize,
                    5123 => {
                        let o = ioff + k * 2;
                        u16::from_le_bytes([idata[o], idata[o + 1]]) as usize
                    }
                    _ => {
                        let o = ioff + k * 4;
                        u32::from_le_bytes([idata[o], idata[o + 1], idata[o + 2], idata[o + 3]]) as usize
                    }
                };
                if ix < out.len() {
                    for c in 0..ncomp {
                        out[ix][c] = read(vdata, voff + (k * ncomp + c) * csize);
                    }
                }
            }
        }
        Ok((out, ncomp))
    }
}

impl Model {
    /// Lee un `.glb` o un `.gltf` (con sus `.bin` e imágenes al lado).
    pub fn load(path: &Path) -> Result<Model, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        Model::from_bytes(&bytes, Some(&dir)).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_bytes(bytes: &[u8], dir: Option<&Path>) -> Result<Model, String> {
        let (json_text, bin) = if bytes.len() >= 12 && &bytes[0..4] == b"glTF" {
            let mut i = 12;
            let mut json = None;
            let mut bin = None;
            while i + 8 <= bytes.len() {
                let len = u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
                let kind = &bytes[i + 4..i + 8];
                let data = bytes.get(i + 8..i + 8 + len).ok_or("bloque GLB truncado")?;
                if kind == b"JSON" {
                    json = Some(String::from_utf8_lossy(data).into_owned());
                } else if kind == b"BIN\0" {
                    bin = Some(data.to_vec());
                }
                i += 8 + len;
            }
            (json.ok_or("GLB sin JSON")?, bin)
        } else {
            (String::from_utf8_lossy(bytes).into_owned(), None)
        };
        let j = Json::parse(&json_text)?;
        let load_uri = |uri: &str| -> Result<Vec<u8>, String> {
            if uri.starts_with("data:") {
                return data_uri(uri).ok_or_else(|| "URI de datos no válida".to_string());
            }
            let d = dir.ok_or("el modelo apunta a archivos externos")?;
            let p = d.join(percent_decode(uri));
            std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
        };
        let mut buffers = Vec::new();
        for (k, b) in j.get("buffers").arr().iter().enumerate() {
            match b.get("uri").s() {
                Some(uri) => buffers.push(load_uri(uri)?),
                None if k == 0 => buffers.push(bin.clone().unwrap_or_default()),
                None => buffers.push(Vec::new()),
            }
        }
        let r = Reader { j: &j, buffers };

        // Imágenes
        let mut images = Vec::new();
        for im in j.get("images").arr() {
            let data = if let Some(bv) = im.get("bufferView").u() {
                r.view(bv)?.0.to_vec()
            } else if let Some(uri) = im.get("uri").s() {
                load_uri(uri).unwrap_or_default()
            } else {
                Vec::new()
            };
            images.push(data);
        }
        let tex_image = |t: usize| -> Option<usize> {
            let tx = j.get("textures").at(t);
            tx.get("source").u().or_else(|| {
                // Extensiones habituales con la imagen en otro formato
                let e = tx.get("extensions");
                e.get("KHR_texture_basisu").get("source").u().or_else(|| e.get("EXT_texture_webp").get("source").u())
            })
        };

        // Materiales
        let mut materials: Vec<Material> = j
            .get("materials")
            .arr()
            .iter()
            .map(|m| {
                let pbr = m.get("pbrMetallicRoughness");
                let f = pbr.get("baseColorFactor").floats();
                let color = if f.len() == 4 { [f[0], f[1], f[2], f[3]] } else { [1.0; 4] };
                let image = pbr.get("baseColorTexture").get("index").u().and_then(tex_image);
                // Materiales «sin luz» también pueden traer su textura en emisión
                let image = image.or_else(|| m.get("emissiveTexture").get("index").u().and_then(tex_image));
                let alpha = match m.get("alphaMode").s().unwrap_or("OPAQUE") {
                    "MASK" => Alpha::Mask(m.get("alphaCutoff").f().unwrap_or(0.5) as f32),
                    "BLEND" => Alpha::Blend,
                    _ => Alpha::Opaque,
                };
                Material {
                    name: m.get("name").s().unwrap_or("").to_string(),
                    color,
                    image,
                    alpha,
                    double_sided: m.get("doubleSided").b().unwrap_or(false),
                }
            })
            .collect();
        let default_mat = materials.len();
        materials.push(Material { name: "defecto".into(), color: [0.8, 0.8, 0.8, 1.0], image: None, alpha: Alpha::Opaque, double_sided: false });

        // Mallas
        let mut meshes = Vec::new();
        for m in j.get("meshes").arr() {
            let mut prims = Vec::new();
            for p in m.get("primitives").arr() {
                let mode = p.get("mode").u().unwrap_or(4);
                if mode != 4 && mode != 5 && mode != 6 {
                    continue; // líneas y puntos: no se dibujan
                }
                let at = p.get("attributes");
                let Some(pi) = at.get("POSITION").u() else { continue };
                let pos: Vec<[f32; 3]> = r.accessor(pi)?.0.iter().map(|v| [v[0], v[1], v[2]]).collect();
                let n = pos.len();
                let mut prim = Prim { pos, material: p.get("material").u().unwrap_or(default_mat).min(default_mat), ..Default::default() };
                if let Some(i) = at.get("NORMAL").u() {
                    prim.nrm = r.accessor(i)?.0.iter().map(|v| norm3([v[0], v[1], v[2]])).collect();
                }
                if let Some(i) = at.get("TEXCOORD_0").u() {
                    prim.uv = r.accessor(i)?.0.iter().map(|v| [v[0], v[1]]).collect();
                }
                if let Some(i) = at.get("COLOR_0").u() {
                    let (c, nc) = r.accessor(i)?;
                    prim.col = c.iter().map(|v| [v[0], v[1], v[2], if nc == 4 { v[3] } else { 1.0 }]).collect();
                }
                if let (Some(ji), Some(wi)) = (at.get("JOINTS_0").u(), at.get("WEIGHTS_0").u()) {
                    prim.joints = r.accessor(ji)?.0.iter().map(|v| [v[0] as u16, v[1] as u16, v[2] as u16, v[3] as u16]).collect();
                    prim.weights = r
                        .accessor(wi)?
                        .0
                        .iter()
                        .map(|v| {
                            let s = v[0] + v[1] + v[2] + v[3];
                            if s > 1e-6 { [v[0] / s, v[1] / s, v[2] / s, v[3] / s] } else { [1.0, 0.0, 0.0, 0.0] }
                        })
                        .collect();
                }
                let raw: Vec<u32> = match p.get("indices").u() {
                    Some(i) => r.accessor(i)?.0.iter().map(|v| v[0] as u32).collect(),
                    None => (0..n as u32).collect(),
                };
                prim.idx = match mode {
                    5 => (2..raw.len())
                        .flat_map(|k| if k % 2 == 0 { [raw[k - 2], raw[k - 1], raw[k]] } else { [raw[k - 1], raw[k - 2], raw[k]] })
                        .collect(),
                    6 => (2..raw.len()).flat_map(|k| [raw[0], raw[k - 1], raw[k]]).collect(),
                    _ => raw,
                };
                prim.idx.retain(|&i| (i as usize) < n);
                prim.idx.truncate(prim.idx.len() / 3 * 3);
                if prim.nrm.len() != n {
                    prim.nrm = smooth_normals(&prim.pos, &prim.idx);
                }
                if prim.uv.len() != n {
                    prim.uv = vec![[0.0, 0.0]; n];
                }
                if prim.joints.len() != n || prim.weights.len() != n {
                    prim.joints.clear();
                    prim.weights.clear();
                }
                prims.push(prim);
            }
            meshes.push(prims);
        }

        // Nodos
        let mut nodes: Vec<Node> = j
            .get("nodes")
            .arr()
            .iter()
            .map(|n| {
                let t = n.get("translation").floats();
                let rr = n.get("rotation").floats();
                let s = n.get("scale").floats();
                let m = n.get("matrix").floats();
                Node {
                    name: n.get("name").s().unwrap_or("").to_string(),
                    parent: None,
                    children: n.get("children").arr().iter().filter_map(|c| c.u()).collect(),
                    t: if t.len() == 3 { [t[0], t[1], t[2]] } else { [0.0; 3] },
                    r: if rr.len() == 4 { [rr[0], rr[1], rr[2], rr[3]] } else { [0.0, 0.0, 0.0, 1.0] },
                    s: if s.len() == 3 { [s[0], s[1], s[2]] } else { [1.0; 3] },
                    matrix: if m.len() == 16 {
                        let mut a = [0.0; 16];
                        a.copy_from_slice(&m);
                        Some(a)
                    } else {
                        None
                    },
                    mesh: n.get("mesh").u(),
                    skin: n.get("skin").u(),
                }
            })
            .collect();
        let count = nodes.len();
        for k in 0..count {
            let ch = nodes[k].children.clone();
            nodes[k].children.retain(|&c| c < count);
            for c in ch.into_iter().filter(|&c| c < count) {
                nodes[c].parent = Some(k);
            }
        }
        let scene = j.get("scene").u().unwrap_or(0);
        let mut roots: Vec<usize> = j.get("scenes").at(scene).get("nodes").arr().iter().filter_map(|n| n.u()).filter(|&n| n < count).collect();
        if roots.is_empty() {
            roots = (0..count).filter(|&k| nodes[k].parent.is_none()).collect();
        }

        // Esqueletos
        let mut skins = Vec::new();
        for s in j.get("skins").arr() {
            let joints: Vec<usize> = s.get("joints").arr().iter().filter_map(|x| x.u()).map(|x| x.min(count.saturating_sub(1))).collect();
            let mut inverse_bind = vec![IDENTITY; joints.len()];
            if let Some(a) = s.get("inverseBindMatrices").u() {
                let (cols, _) = r.accessor(a)?;
                for (k, m) in inverse_bind.iter_mut().enumerate() {
                    if k * 4 + 3 < cols.len() {
                        for c in 0..4 {
                            for row in 0..4 {
                                m[c * 4 + row] = cols[k * 4 + c][row];
                            }
                        }
                    }
                }
            }
            skins.push(Skin { joints, inverse_bind });
        }

        // Animaciones
        let mut anims = Vec::new();
        for (k, a) in j.get("animations").arr().iter().enumerate() {
            let samplers = a.get("samplers");
            let mut channels = Vec::new();
            let mut duration: f32 = 0.0;
            for ch in a.get("channels").arr() {
                let tg = ch.get("target");
                let Some(node) = tg.get("node").u().filter(|&n| n < count) else { continue };
                let path = match tg.get("path").s().unwrap_or("") {
                    "translation" => Path3::T,
                    "rotation" => Path3::R,
                    "scale" => Path3::S,
                    _ => continue, // pesos de «morph targets»: no se usan
                };
                let sm = samplers.at(ch.get("sampler").u().unwrap_or(0));
                let times: Vec<f32> = r.accessor(sm.get("input").u().unwrap_or(0))?.0.iter().map(|v| v[0]).collect();
                let mut values = r.accessor(sm.get("output").u().unwrap_or(0))?.0;
                let interp = sm.get("interpolation").s().unwrap_or("LINEAR");
                if interp == "CUBICSPLINE" {
                    // Nos quedamos con los valores (sin las tangentes)
                    values = values.chunks(3).filter(|c| c.len() == 3).map(|c| c[1]).collect();
                }
                if times.is_empty() || values.len() < times.len() {
                    continue;
                }
                duration = duration.max(*times.last().unwrap());
                channels.push(Channel { node, path, times, values, step: interp == "STEP" });
            }
            anims.push(Anim { name: a.get("name").s().map(str::to_string).unwrap_or_else(|| format!("anim{k}")), channels, duration: duration.max(1e-3) });
        }

        Ok(Model { nodes, meshes, materials, images, skins, anims, roots })
    }

    pub fn find_node(&self, names: &[&str]) -> Option<usize> {
        for want in names {
            let w = want.to_lowercase();
            if let Some(k) = self.nodes.iter().position(|n| n.name.to_lowercase() == w) {
                return Some(k);
            }
        }
        for want in names {
            let w = want.to_lowercase();
            if let Some(k) = self.nodes.iter().position(|n| n.name.to_lowercase().contains(&w)) {
                return Some(k);
            }
        }
        None
    }

    /// Busca una animación por nombre: primero exacto y después por
    /// palabras clave; `avoid` descarta nombres que contengan esas palabras.
    pub fn find_anim(&self, keys: &[&str], avoid: &[&str]) -> Option<usize> {
        let names: Vec<String> = self.anims.iter().map(|a| a.name.to_lowercase()).collect();
        for k in keys {
            let k = k.to_lowercase();
            if let Some(i) = names.iter().position(|n| *n == k) {
                return Some(i);
            }
        }
        for k in keys {
            let k = k.to_lowercase();
            if let Some(i) = names.iter().position(|n| n.contains(&k) && !avoid.iter().any(|a| n.contains(&a.to_lowercase()))) {
                return Some(i);
            }
        }
        None
    }

    /// Transformaciones locales (TRS) de todos los nodos mezclando las
    /// animaciones dadas como (índice, tiempo, peso).
    pub fn local_pose(&self, mix: &[(usize, f32, f32)]) -> Vec<([f32; 3], [f32; 4], [f32; 3])> {
        let mut out: Vec<_> = self.nodes.iter().map(|n| (n.t, n.r, n.s)).collect();
        let total: f32 = mix.iter().filter(|m| m.0 < self.anims.len()).map(|m| m.2.max(0.0)).sum();
        if total <= 1e-6 {
            return out;
        }
        // Acumuladores por nodo: (peso acumulado, valor) para T, R y S
        let mut acc: HashMap<(usize, u8), (f32, [f32; 4])> = HashMap::new();
        for &(ai, time, w) in mix {
            let Some(a) = self.anims.get(ai) else { continue };
            let w = w.max(0.0) / total;
            if w <= 0.0 {
                continue;
            }
            let t = time.rem_euclid(a.duration.max(1e-3)).min(a.duration);
            for ch in &a.channels {
                let v = sample(ch, t);
                let key = (ch.node, ch.path as u8);
                let e = acc.entry(key).or_insert((0.0, [0.0; 4]));
                if ch.path == Path3::R {
                    if e.0 == 0.0 {
                        e.1 = v;
                    } else {
                        e.1 = nlerp(e.1, v, w / (e.0 + w));
                    }
                } else {
                    for c in 0..4 {
                        e.1[c] += v[c] * w;
                    }
                }
                e.0 += w;
            }
        }
        for ((node, path), (wsum, v)) in acc {
            let rest = out[node];
            // Si no todas las animaciones mueven este canal, se completa con la pose de reposo
            let missing = (1.0 - wsum).max(0.0);
            match path {
                0 => out[node].0 = [v[0] + rest.0[0] * missing, v[1] + rest.0[1] * missing, v[2] + rest.0[2] * missing],
                1 => out[node].1 = if missing > 0.0 { nlerp(v, rest.1, missing) } else { v },
                _ => out[node].2 = [v[0] + rest.2[0] * missing, v[1] + rest.2[1] * missing, v[2] + rest.2[2] * missing],
            }
        }
        out
    }

    /// Matrices globales de cada nodo a partir de una pose local.
    pub fn globals(&self, local: &[([f32; 3], [f32; 4], [f32; 3])]) -> Vec<M4> {
        let mut g = vec![IDENTITY; self.nodes.len()];
        let mut stack: Vec<(usize, M4)> = self.roots.iter().map(|&r| (r, IDENTITY)).collect();
        let mut seen = vec![false; self.nodes.len()];
        while let Some((n, parent)) = stack.pop() {
            if seen[n] {
                continue;
            }
            seen[n] = true;
            let node = &self.nodes[n];
            let l = match node.matrix {
                Some(m) => m,
                None => trs(local[n].0, local[n].1, local[n].2),
            };
            g[n] = mul(&parent, &l);
            for &c in &node.children {
                stack.push((c, g[n]));
            }
        }
        g
    }

    /// Pose de reposo (o la primera imagen de una animación).
    pub fn rest_globals(&self) -> Vec<M4> {
        self.globals(&self.local_pose(&[]))
    }

    /// Todos los descendientes de un nodo (incluido él mismo).
    pub fn subtree(&self, n: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut st = vec![n];
        while let Some(k) = st.pop() {
            if out.contains(&k) {
                continue;
            }
            out.push(k);
            st.extend(self.nodes[k].children.iter().copied());
        }
        out
    }

    /// Lista de (nodo, malla) que se dibujan, en orden estable.
    pub fn drawables(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut st: Vec<usize> = self.roots.iter().rev().copied().collect();
        let mut seen = vec![false; self.nodes.len()];
        while let Some(n) = st.pop() {
            if seen[n] {
                continue;
            }
            seen[n] = true;
            if let Some(m) = self.nodes[n].mesh.filter(|&m| m < self.meshes.len()) {
                out.push((n, m));
            }
            st.extend(self.nodes[n].children.iter().rev().copied());
        }
        out
    }

    /// Coloca (y deforma con el esqueleto) todas las primitivas con las
    /// matrices globales dadas. El resultado sigue el orden de `drawables()`
    /// y, dentro de cada malla, el de sus primitivas.
    pub fn pose(&self, globals: &[M4], out: &mut Vec<Posed>) {
        let mut k = 0;
        for (node, mesh) in self.drawables() {
            let skin = self.nodes[node].skin.and_then(|s| self.skins.get(s));
            let joint_m: Vec<M4> = match skin {
                Some(s) => s.joints.iter().zip(&s.inverse_bind).map(|(&j, ib)| mul(&globals[j], ib)).collect(),
                None => Vec::new(),
            };
            for prim in &self.meshes[mesh] {
                if out.len() <= k {
                    out.push(Posed::default());
                }
                let o = &mut out[k];
                k += 1;
                o.pos.clear();
                o.nrm.clear();
                if !joint_m.is_empty() && !prim.joints.is_empty() {
                    for v in 0..prim.pos.len() {
                        let (jw, ww) = (prim.joints[v], prim.weights[v]);
                        let mut m = [0.0f32; 16];
                        for c in 0..4 {
                            if ww[c] <= 0.0 {
                                continue;
                            }
                            let jm = &joint_m[(jw[c] as usize).min(joint_m.len() - 1)];
                            for e in 0..16 {
                                m[e] += jm[e] * ww[c];
                            }
                        }
                        o.pos.push(xform(&m, prim.pos[v]));
                        o.nrm.push(norm3(xform_dir(&m, prim.nrm[v])));
                    }
                } else {
                    let m = &globals[node];
                    for v in 0..prim.pos.len() {
                        o.pos.push(xform(m, prim.pos[v]));
                        o.nrm.push(norm3(xform_dir(m, prim.nrm[v])));
                    }
                }
            }
        }
        out.truncate(k);
    }

    /// Primitivas en el mismo orden que `pose()`.
    pub fn prims_in_order(&self) -> Vec<&Prim> {
        self.drawables().into_iter().flat_map(|(_, m)| self.meshes[m].iter()).collect()
    }

    /// Caja (mín, máx) de la geometría colocada.
    pub fn bounds(posed: &[Posed]) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in posed {
            for v in &p.pos {
                for c in 0..3 {
                    lo[c] = lo[c].min(v[c]);
                    hi[c] = hi[c].max(v[c]);
                }
            }
        }
        if lo[0] > hi[0] {
            ([0.0; 3], [0.0; 3])
        } else {
            (lo, hi)
        }
    }
}

fn sample(ch: &Channel, t: f32) -> [f32; 4] {
    let ts = &ch.times;
    if t <= ts[0] {
        return ch.values[0];
    }
    let last = ts.len() - 1;
    if t >= ts[last] {
        return ch.values[last];
    }
    let k = match ts.binary_search_by(|x| x.partial_cmp(&t).unwrap_or(std::cmp::Ordering::Less)) {
        Ok(k) => return ch.values[k],
        Err(k) => k - 1,
    };
    let (a, b) = (ch.values[k], ch.values[k + 1]);
    if ch.step {
        return a;
    }
    let f = ((t - ts[k]) / (ts[k + 1] - ts[k]).max(1e-6)).clamp(0.0, 1.0);
    if ch.path == Path3::R {
        nlerp(a, b, f)
    } else {
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, 0.0]
    }
}

pub fn smooth_normals(pos: &[[f32; 3]], idx: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; pos.len()];
    for t in idx.chunks(3) {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for k in 0..3 {
                n[i as usize][k] += f[k];
            }
        }
    }
    n.into_iter().map(norm3).collect()
}

/// Normales suavizadas por posición (fusionando vértices duplicados en las
/// aristas duras). Sirven para engordar el contorno de tinta sin que se abra.
pub fn outline_normals(pos: &[[f32; 3]], nrm: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let key = |p: &[f32; 3]| ((p[0] * 2000.0).round() as i64, (p[1] * 2000.0).round() as i64, (p[2] * 2000.0).round() as i64);
    let mut acc: HashMap<(i64, i64, i64), [f32; 3]> = HashMap::new();
    for (p, n) in pos.iter().zip(nrm) {
        let e = acc.entry(key(p)).or_insert([0.0; 3]);
        for k in 0..3 {
            e[k] += n[k];
        }
    }
    pos.iter().map(|p| norm3(acc[&key(p)])).collect()
}

// ----------------------------------------------------------------------
// Catálogo de modelos (assets/modelos/modelos.txt)
// ----------------------------------------------------------------------

/// Una línea del catálogo: `rol = archivo.glb  clave=valor ...`
#[derive(Clone, Debug)]
pub struct Entry {
    pub role: String,
    pub file: PathBuf,
    pub opts: HashMap<String, String>,
}

impl Entry {
    pub fn num(&self, k: &str) -> Option<f32> {
        self.opts.get(k).and_then(|v| v.replace(',', ".").parse().ok())
    }
    pub fn vec3(&self, k: &str) -> Option<[f32; 3]> {
        let v: Vec<f32> = self.opts.get(k)?.split(['/', ';', ' ']).filter_map(|x| x.trim().replace(',', ".").parse().ok()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    }
    pub fn text(&self, k: &str) -> Option<&str> {
        self.opts.get(k).map(String::as_str)
    }
    pub fn yes(&self, k: &str) -> bool {
        matches!(self.text(k), Some("si" | "sí" | "1" | "true" | "yes"))
    }
}

/// Carpetas donde se buscan los modelos, por orden de preferencia:
/// la variable `DEIA_MODELOS`, la carpeta del usuario, junto al ejecutable,
/// la carpeta actual y la instalación del sistema (Linux).
pub fn search_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("DEIA_MODELOS") {
        v.push(PathBuf::from(p));
    }
    if let Some(c) = crate::config::config_dir() {
        v.push(c.join("modelos"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.join("assets").join("modelos"));
            v.push(d.join("modelos"));
            v.push(d.join("../share/eginbeharraren-deia/modelos"));
        }
    }
    v.push(PathBuf::from("assets/modelos"));
    v.push(PathBuf::from("/usr/share/eginbeharraren-deia/modelos"));
    v
}

pub fn parse_catalog(text: &str, dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((role, rest)) = line.split_once('=') else { continue };
        let role = role.trim().to_lowercase();
        let mut parts = rest.split_whitespace();
        let Some(file) = parts.next() else { continue };
        let mut opts = HashMap::new();
        for p in parts {
            if let Some((k, v)) = p.split_once('=') {
                opts.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
        out.push(Entry { role, file: dir.join(file), opts });
    }
    out
}

/// Lee el primer `modelos.txt` encontrado. Las entradas cuyo archivo no
/// existe se descartan (y el juego usa sus modelos de siempre).
pub fn catalog() -> Vec<Entry> {
    for d in search_dirs() {
        let f = d.join("modelos.txt");
        if let Ok(text) = std::fs::read_to_string(&f) {
            return parse_catalog(&text, &d).into_iter().filter(|e| e.file.is_file()).collect();
        }
    }
    Vec::new()
}

/// Huella de un archivo (para comprobar que el cliente y el servidor usan
/// el mismo escenario).
pub fn fingerprint(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h.max(1)
}

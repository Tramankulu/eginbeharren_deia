//! Pruebas del cargador glTF y de los escenarios hechos en Blender.

use deia::model::*;
use deia::world::World;

/// Codifica bytes en base64 (para meter el búfer en el propio .gltf).
fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                s.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Un cubo unidad (8 vértices, 12 triángulos) y un búfer con todo.
fn cube() -> (Vec<u8>, usize, usize) {
    let mut pos = Vec::new();
    for i in 0..8 {
        pos.extend_from_slice(&[(i & 1) as f32 - 0.5, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32 - 0.5]);
    }
    let idx: [u16; 36] = [0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5];
    let mut buf = f32s(&pos);
    let ioff = buf.len();
    buf.extend(idx.iter().flat_map(|x| x.to_le_bytes()));
    (buf, ioff, idx.len())
}

#[test]
fn escenario_desde_nombres() {
    let (buf, ioff, icount) = cube();
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,1,2,3,4,5]}}],
        "nodes":[
          {{"name":"Casa","mesh":0,"translation":[10,0,0],"scale":[4,3,4]}},
          {{"name":"COL_muro","mesh":0,"translation":[10,0,0],"scale":[4,3,4]}},
          {{"name":"SALIDA_ALIADOS.001","translation":[0,0,-20]}},
          {{"name":"SALIDA_EJE","translation":[0,0,20]}},
          {{"name":"PASO_1","translation":[0,0,0]}},
          {{"name":"LIMITE","translation":[30,0,30]}}],
        "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
        "accessors":[
          {{"bufferView":0,"componentType":5126,"count":8,"type":"VEC3","min":[-0.5,0,-0.5],"max":[0.5,1,0.5]}},
          {{"bufferView":1,"componentType":5123,"count":{icount},"type":"SCALAR"}}],
        "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{ioff}}},{{"buffer":0,"byteOffset":{ioff},"byteLength":{}}}],
        "buffers":[{{"byteLength":{},"uri":"data:application/octet-stream;base64,{}"}}]}}"#,
        buf.len() - ioff,
        buf.len(),
        b64(&buf)
    );
    let dir = std::env::temp_dir().join(format!("deia-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mapa.gltf");
    std::fs::write(&path, &json).unwrap();

    let m = Model::load(&path).unwrap();
    assert_eq!(m.nodes.len(), 6);
    assert_eq!(m.drawables().len(), 2);
    let w = World::from_scene(&path, 1.0).unwrap();
    assert_eq!(w.blocks.len(), 1, "solo COL_muro es colisión");
    let b = w.blocks[0].b;
    assert!((b.min.x - 8.0).abs() < 1e-4 && (b.max.x - 12.0).abs() < 1e-4 && (b.max.y - 3.0).abs() < 1e-4, "{b:?}");
    assert_eq!(w.allied_spawns.len(), 1);
    assert_eq!(w.axis_spawns.len(), 1);
    assert!((w.half - 30.0).abs() < 1e-4);
    assert!(w.map_id != 0);
    // Un muro en medio corta el paso
    assert!(!w.walk_clear(deia::math::v3(5.0, 0.0, 0.0), deia::math::v3(15.0, 0.0, 0.0)));
    assert!(w.links.iter().any(|l| !l.is_empty()));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn esqueleto_y_animacion() {
    // Dos huesos: el segundo gira 90° sobre Z a lo largo de 1 s
    let (cbuf, ioff, icount) = cube();
    let mut buf = cbuf;
    let nverts = 8;
    // Articulaciones: los vértices de arriba (y=1) dependen del hueso 1
    let joints_off = buf.len();
    for i in 0..nverts {
        let j = if (i >> 1) & 1 == 1 { 1u8 } else { 0 };
        buf.extend_from_slice(&[j, 0, 0, 0]);
    }
    let weights_off = buf.len();
    for _ in 0..nverts {
        buf.extend(f32s(&[1.0, 0.0, 0.0, 0.0]));
    }
    let ibm_off = buf.len();
    let id = [1.0f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
    buf.extend(f32s(&id));
    buf.extend(f32s(&id));
    let t_off = buf.len();
    buf.extend(f32s(&[0.0, 1.0]));
    let r_off = buf.len();
    let h = std::f32::consts::FRAC_1_SQRT_2;
    buf.extend(f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, h, h]));
    let end = buf.len();
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scenes":[{{"nodes":[0,1]}}],
        "nodes":[{{"name":"cuerpo","mesh":0,"skin":0}},{{"name":"raiz","children":[2]}},{{"name":"Spine2"}}],
        "skins":[{{"joints":[1,2],"inverseBindMatrices":4}}],
        "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"JOINTS_0":2,"WEIGHTS_0":3}},"indices":1}}]}}],
        "animations":[{{"name":"Walk","samplers":[{{"input":5,"output":6}}],"channels":[{{"sampler":0,"target":{{"node":2,"path":"rotation"}}}}]}}],
        "accessors":[
          {{"bufferView":0,"componentType":5126,"count":8,"type":"VEC3"}},
          {{"bufferView":1,"componentType":5123,"count":{icount},"type":"SCALAR"}},
          {{"bufferView":2,"componentType":5121,"count":8,"type":"VEC4"}},
          {{"bufferView":3,"componentType":5126,"count":8,"type":"VEC4"}},
          {{"bufferView":4,"componentType":5126,"count":2,"type":"MAT4"}},
          {{"bufferView":5,"componentType":5126,"count":2,"type":"SCALAR"}},
          {{"bufferView":6,"componentType":5126,"count":2,"type":"VEC4"}}],
        "bufferViews":[
          {{"buffer":0,"byteOffset":0,"byteLength":{ioff}}},
          {{"buffer":0,"byteOffset":{ioff},"byteLength":{}}},
          {{"buffer":0,"byteOffset":{joints_off},"byteLength":{}}},
          {{"buffer":0,"byteOffset":{weights_off},"byteLength":{}}},
          {{"buffer":0,"byteOffset":{ibm_off},"byteLength":128}},
          {{"buffer":0,"byteOffset":{t_off},"byteLength":8}},
          {{"buffer":0,"byteOffset":{r_off},"byteLength":32}}],
        "buffers":[{{"byteLength":{end},"uri":"data:application/octet-stream;base64,{}"}}]}}"#,
        joints_off - ioff,
        weights_off - joints_off,
        ibm_off - weights_off,
        b64(&buf)
    );
    let m = Model::from_bytes(json.as_bytes(), None).unwrap();
    assert_eq!(m.find_anim(&["walk"], &[]), Some(0));
    assert_eq!(m.find_node(&["spine2"]), Some(2));
    let mut posed = Vec::new();
    m.pose(&m.rest_globals(), &mut posed);
    let rest = posed[0].pos[2]; // (-0.5, 1, -0.5), hueso 1
    assert!((rest[0] + 0.5).abs() < 1e-5 && (rest[1] - 1.0).abs() < 1e-5);
    let g = m.globals(&m.local_pose(&[(0, 0.999, 1.0)]));
    m.pose(&g, &mut posed);
    let p = posed[0].pos[2];
    // Girado 90° sobre Z: (x, y) -> (-y, x)
    assert!((p[0] + 1.0).abs() < 0.02 && (p[1] + 0.5).abs() < 0.02, "{p:?}");
    let still = posed[0].pos[0]; // hueso 0, quieto
    assert!((still[0] + 0.5).abs() < 1e-5 && still[1].abs() < 1e-5);
    // Mezcla al 50 %: unos 45°
    let g = m.globals(&m.local_pose(&[(0, 0.999, 0.5), (0, 0.0, 0.5)]));
    m.pose(&g, &mut posed);
    let p = posed[0].pos[2];
    let ang = p[1].atan2(p[0]).to_degrees();
    let rest_ang = 1.0f32.atan2(-0.5).to_degrees();
    assert!(((ang - rest_ang) - 45.0).abs() < 3.0, "{ang} {rest_ang}");
}

#[test]
fn catalogo() {
    let dir = std::path::Path::new("/modelos");
    let e = parse_catalog("# comentario\nsoldado_aliado = gi.glb altura=1,8 giro=180\n\nvista_eje_fusil=k98.glb cadera=0.1/-0.2/0.3\n", dir);
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].role, "soldado_aliado");
    assert_eq!(e[0].num("altura"), Some(1.8));
    assert_eq!(e[0].file, dir.join("gi.glb"));
    assert_eq!(e[1].vec3("cadera"), Some([0.1, -0.2, 0.3]));
}

#[test]
fn json_basico() {
    let j = Json::parse(r#"{"a":[1,2.5,-3e2],"b":"hóla \"x\"","c":true,"d":null}"#).unwrap();
    assert_eq!(j.get("a").at(2).f(), Some(-300.0));
    assert_eq!(j.get("b").s(), Some("hóla \"x\""));
    assert_eq!(j.get("c").b(), Some(true));
    assert!(j.get("d").is_null() && j.get("zz").is_null());
}

//! Sombreador del mundo (GLSL ES 1.00): luz cálida de tarde, sombras
//! azuladas, cielo pintado, nubes algodonosas y detalle procedural de cada
//! material. Los datos llegan en el vértice:
//!   color    → color base
//!   normal   → normal (xyz) y luz solar precalculada (w)
//!   texcoord → oclusión ambiental (x) y material (y)

pub const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;

varying lowp vec4 color;
varying highp vec3 wpos;
varying mediump vec4 nrm;
varying mediump vec2 info;

uniform mat4 Model;
uniform mat4 Projection;
uniform float Time;

void main() {
    vec4 w = Model * vec4(position, 1.0);
    // Viento: la hierba se mece y las hojas tiemblan un poco
    float m = texcoord.y;
    if (m > 12.5 && m < 13.5) {
        float k = clamp(w.y * 2.2, 0.0, 1.0);
        float ph = Time * 1.7 + w.x * 0.35 + w.z * 0.27;
        w.x += (sin(ph) * 0.07 + sin(ph * 2.3) * 0.025) * k;
        w.z += cos(ph * 0.8) * 0.04 * k;
    } else if (m > 6.5 && m < 7.5) {
        float ph = Time * 1.3 + w.x * 0.5 + w.y * 0.4;
        w.xyz += vec3(sin(ph), sin(ph * 1.3) * 0.5, cos(ph * 0.9)) * 0.018;
    }
    gl_Position = Projection * w;
    wpos = w.xyz;
    color = color0 / 255.0;
    nrm = normal;
    info = texcoord;
}
"#;

pub const FRAGMENT: &str = r#"#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

varying lowp vec4 color;
varying highp vec3 wpos;
varying mediump vec4 nrm;
varying mediump vec2 info;

uniform sampler2D Texture;
uniform vec3 SunDir;
uniform vec3 CamPos;
uniform float Time;

float hash(vec3 p) {
    p = fract(p * 0.3183099 + vec3(0.11, 0.17, 0.13));
    p *= 17.0;
    return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

float noise(vec3 x) {
    vec3 i = floor(x);
    vec3 f = fract(x);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(mix(hash(i), hash(i + vec3(1.0, 0.0, 0.0)), f.x),
                   mix(hash(i + vec3(0.0, 1.0, 0.0)), hash(i + vec3(1.0, 1.0, 0.0)), f.x), f.y),
               mix(mix(hash(i + vec3(0.0, 0.0, 1.0)), hash(i + vec3(1.0, 0.0, 1.0)), f.x),
                   mix(hash(i + vec3(0.0, 1.0, 1.0)), hash(i + vec3(1.0, 1.0, 1.0)), f.x), f.y), f.z);
}

// Piedras irregulares: distancia al centro de la celda de Voronoi más
// cercana (x) y diferencia con la segunda (y, cerca de 0 = junta)
vec3 stones(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    float d1 = 8.0;
    float d2 = 8.0;
    float id = 0.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            vec2 g = vec2(float(x), float(y));
            vec2 o = vec2(hash(vec3(i + g, 1.7)), hash(vec3(i + g, 4.3))) * 0.8 + 0.1;
            float d = length(g + o - f);
            if (d < d1) {
                d2 = d1;
                d1 = d;
                id = hash(vec3(i + g, 9.1));
            } else if (d < d2) {
                d2 = d;
            }
        }
    }
    return vec3(d1, d2 - d1, id);
}

float fbm(vec3 p) {
    return 0.5 * noise(p) + 0.25 * noise(p * 2.03 + 1.7) + 0.125 * noise(p * 4.01 + 3.1);
}

vec3 sky(vec3 d) {
    float h = d.y;
    vec3 zenith = vec3(0.22, 0.48, 0.86);
    vec3 horizon = vec3(0.80, 0.89, 0.95);
    vec3 c = mix(horizon, zenith, pow(clamp(h, 0.0, 1.0), 0.5));
    float s = max(dot(d, SunDir), 0.0);
    c = mix(c, vec3(1.0, 0.93, 0.78), pow(s, 6.0) * 0.35);
    c += vec3(1.0, 0.86, 0.6) * pow(s, 300.0) * 1.4;
    if (h < 0.0) {
        c = mix(c, vec3(0.66, 0.78, 0.74), clamp(-h * 4.0, 0.0, 1.0));
    }
    return c;
}

void main() {
    float mat = info.y;
    vec3 base = color.rgb * texture2D(Texture, vec2(0.5, 0.5)).rgb;
    vec3 p = wpos;

    // ---- Sin iluminación ----
    if (mat > 19.5) {
        vec2 uv = nrm.xy;
        float r = length(uv);
        if (mat < 20.5) {
            // Cielo
            gl_FragColor = vec4(sky(normalize(p - CamPos)), 1.0);
            return;
        }
        if (mat < 21.5) {
            // Nube: bola suave, iluminada por arriba, base azulada
            float a = 1.0 - smoothstep(0.62, 1.0, r);
            if (a < 0.01) discard;
            float lit = clamp(0.5 + 0.5 * uv.y * 0.8 + nrm.z * 0.45 + 0.25 * dot(normalize(vec3(uv, 0.6)), normalize(SunDir + vec3(0.0, 0.3, 0.0))), 0.0, 1.0);
            vec3 shadowc = vec3(0.66, 0.73, 0.86);
            vec3 lightc = vec3(1.0, 0.98, 0.94);
            vec3 c = mix(shadowc, lightc, smoothstep(0.25, 0.85, lit));
            c += vec3(1.0, 0.92, 0.75) * (1.0 - smoothstep(0.5, 1.0, r)) * 0.04;
            float fade = clamp((p.y - CamPos.y) / 60.0, 0.0, 1.0);
            c = mix(sky(normalize(p - CamPos)), c, 0.25 + 0.75 * fade);
            gl_FragColor = vec4(c, a * color.a);
            return;
        }
        if (mat < 22.5) {
            // Destello o luz propia
            gl_FragColor = color;
            return;
        }
        if (mat < 23.5) {
            // Sombra de contacto bajo personajes
            float a = 1.0 - smoothstep(0.2, 1.0, r);
            gl_FragColor = vec4(color.rgb, color.a * a);
            return;
        }
        // Humo y polvo: disco suave
        float a = (1.0 - smoothstep(0.3, 1.0, r)) * color.a;
        if (a < 0.01) discard;
        gl_FragColor = vec4(color.rgb * (0.85 + 0.25 * (uv.y * 0.5 + 0.5)), a);
        return;
    }

    vec3 n = normalize(nrm.xyz);
    float sunv = nrm.w;
    float ao = info.x;
    vec3 plain = base;
    // De lejos, el detalle se disuelve en manchas (como en un fondo pintado)
    float near = 1.0 - smoothstep(16.0, 42.0, length(p - CamPos));

    // ---- Detalle pintado de cada material ----
    if (mat < 0.5) {
        // liso
    } else if (mat < 1.5) {
        // Hierba: manchas de color y matices amarillos
        base *= 0.86 + 0.28 * fbm(p * 0.45);
        base = mix(base, base * vec3(1.12, 1.08, 0.72), smoothstep(0.55, 0.8, noise(p * 0.07)) * 0.5);
    } else if (mat < 2.5) {
        // Encalado: irregular y algo más sucio cerca del suelo
        base *= 0.94 + 0.09 * fbm(p * 1.3);
        base *= mix(0.86, 1.0, smoothstep(0.0, 0.9, p.y));
    } else if (mat < 3.5) {
        // Ladrillo: hiladas con llagas
        vec3 an = abs(n);
        float along = an.x > an.z ? p.z : p.x;
        float row = floor(p.y / 0.21);
        float col = floor((along + mod(row, 2.0) * 0.22) / 0.44);
        float fy = fract(p.y / 0.21);
        float fx = fract((along + mod(row, 2.0) * 0.22) / 0.44);
        float mortar = step(fy, 0.1) + step(fx, 0.05);
        base *= 0.85 + 0.25 * hash(vec3(row, col, 1.0));
        base = mix(base, vec3(0.82, 0.78, 0.70), clamp(mortar, 0.0, 1.0) * 0.8);
    } else if (mat < 4.5) {
        // Mampostería de piedras redondeadas, con musgo arriba y en la base
        vec3 an = abs(n);
        vec2 q = an.y > 0.6 ? p.xz : vec2(an.x > an.z ? p.z : p.x, p.y);
        vec3 st = stones(q * vec2(2.0, 2.7));
        base *= 0.8 + 0.32 * st.z;
        base *= 0.82 + 0.18 * smoothstep(0.0, 0.5, 1.0 - st.x);
        base = mix(base * 0.55, base, smoothstep(0.02, 0.1, st.y));
        float moss = smoothstep(0.55, 0.9, n.y) * smoothstep(0.35, 0.7, fbm(p * 0.8)) + (1.0 - smoothstep(0.0, 0.5, p.y)) * 0.4 * fbm(p * 1.5);
        base = mix(base, vec3(0.38, 0.55, 0.26), clamp(moss, 0.0, 0.8));
    } else if (mat < 5.5) {
        // Tejas en hileras
        float row = floor(p.y * 3.4 + noise(p * 2.0) * 0.3);
        float fy = fract(p.y * 3.4);
        float col = floor((p.x + p.z) * 2.2 + mod(row, 2.0) * 0.5);
        base *= 0.82 + 0.28 * hash(vec3(row, col, 5.0));
        base *= 0.78 + 0.22 * smoothstep(0.0, 0.35, fy);
        base = mix(base, vec3(0.42, 0.55, 0.32), smoothstep(0.7, 0.9, fbm(p * 0.6)) * 0.35);
    } else if (mat < 6.5) {
        // Madera con veta
        float grain = noise(vec3(p.x * 1.2, p.y * 9.0, p.z * 1.2) + vec3(p.z * 2.0, 0.0, p.x * 2.0));
        base *= 0.84 + 0.25 * grain;
    } else if (mat < 7.5) {
        // Follaje: grumos de luz
        base *= 0.78 + 0.42 * fbm(p * 0.9);
    } else if (mat < 8.5) {
        // Tela
        base *= 0.93 + 0.1 * noise(p * 14.0);
    } else if (mat < 9.5) {
        // Piel
    } else if (mat < 10.5) {
        // Tierra con piedrecitas y roderas
        base *= 0.84 + 0.3 * fbm(p * 0.7);
        base *= 1.0 - 0.18 * step(0.82, noise(p * 9.0));
    } else if (mat < 11.5) {
        // Metal con óxido
        float rust = smoothstep(0.55, 0.85, fbm(p * 1.6));
        base = mix(base, vec3(0.55, 0.33, 0.20), rust * 0.6);
    } else if (mat < 12.5) {
        // Paja
        base *= 0.85 + 0.25 * noise(vec3(p.x * 3.0, p.y * 18.0, p.z * 3.0));
    } else {
        // Briznas: el mismo matiz que el prado sobre el que crecen
        base *= 0.9 + 0.2 * noise(p * 0.45);
    }

    if (mat > 1.5 && mat < 5.5) {
        base = mix(plain * (0.94 + 0.1 * noise(p * 0.6)), base, near);
    }

    // ---- Iluminación de ilustración ----
    float ndl = dot(n, SunDir);
    // Terminador suave y algo irregular, como una pincelada
    float brush = (noise(p * 2.7) - 0.5) * 0.12;
    float lam = clamp(ndl + brush, 0.0, 1.0);
    float soft = smoothstep(0.0, 0.42, lam);
    float direct = mix(lam, soft, 0.7) * sunv;
    vec3 sunCol = vec3(1.0, 0.91, 0.76) * 1.15;
    vec3 skyAmb = vec3(0.50, 0.60, 0.88);
    vec3 groundAmb = vec3(0.56, 0.54, 0.38);
    vec3 amb = mix(groundAmb, skyAmb, 0.5 + 0.5 * n.y) * (0.38 + 0.62 * ao) * 0.66;
    vec3 c = base * (sunCol * direct + amb);
    // Las sombras tiran a azul violáceo; las luces, a dorado
    float shadowness = 1.0 - clamp(direct, 0.0, 1.0);
    c = mix(c, c * vec3(0.86, 0.9, 1.12), shadowness * 0.5);
    c = mix(c, c * vec3(1.06, 1.0, 0.9), clamp(direct, 0.0, 1.0) * 0.4);
    // Hojas y hierba dejan pasar algo de luz
    if (mat > 6.5 && mat < 7.5) {
        c += base * vec3(0.9, 1.0, 0.5) * max(-ndl, 0.0) * sunv * 0.18;
    }
    // Borde iluminado suave contra el cielo
    vec3 v = p - CamPos;
    float d = length(v);
    vec3 vd = v / max(d, 0.001);
    float rim = pow(1.0 - clamp(dot(n, -vd), 0.0, 1.0), 3.0);
    c += sunCol * rim * 0.12 * sunv * clamp(dot(vd, SunDir) * 0.5 + 0.5, 0.0, 1.0);

    // ---- Perspectiva aérea ----
    float fog = 1.0 - exp(-pow(d * 0.0042, 1.35));
    vec3 haze = sky(normalize(vec3(vd.x, max(vd.y, 0.02) * 0.5, vd.z)));
    c = mix(c, haze, clamp(fog, 0.0, 0.9));

    gl_FragColor = vec4(c, color.a);
}
"#;

/// Filtro de pintura (Kuwahara en cuatro sectores): cada píxel toma el
/// color medio del sector más uniforme de su vecindad. Los bordes quedan
/// definidos y las superficies se resuelven en manchas, como en un fondo
/// pintado a mano. Encima, una veladura de papel y un ligero ajuste de color.
pub const PAINT_VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying lowp vec4 color;
varying mediump vec2 uv;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    color = color0 / 255.0;
    uv = texcoord;
}
"#;

pub const PAINT_FRAGMENT: &str = r#"#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

varying lowp vec4 color;
varying mediump vec2 uv;

uniform sampler2D Texture;
uniform vec2 Texel;
uniform float Spread;

float hash2(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

void main() {
    vec2 d = Texel * Spread;
    vec3 m0 = vec3(0.0); vec3 m1 = vec3(0.0); vec3 m2 = vec3(0.0); vec3 m3 = vec3(0.0);
    vec3 s0 = vec3(0.0); vec3 s1 = vec3(0.0); vec3 s2 = vec3(0.0); vec3 s3 = vec3(0.0);
    for (int j = 0; j <= 3; j++) {
        for (int i = 0; i <= 3; i++) {
            vec2 o = vec2(float(i), float(j)) * d;
            vec3 a = texture2D(Texture, uv + vec2(-o.x, -o.y)).rgb;
            vec3 b = texture2D(Texture, uv + vec2(o.x, -o.y)).rgb;
            vec3 c = texture2D(Texture, uv + vec2(o.x, o.y)).rgb;
            vec3 e = texture2D(Texture, uv + vec2(-o.x, o.y)).rgb;
            m0 += a; s0 += a * a;
            m1 += b; s1 += b * b;
            m2 += c; s2 += c * c;
            m3 += e; s3 += e * e;
        }
    }
    float n = 16.0;
    m0 /= n; m1 /= n; m2 /= n; m3 /= n;
    vec3 v0 = abs(s0 / n - m0 * m0);
    vec3 v1 = abs(s1 / n - m1 * m1);
    vec3 v2 = abs(s2 / n - m2 * m2);
    vec3 v3 = abs(s3 / n - m3 * m3);
    // En vez de quedarse con un solo sector (bordes en escalera), se mezclan
    // los cuatro según lo uniformes que sean
    float w0 = 1.0 / (1e-5 + pow(v0.r + v0.g + v0.b, 1.6) * 600.0);
    float w1 = 1.0 / (1e-5 + pow(v1.r + v1.g + v1.b, 1.6) * 600.0);
    float w2 = 1.0 / (1e-5 + pow(v2.r + v2.g + v2.b, 1.6) * 600.0);
    float w3 = 1.0 / (1e-5 + pow(v3.r + v3.g + v3.b, 1.6) * 600.0);
    vec3 col = (m0 * w0 + m1 * w1 + m2 * w2 + m3 * w3) / (w0 + w1 + w2 + w3);

    // Color: un poco más de saturación y calidez, sombras levantadas
    float l = dot(col, vec3(0.299, 0.587, 0.114));
    col = mix(vec3(l), col, 1.12);
    col = col * vec3(1.02, 1.0, 0.97) + vec3(0.012, 0.014, 0.03) * (1.0 - l);

    // Grano de papel de acuarela
    vec2 px = uv / Texel;
    float grain = hash2(floor(px / 2.0)) * 0.5 + hash2(floor(px / 5.0) + 3.1) * 0.5;
    col *= 0.975 + 0.05 * grain;

    // Viñeta suave
    vec2 q = uv - 0.5;
    col *= 1.0 - dot(q, q) * 0.35;

    gl_FragColor = vec4(col, 1.0);
}
"#;

/// Modelos de artista (glTF): textura pintada con sombreado de anime.
/// Dos bandas de luz con el borde nítido, sombras azuladas, un brillo
/// pequeño y un filo de luz en el contorno. Con `Thick > 0` el mismo
/// programa dibuja el contorno de tinta: la malla se engorda a lo largo de
/// la normal (tanto más cuanto más lejos, para que la línea mida lo mismo en
/// pantalla) y solo se ven sus caras traseras.
///   color    → color del vértice por el del material
///   normal   → normal (xyz) y luz solar en ese punto (w)
///   texcoord → coordenadas de la textura
pub const ART_VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;

varying lowp vec4 color;
varying highp vec3 wpos;
varying mediump vec4 nrm;
varying mediump vec2 uv;

uniform mat4 Model;
uniform mat4 Projection;
uniform vec3 CamPos;
uniform float Thick;

void main() {
    vec3 p = position;
    if (Thick > 0.0) {
        float d = length(p - CamPos);
        p += normal.xyz * clamp(d * 0.0034, 0.004, 0.14) * Thick;
    }
    vec4 w = Model * vec4(p, 1.0);
    gl_Position = Projection * w;
    wpos = w.xyz;
    color = color0 / 255.0;
    nrm = normal;
    uv = texcoord;
}
"#;

pub const ART_FRAGMENT: &str = r#"#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

varying lowp vec4 color;
varying highp vec3 wpos;
varying mediump vec4 nrm;
varying mediump vec2 uv;

uniform sampler2D Texture;
uniform vec3 SunDir;
uniform vec3 CamPos;
uniform float Thick;
uniform float Cut;

vec3 sky(vec3 d) {
    float h = d.y;
    vec3 zenith = vec3(0.22, 0.48, 0.86);
    vec3 horizon = vec3(0.80, 0.89, 0.95);
    vec3 c = mix(horizon, zenith, pow(clamp(h, 0.0, 1.0), 0.5));
    float s = max(dot(d, SunDir), 0.0);
    c = mix(c, vec3(1.0, 0.93, 0.78), pow(s, 6.0) * 0.35);
    return c;
}

void main() {
    vec4 t = texture2D(Texture, uv) * color;
    if (t.a < Cut) {
        discard;
    }
    vec3 base = t.rgb;
    vec3 v = wpos - CamPos;
    float d = length(v);
    vec3 vd = v / max(d, 0.001);
    float fog = clamp(1.0 - exp(-pow(d * 0.0042, 1.35)), 0.0, 0.9);
    vec3 haze = sky(normalize(vec3(vd.x, max(vd.y, 0.02) * 0.5, vd.z)));

    if (Thick > 0.0) {
        // Tinta: oscura y cálida, del color de lo que rodea
        vec3 ink = base * vec3(0.30, 0.25, 0.24) + vec3(0.035, 0.03, 0.035);
        gl_FragColor = vec4(mix(ink, haze, fog * 0.85), 1.0);
        return;
    }

    vec3 n = normalize(nrm.xyz);
    if (!gl_FrontFacing) {
        n = -n;
    }
    float sunv = nrm.w;
    float ndl = dot(n, SunDir);
    // Dos bandas: luz y sombra, con un escalón intermedio dentro de la sombra
    float lit = smoothstep(0.0, 0.07, ndl) * sunv;
    float mid = smoothstep(-0.42, -0.32, ndl);
    vec3 sunCol = vec3(1.0, 0.94, 0.82) * 1.07;
    vec3 shade = mix(vec3(0.50, 0.53, 0.72), vec3(0.62, 0.65, 0.82), mid);
    // El cielo aclara un poco lo que mira hacia arriba
    shade *= 0.92 + 0.12 * clamp(n.y, 0.0, 1.0);
    vec3 c = base * mix(shade, sunCol, lit);
    // Brillo pequeño y recortado
    vec3 h = normalize(SunDir - vd);
    c += vec3(1.0, 0.96, 0.86) * smoothstep(0.93, 0.955, dot(n, h)) * lit * 0.14;
    // Filo de luz en el contorno
    float rim = smoothstep(0.64, 0.74, 1.0 - max(dot(n, -vd), 0.0));
    c += base * sunCol * rim * (0.12 + 0.2 * lit);

    gl_FragColor = vec4(mix(c, haze, fog), t.a);
}
"#;

/// Visión herida: la imagen se emborrona y pierde color según `Strength`.
pub const HURT_FRAGMENT: &str = r#"#version 100
precision mediump float;
varying lowp vec4 color;
varying mediump vec2 uv;
uniform sampler2D Texture;
uniform vec2 Texel;
uniform float Strength;

void main() {
    vec2 c = uv - 0.5;
    // Más borroso hacia los bordes
    float edge = clamp(length(c) * 1.6, 0.0, 1.0);
    float r = Strength * (2.0 + 6.0 * edge);
    vec3 acc = vec3(0.0);
    float w = 0.0;
    for (int y = -2; y <= 2; y++) {
        for (int x = -2; x <= 2; x++) {
            vec2 o = vec2(float(x), float(y));
            float k = exp(-dot(o, o) * 0.35);
            acc += texture2D(Texture, uv + o * Texel * r).rgb * k;
            w += k;
        }
    }
    vec3 col = acc / w;
    float grey = dot(col, vec3(0.3, 0.59, 0.11));
    col = mix(col, vec3(grey) * vec3(1.05, 0.92, 0.9), Strength * 0.6);
    gl_FragColor = vec4(col, 1.0);
}
"#;

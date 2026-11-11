# Modelos de artista

El juego trae modelos hechos con código, que sirven para jugar pero no tienen
el aspecto de las ilustraciones de referencia (`docs/referencias/`). Para
llegar a ese aspecto hacen falta modelos 3D con **texturas pintadas a mano**.
El juego ya está preparado para cargarlos: no hay que tocar el código.

Basta con dejar los archivos en la carpeta de modelos y apuntarlos en
`modelos.txt`. Lo que falte se sigue dibujando como siempre, así que se puede
ir sustituyendo poco a poco.

## Dónde van

Cualquiera de estas carpetas, por orden de preferencia:

| Sistema | Carpeta del usuario (recomendada) | Carpeta de la instalación |
|---|---|---|
| Windows | `%APPDATA%\EginbeharrarenDeia\modelos\` | `C:\Program Files\Eginbeharraren deia\assets\modelos\` |
| Ubuntu | `~/.config/eginbeharraren-deia/modelos/` | `/usr/share/eginbeharraren-deia/modelos/` |

Para probar sin instalar nada, también vale `DEIA_MODELOS=/ruta/a/la/carpeta`.

Cada carpeta lleva su `modelos.txt` (hay una plantilla en
`assets/modelos/modelos.txt`):

```
soldado_aliado = soldado_aliado.glb   altura=1.8
vista_eje_fusil = kar98k_fp.glb       cadera=0.16/-0.15/0.28
escenario = carentan.glb
```

> **Partidas en línea.** Los modelos de personajes, armas y objetos son solo
> visuales: cada jugador puede tener los suyos. El **escenario** no: cambia
> las colisiones, así que el servidor y todos los jugadores deben tener el
> mismo archivo. Si no coincide, el servidor rechaza la conexión con un aviso.

## Formato

- **glTF 2.0**, preferiblemente `.glb` (un solo archivo con texturas dentro).
  En Blender: *Archivo › Exportar › glTF 2.0*, formato *glTF Binary*.
- Texturas en **PNG o JPEG**. Solo se usa el color base (*Base Color*): la luz,
  las sombras y el contorno de tinta los pone el juego, como en el anime.
  Las texturas deben venir **pintadas a mano**, con sombras suaves ya
  dibujadas; nada de texturas fotográficas ni de mapas PBR.
- Metros, **Y hacia arriba** y el modelo **mirando a +Z**. Es el convenio de
  glTF; Blender lo convierte solo al exportar con *+Y Up*.
- Materiales *Opaque* para casi todo. *Alpha Clip* para hojas, redes del
  casco o alambradas, y *Alpha Blend* solo para cristales y humo.
- Si un modelo sale girado, se arregla con `giro=180` (grados) sin volver a
  exportarlo.

### Opciones de `modelos.txt`

| Opción | Para qué |
|---|---|
| `altura=1.8` | Ajusta el tamaño para que mida eso (en metros) |
| `escala=0.01` | Escala fija (en lugar de `altura`) |
| `giro=180` | Gira el modelo alrededor del eje vertical (grados) |
| `contorno=no` | Sin contorno de tinta |
| `cadera=x/y/z`, `apuntar=x/y/z` | Posición del arma en primera persona, desde el ojo (derecha / arriba / delante) |
| `boca=x/y/z` | Boca del cañón, para el fogonazo (si no hay un objeto `boca`) |
| `anim_quieto=…`, `anim_andar=…`, `anim_correr=…`, `anim_agachado=…`, `anim_agachado_andar=…`, `anim_morir=…`, `anim_apuntar=…` | Nombre exacto de cada animación, si no se encuentra sola |
| `columna=…` | Hueso del torso que se inclina al apuntar arriba o abajo |
| `vel_anim=1.2` | Velocidad de las animaciones |

## Lo que hace falta encargar o comprar

Por orden de impacto en el aspecto del juego:

### 1. Soldados — `soldado_aliado`, `soldado_eje` (y variantes `_subfusil`)

- Personaje completo **con su arma en las manos**: fusil (M1 Garand o
  Kar98k) o subfusil (Thompson o MP40).
- Estilo de las referencias:
  - **Aliados:** casco M1 con red, uniforme verde oliva y bandera en el hombro.
  - **Eje:** casco Stahlhelm, uniforme gris y la cruz de hierro de los vehículos.
  - **Los dos:** cara expresiva de anime, mochila y correajes, y pliegues de
    la tela pintados en la textura.
- **Sin símbolos nazis** (esvásticas, runas SS). Son ilegales en juegos en
  Alemania y en otros países, y las tiendas los rechazan.
- Entre 8.000 y 25.000 triángulos, con una o dos texturas de 2048 px.
- Con esqueleto y estas animaciones, en bucle salvo la de morir.
  Los nombres pueden ser en inglés:

  | Animación | Nombres que reconoce el juego |
  |---|---|
  | Quieto con el arma | `idle`, `quieto`, `rifle idle` |
  | Andar | `walk`, `andar` |
  | Correr | `run`, `correr`, `sprint` |
  | Agachado quieto | `crouch idle`, `agachado` |
  | Agachado andando | `crouch walk`, `agachado_andar` |
  | Morir (sin bucle) | `death`, `die`, `morir` |

  Las animaciones de **Mixamo** (gratis con una cuenta de Adobe) sirven tal
  cual: se aplican al personaje en Mixamo, se juntan en Blender y se exporta
  un solo `.glb` con todas. Si falta alguna, el juego se apaña: sin agacharse
  baja el cuerpo y sin morir lo tumba.
- El torso se inclina solo al apuntar arriba o abajo. El juego busca un hueso
  llamado `Spine2`, `Spine1` o `Chest`, como los de Mixamo.

### 2. Armas en primera persona — `vista_*_fusil`, `vista_*_subfusil`

- Los **brazos con las mangas del uniforme** y el arma, como en las capturas
  de referencia.
- El origen (0, 0, 0) del modelo debe estar en el **alza** (la mira trasera),
  con el cañón hacia +Z. Así, al apuntar, la mira queda en el centro de la
  pantalla sin más ajustes.
- Un objeto vacío llamado `boca` en la punta del cañón, para el fogonazo.
- Con detalle (es lo que más se ve): de 5.000 a 15.000 triángulos.

### 3. Escenario — `escenario` (Carentan)

Se hace entero en Blender, y el juego lo entiende por los **nombres de los
objetos**:

| Nombre del objeto | Qué es |
|---|---|
| `COL…` (por ejemplo `COL_muro.012`) | Caja de colisión: se usa su caja envolvente y **no se dibuja**. Lo normal es hacer cubos sencillos que envuelvan cada muro, casa o coche. Un sufijo da el material para el sonido y los impactos: `COL_madera`, `COL_metal`, `COL_seto`, `COL_saco`, `COL_paja`, `COL_ladrillo`, `COL_tierra` (si no lleva, es piedra). |
| `SALIDA_ALIADOS…`, `SALIDA_EJE…` | Puntos de reaparición: varios de cada uno, con un objeto vacío por punto. Son obligatorios. |
| `BANDERA_ALIADOS`, `BANDERA_EJE` | Bases de las banderas (Captura la bandera) |
| `BOMBA_A`, `BOMBA_B` | Objetivos de Buscar y destruir |
| `PASO…` | Puntos de paso para que los bots sepan moverse. Si no hay, se reparten solos. |
| `LIMITE` | Un objeto en una esquina del área jugable (el mapa es un cuadrado centrado en el origen) |

- El **suelo jugable está a altura 0** y el mapa centrado en el origen. Unos
  150 × 150 m caben bien, como el pueblo actual.
- El terreno puede tener relieve visual, pero los jugadores caminan sobre las
  cajas `COL` y el suelo a 0. Para subir a un sitio hay que poner cajas.
- Las referencias marcan el estilo:
  - la iglesia gótica de aguja alta;
  - el puente de piedra de dos arcos sobre el río;
  - las casas de piedra clara con tejado de pizarra azulada y contraventanas;
  - las ruinas con escombros;
  - el cartel de «CARENTAN»;
  - un Sherman y un Panzer destruidos;
  - el jeep y los sacos terreros.
- Las sombras del sol se calculan con las cajas `COL`: cuanto mejor envuelvan
  a los edificios, mejores serán las sombras.

### 4. Objetos sueltos — `arbol`, `arbol2`…, `tanque`, `carro`, `barril`, `paja`

Sustituyen a los del pueblo de siempre, en los mismos sitios. Para los árboles
se pueden dar variantes (`arbol2`, `arbol3`) y se reparten al azar. Solo
tienen sentido si no hay `escenario`.

## Dónde conseguirlos

- **Encargo a un artista 3D** (ArtStation, Fiverr, foros de Blender): es lo
  único que da exactamente el estilo de las referencias. Hay que pasarle esta
  guía y la carpeta `docs/referencias/`. Hay que pedirle «stylized / anime /
  hand-painted, cel-shaded».
- **Tiendas** (Fab, Sketchfab Store, itch.io, CGTrader): buscar
  «stylized WW2 soldier», «anime soldier», «hand painted ww2»,
  «stylized Sherman», «stylized European village». Hay que comprobar que
  la licencia permite usar el modelo en un juego que se distribuye.
- **Mixamo**: animaciones gratis para cualquier personaje humanoide.

## Cómo se ve en el juego

- El sombreado es de anime: dos tonos (luz y sombra azulada) con el borde
  nítido, un brillo pequeño y un filo de luz en el contorno.
- Todos los modelos llevan un contorno de tinta del grosor de un trazo.
- Con modelos de artista no se aplica el *Efecto de pintura*: emborronaría
  las líneas.

Para comprobar un modelo nuevo sin jugar, el modo de capturas saca fotos del
pueblo, de los soldados y del arma:

```
DEIA_MODELOS=/ruta/modelos DEIA_CAPTURA=/ruta/fotos eginbeharraren-deia
```

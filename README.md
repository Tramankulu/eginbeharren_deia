# Eginbeharraren deia

Shooter **multijugador en línea** de la Segunda Guerra Mundial, escrito en Rust e inspirado en
los clásicos de 2005. Aliados contra Eje en Saint-Aubin, un pueblo normando en ruinas.

- **Windows y Ubuntu**, con instaladores de doble clic.
- **Cuatro modos**: Todos contra todos, Por equipos, Buscar y destruir y Captura la bandera.
- **Como en los viejos tiempos**: cualquiera crea partida desde el menú o lanza un servidor
  dedicado; los demás entran por IP o la ven aparecer sola en la red local.

## Instalar

Descarga el instalador de tu sistema desde la página **Releases** del repositorio:

| Sistema | Archivo | Notas |
|---|---|---|
| Windows | `EginbeharrarenDeia-…-Windows-Instalador.exe` | Crea accesos en el menú Inicio y el escritorio, y abre el cortafuegos para poder alojar partidas. También hay un `.zip` portable. |
| Ubuntu | `eginbeharraren-deia_…_amd64.deb` | Doble clic abre el Centro de aplicaciones; tras instalar, aparece en el menú de aplicaciones. Desde terminal: `sudo apt install ./eginbeharraren-deia_…_amd64.deb`. |

Windows puede mostrar el aviso de SmartScreen porque el instalador no está firmado:
pulsa *Más información › Ejecutar de todas formas*.

## Jugar

**En la misma red (casa, LAN party):** uno pulsa *Crear partida* y los demás entran en
*Unirse a una partida*, donde aparece automáticamente.

**Por internet:** quien aloja tiene que abrir (redirigir) el puerto **UDP 28960** de su router
hacia su ordenador y pasar su **IP pública** a los demás, que la escriben en *Unirse a una
partida › Dirección del servidor*. Puedes elegir otro puerto al crear la partida; entonces
los demás escriben `IP:puerto`.

**Servidor dedicado** (un PC o VPS siempre encendido, sin ventana):

```
eginbeharraren-deia-server --nombre "Mi servidor" --max 16 --modos tdm,dm,sd,ctf
```

| Opción | Significado |
|---|---|
| `--nombre` | Nombre que ven los jugadores |
| `--puerto` | Puerto UDP (por defecto 28960) |
| `--max` | Jugadores máximos (2–16) |
| `--modos` | Rotación: `dm`, `tdm`, `sd`, `ctf` |
| `--limite` | Límite de puntuación (sustituye al del modo) |
| `--tiempo` | Límite de tiempo en minutos |

En Windows hay un acceso «Servidor dedicado de Eginbeharraren deia» en el menú Inicio (edita
`servidor-dedicado.bat` para cambiar opciones). En Ubuntu, `eginbeharraren-deia-server` en una terminal.

### Modos

| Modo | Reglas |
|---|---|
| Todos contra todos | Gana el primero en llegar a 30 bajas (10 min). |
| Por equipos | Aliados contra Eje, 75 bajas (15 min). |
| Buscar y destruir | Rondas sin reaparición, 2 min 30 s. Los atacantes colocan la bomba (mantener F, 5 s) en el objetivo A o B; los defensores la desactivan (7 s) antes de 45 s. También se gana eliminando al otro equipo. Cambio de papeles a las 3 rondas; gana quien llegue a 4. Los caídos ven la ronda desde un compañero. |
| Captura la bandera | Roba la bandera enemiga y llévala a tu base con la tuya en casa. Si muere el portador, cae; un compañero la devuelve tocándola o vuelve sola en 30 s. 5 capturas (15 min). |

La partida empieza cuando hay al menos dos jugadores (uno por equipo en los modos por equipos);
mientras tanto se juega en calentamiento. Al acabar, marcador final y siguiente modo de la rotación.

### Armas

Cada bando tiene su fusil y su subfusil, y dos granadas por vida:
Aliados **M1 Garand** (semiautomático, el *ping* del peine) y **Thompson**;
Eje **Kar98k** (cerrojo, letal) y **MP40**. La salud se regenera si te cubres unos segundos.

### Controles

| Tecla | Acción |
|---|---|
| WASD | Moverse |
| Ratón | Mirar · clic izquierdo dispara · clic derecho apunta con la mira |
| R | Recargar |
| 1 / 2, Q o rueda | Cambiar de arma |
| G | Granada |
| F (mantener) | Colocar / desactivar bomba |
| C o Ctrl | Agacharse · Espacio salta · Shift corre |
| Tab | Marcador |
| T / Y | Chat a todos / a tu equipo |
| M | Cambiar de equipo |
| Esc | Menú (sensibilidad, desconectar) |

Nombre, sensibilidad, campo de visión, volumen e inversión del ratón se guardan en *Ajustes*.

## Cómo se generan los instaladores

Los compila GitHub gratis en sus máquinas Windows y Ubuntu (flujo
`.github/workflows/instaladores.yml`):

1. Crea un repositorio en GitHub y sube el proyecto:
   ```
   git remote add origin https://github.com/TU_USUARIO/eginbeharraren-deia.git
   git push -u origin main
   ```
2. Publica una versión:
   ```
   git tag v0.2.0
   git push origin v0.2.0
   ```
3. En unos 10 minutos aparecen los instaladores en la pestaña **Releases**. También se puede
   lanzar a mano en *Actions › Instaladores › Run workflow* (los archivos quedan como
   artefactos de esa ejecución).

Para una versión nueva, sube el número en `Cargo.toml` y crea la etiqueta correspondiente.

## Compilar desde el código

```
cargo run --release                       # el juego
cargo run --release --bin eginbeharraren-deia-server # el servidor dedicado
cargo test --release                      # pruebas de red
```

En Ubuntu hacen falta antes: `sudo apt install pkg-config libx11-dev libxi-dev libgl1-mesa-dev libasound2-dev`.

## Cómo funciona la red

- **Servidor autoritativo** a 60 ticks/s sobre UDP; envía 20 instantáneas por segundo.
- **Predicción en el cliente**: tu movimiento se simula al instante con el mismo código que usa
  el servidor y se reconcilia en cuanto llega su respuesta, así que no notas el ping al moverte.
- **Interpolación** de los demás jugadores 100 ms en el pasado, para que se muevan suaves.
- **Compensación de lag**: al disparar, el servidor rebobina a los demás hasta el instante que tú
  veías. Acertar donde ves es acertar.
- Eventos fiables (bajas, chat, avisos) sobre UDP con confirmación; comandos redundantes contra
  pérdidas de paquetes; límite de comandos por tick contra trampas de velocidad.

`tests/red.rs` levanta un servidor real y varios clientes por UDP en 127.0.0.1. Juega partidas
completas con bots (por equipos, todos contra todos y buscar y destruir) y prueba capturas de
bandera, colocación, desactivación y explosión de la bomba. También comprueba la predicción, el chat,
las desconexiones, el servidor lleno y la compensación de lag. Con un objetivo en movimiento
acierta 8 de 8 disparos con compensación, frente a 2 de 8 sin ella.

## Estructura

| Archivo | Contenido |
|---|---|
| `src/main.rs` | El juego: menús, entrada, dibujado, HUD, sonido (macroquad) |
| `src/bin/eginbeharraren-deia-server.rs` | Servidor dedicado |
| `src/server.rs` | Servidor: conexiones, modos, reapariciones, compensación de lag |
| `src/client.rs` | Cliente: conexión, predicción, interpolación, búsqueda en red local |
| `src/net.rs` | Protocolo binario y mensajes |
| `src/sim.rs` | Simulación compartida: movimiento, armas, granadas |
| `src/world.rs` | Mapa, colisiones, rayos, navegación |
| `src/geo.rs` | Modelos low-poly generados por código |
| `src/sfx.rs` | Sonidos sintetizados |
| `src/config.rs` | Ajustes guardados |
| `packaging/` | Instaladores de cada sistema |

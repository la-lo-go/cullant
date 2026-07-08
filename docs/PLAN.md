# Cullant — Plan de implementación

## Contexto

Aplicación de escritorio para Windows, open source y en inglés, cuyo objetivo es el **photo culling rápido**: cargar un "proyecto" (carpeta con subcarpetas), previsualizar imágenes a gran velocidad y clasificar/filtrar/eliminar mediante atajos de teclado rápidos y personalizables. Todo el estado se guarda en SQLite; las acciones pueden ser inmediatas o diferidas hasta un "commit" final.

**Funcionalidad diferencial**: modo espejo RAW+JPEG — un switch global que agrupa los archivos con el mismo nombre base y distinta extensión (IMG_001.CR3 + IMG_001.JPG) como una sola foto lógica, o los trata por separado. En modo espejo las acciones aplican al par por defecto, con posibilidad de desacoplar un par y comandos específicos ("borrar solo RAW", "borrar solo JPEG").

## Decisiones confirmadas con el usuario

| Tema | Decisión |
|---|---|
| Nombre | **Cullant** |
| Licencia | **GPL-3.0-or-later** (habilita rawler; norma del sector foto OSS) |
| Ubicación del repo | **`C:\dev\cullant`** (fuera de OneDrive — el `target/` de Rust y `node_modules/` rompen el sync; GitHub es el respaldo del código). La carpeta actual de OneDrive queda intacta |
| Stack | **Rust + Tauri v2** (frontend por decidir en diseño técnico) |
| Clasificación | Banderas pick/reject + estrellas 1–5 + etiquetas de color + acción de destino diferida |
| Etiquetas de tarea | Lista personalizable (trim, stabilize, color grade, retouch…) con atajo asignable y ámbito foto/vídeo/ambos |
| Persistencia | SQLite; XMP sidecars opcionales (al commit o en modo auto) |
| Modos de flujo | Configurable por tipo de acción: **auto** (se aplica al momento, p.ej. escribir estrellas a metadatos) o **diferido** (cola hasta commit) |
| Borrado | Configurable: Papelera de Windows (defecto) / permanente / carpeta `_trash` del proyecto |
| Preview RAW | JPEG embebido del RAW por defecto (estilo Photo Mechanic); decode completo del RAW configurable/bajo demanda |
| Vistas MVP | Visor + filmstrip, grid, comparar lado a lado, zoom 100% instantáneo manteniendo posición entre fotos |
| Vídeo | Básico en el MVP, en pestaña separada (MP4/MOV, reproducción HTML5, mismo flujo de clasificación y tags) |
| Commit | Diálogo de confirmación con resumen; borrar rechazadas, mover/copiar por reglas (5★ → /selects…), escribir XMP |
| Pares espejo | Acción sobre ambos por defecto + desacoplar par + comandos "solo RAW"/"solo JPEG" |
| Futuro (no MVP) | Culling IA/semi-IA (agrupar similares, elegir mejor de ráfaga); diseñar BD/arquitectura preparada. Heurísticas baratas (hash perceptual + tiempo) pueden entrar si son casi gratis |
| Teclado | Prioridad máxima: atajos totalmente remapeables con defaults sensatos (X=reject, P=pick, 1–5=estrellas…) |

## Diseño técnico

### Elecciones de tecnología

| Área | Elección | Justificación |
|---|---|---|
| RAW decode + preview embebido | **`rawler`** (~0.7.x, del proyecto dnglab), pinneado y envuelto en un trait `RawDecoder` propio | Rust puro, mantenido, decodifica CR2/CR3/NEF/ARW/RAF/ORF/DNG y expone el JPEG embebido (la vía rápida estilo Photo Mechanic). API inestable → pin exacto. LibRaw como feature opcional post-MVP (escape hatch para cámaras exóticas) |
| EXIF de JPEG/TIFF | `kamadak-exif` 0.6 | Fecha de captura, orientación, cámara |
| Miniaturas | `image` 0.25 (zune-jpeg) + **`fast_image_resize`** 5.x (SIMD + rayon) | Lo más rápido en Rust puro; libvips descartado (dependencia C dolorosa en Windows) |
| Formato de thumbs | JPEG q80, dos tamaños: `thumb` ~320px (grid) y `preview` ~2560px (visor). Si el preview embebido del RAW ya es JPEG full-size → copia de bytes sin transcodificar | Caché en disco `<proyecto>/.cullant/thumbs/`, indexada en SQLite con mtime para invalidación |
| Workers | Pool **rayon** (CPU) + cola de prioridad orquestada por tokio. P0 = viewport visible, P1 = prefetch, P2 = escaneo de fondo; cancelación por generación | El frontend reporta el rango visible al hacer scroll |
| Servir imágenes al webview | **Protocolo URI asíncrono custom `cullant://`** (nunca base64 por IPC): `thumb/{id}`, `preview/{id}`, `original/{id}`, `video/{id}` **con soporte HTTP Range** (imprescindible para seek de `<video>`), `poster/{id}`. Headers `Cache-Control` por mtime | En Windows se sirve como `http://cullant.localhost/...` — contemplar ambas formas |
| SQLite | **`rusqlite`** (feature bundled), WAL, un hilo escritor único (comandos por canal, transacciones por lotes) + pool de lectura. Migraciones con tabla `schema_version` | sqlx/async no aporta nada en BD local embebida |
| Ubicación BD | **Por proyecto: `<proyecto>/.cullant/cullant.db`** con ajuste `storage.location = project \| appdata` | El proyecto es autocontenido (se mueve/archiva con su estado). Opción appdata para carpetas en OneDrive/NAS (el WAL provoca tormentas de sync) |
| Papelera | crate `trash` 5.x (usa IFileOperation) | Modos: recycle (defecto) / permanente / `_trash` del proyecto preservando subruta relativa |
| XMP sidecars | **`quick-xml`** (leer-modificar-escribir preservando propiedades ajenas como `crs:` de Lightroom) con plantilla propia | Ver sección "Interop XMP" más abajo. Evitar el SDK C++ de Adobe (`xmp_toolkit` queda como plan B para parsing robusto) |
| Vídeo | `ffmpeg-sidecar` (descarga binario ffmpeg): poster frame + metadatos ffprobe; reproducción = `<video>` contra `cullant://video/...` | HEVC puede no reproducir en WebView2 → detectar códec y ofrecer proxy H.264 |
| Frontend | **Svelte 5 (runes) + TypeScript + Vite** | Sin VDOM (clave para grids de miles de celdas), poco JS, buen ecosistema para OSS. React descartado |
| Grid virtualizado | **Hecho a mano** (celdas de tamaño fijo, ~150 líneas): ventana de filas visibles + overscan, celdas absolutas **keyed por slot (no por foto)** para reciclar nodos DOM | Fallback: `@tanstack/virtual`. Objetivo: 50k fotos a 60fps, <600MB de renderer |
| Otros crates | `rayon`, `tokio`, `walkdir`, `notify` (post-M1), `xxhash-rust` (identidad de archivo), `image_hasher` (pHash, stretch M8), plugins Tauri: dialog, store, opener, single-instance | |

### Esquema SQLite (por proyecto, `.cullant/cullant.db`)

Principio clave: **el estado de culling y las acciones pendientes son siempre por archivo**; los grupos (pares RAW+JPEG) son identidad materializada, pero **el switch espejo es puro concepto de vista/dispatch** — cambiar de modo es instantáneo y sin migración de datos.

```sql
CREATE TABLE schema_meta (version INTEGER NOT NULL);

CREATE TABLE project (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  root_path TEXT NOT NULL, created_at INTEGER NOT NULL,
  settings TEXT NOT NULL DEFAULT '{}'   -- JSON: overrides por proyecto
);

-- Una fila por archivo físico. El estado de culling vive AQUÍ.
CREATE TABLE files (
  id INTEGER PRIMARY KEY,
  rel_path TEXT NOT NULL UNIQUE, basename TEXT NOT NULL, dir TEXT NOT NULL,
  ext TEXT NOT NULL, kind INTEGER NOT NULL,  -- 0=raw 1=image 2=video 3=sidecar
  size INTEGER NOT NULL, mtime INTEGER NOT NULL,
  quick_hash TEXT, capture_time INTEGER,
  width INTEGER, height INTEGER, camera TEXT, lens TEXT, iso INTEGER,
  orientation INTEGER, duration_ms INTEGER,
  status INTEGER NOT NULL DEFAULT 0,     -- 0=present 1=missing 2=deleted
  group_id INTEGER NOT NULL REFERENCES groups(id),
  rating INTEGER NOT NULL DEFAULT 0 CHECK (rating BETWEEN 0 AND 5),
  flag INTEGER NOT NULL DEFAULT 0,       -- -1 reject, 0 sin marcar, 1 pick
  label TEXT, state_updated_at INTEGER,
  xmp_dirty INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_files_group ON files(group_id);
CREATE INDEX idx_files_pair  ON files(dir, basename);
CREATE INDEX idx_files_cull  ON files(kind, flag, rating, label);
CREATE INDEX idx_files_time  ON files(capture_time);

-- Foto lógica. TODO archivo pertenece a un grupo (singleton si no tiene par).
CREATE TABLE groups (
  id INTEGER PRIMARY KEY,
  primary_file_id INTEGER,               -- miembro que representa al grupo
  decoupled INTEGER NOT NULL DEFAULT 0,  -- 1 = par desacoplado (sin fan-out)
  created_at INTEGER NOT NULL
);

CREATE TABLE task_tags (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE,
  shortcut TEXT, scope INTEGER NOT NULL DEFAULT 2,  -- 0=foto 1=vídeo 2=ambos
  color TEXT, sort_order INTEGER NOT NULL DEFAULT 0,
  builtin INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE file_tags (
  file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES task_tags(id) ON DELETE CASCADE,
  PRIMARY KEY (file_id, tag_id)
);

-- Cola de acciones diferidas, por ARCHIVO ("borrar par" = 2 filas con mismo pair_token)
CREATE TABLE pending_actions (
  id INTEGER PRIMARY KEY,
  file_id INTEGER NOT NULL REFERENCES files(id),
  action INTEGER NOT NULL,               -- 0=delete 1=move 2=copy 3=write_xmp
  params TEXT NOT NULL DEFAULT '{}',     -- JSON: {dest:"selects/"}...
  pair_token TEXT, origin INTEGER NOT NULL,  -- 0=manual 1=regla
  created_at INTEGER NOT NULL,
  UNIQUE (file_id, action)
);

-- Historial de commits + log de deshacer
CREATE TABLE commits (
  id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL, finished_at INTEGER,
  status INTEGER NOT NULL, summary TEXT NOT NULL  -- JSON del diálogo de confirmación
);
CREATE TABLE commit_entries (
  id INTEGER PRIMARY KEY, commit_id INTEGER NOT NULL REFERENCES commits(id),
  file_id INTEGER, action INTEGER NOT NULL,
  before_path TEXT, after_path TEXT, undo_info TEXT,
  result INTEGER NOT NULL, error TEXT
);

CREATE TABLE thumbnails (
  file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  kind INTEGER NOT NULL,                 -- 0=thumb 1=preview 2=poster
  cache_path TEXT NOT NULL, width INTEGER, height INTEGER,
  source_mtime INTEGER NOT NULL, generated_at INTEGER NOT NULL,
  PRIMARY KEY (file_id, kind)
);

CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);

-- Futuro IA (se crean ya, se pueblan después)
CREATE TABLE file_analysis (
  file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
  phash BLOB, blur_score REAL, exposure_score REAL,
  embedding BLOB, analyzed_at INTEGER
);
CREATE TABLE similarity_clusters (
  cluster_id INTEGER NOT NULL, file_id INTEGER NOT NULL REFERENCES files(id),
  score REAL, PRIMARY KEY (cluster_id, file_id)
);
```

**Nivel app (no por proyecto)**: keybindings, plantilla de task tags por defecto, modos de acción por defecto, proyectos recientes → `tauri-plugin-store` JSON en `appConfigDir` (`keybindings.json`, `settings.json`). Los task tags se *copian* de la plantilla global al crear cada proyecto (proyectos autocontenidos, defaults editables globalmente).

### Semántica espejo/separado (decisión de diseño central)

- **Espejo ON**: el grid muestra una celda por grupo; cualquier escritura de estado (rating/flag/label/tag) o encolado de acción sobre un grupo hace **fan-out a todos los miembros en una transacción** — salvo `decoupled=1`, en cuyo caso se muestra agrupado con badge "split" y hacen falta comandos por miembro ("delete RAW only"). Estado mostrado = el del archivo primario; indicador de conflicto si los miembros divergen.
- **Espejo OFF (separado)**: una celda por archivo; la pertenencia a grupo se ignora en el dispatch.
- Cambiar de modo es instantáneo y sin pérdida (solo cambia la query y el dispatch). Reacoplar ofrece "sincronizar estado desde RAW / JPEG / el más reciente".
- Regla de emparejado al escanear: mismo `dir` + mismo `basename`, uno `kind=raw` y otro `kind=image` (sets de extensiones configurables). Los vídeos nunca se emparejan. El re-escaneo solo re-evalúa grupos singleton; nunca fusiona en silencio grupos con estado divergente (pregunta).

### Arquitectura del código

```
cullant/
├── src-tauri/src/
│   ├── main.rs / lib.rs      # builder Tauri, registro de protocolo, estado
│   ├── commands/             # capa IPC fina, sin lógica (project, catalog,
│   │                         #   culling, tags, actions, commit, settings, viewer)
│   ├── db/                   # rusqlite: hilo escritor, migraciones, repos
│   ├── scan/                 # walkdir recursivo, emparejado, reconcile
│   ├── decode/               # trait RawDecoder; rawler_backend; exif; heic stub
│   ├── thumbs/               # scheduler de prioridad, workers rayon, caché disco
│   ├── protocol/             # handlers cullant:// (incl. Range para vídeo)
│   ├── engine/               # lógica de dominio: culling.rs (fan-out espejo),
│   │                         #   committer.rs (ejecución + undo), xmp.rs
│   ├── video.rs              # ffprobe, extracción de posters
│   └── error.rs
├── src/                      # Frontend Svelte 5
│   ├── lib/api.ts            # wrappers tipados de invoke() + eventos
│   ├── lib/stores/           # catalog, selection, viewMode, filters, keymap
│   ├── lib/keyboard/         # dispatcher: tecla → commandId → handler
│   └── lib/components/       # VirtualGrid, GridCell, Viewer, Filmstrip,
│                             #   CompareView, FilterBar, CommitDialog, Settings
└── docs/ README.md LICENSE
```

**Superficie de comandos IPC** (structs serde; los píxeles nunca cruzan IPC, solo por `cullant://`):
- Proyecto: `open_project`, `close_project`, `rescan_project`, `get_scan_status` (+ eventos `scan:progress/done`)
- Catálogo: `query_items(filter, sort, mode) -> Vec<ItemLite>` (~100 bytes/ítem: el índice completo se carga al frontend una vez, 50k ítems es trivial), `get_item_detail(id)`
- Culling (aceptan `targets: {ids, asGroups}` — el fan-out espejo es responsabilidad del backend): `set_rating`, `set_flag`, `set_label`, `toggle_task_tag`; devuelven filas autoritativas + evento `state:changed`
- Grupos: `decouple_group`, `recouple_group(sync_from)`
- Tags: `list/create/update/delete_task_tag`
- Acciones: `enqueue_action`, `remove_pending`, `list_pending`, `apply_rules_preview`
- Commit: `commit_preview() -> CommitPlan`, `commit_execute(plan_hash)` (+ `commit:progress`), `list_commits`, `undo_commit / undo_commit_entry` (mejor esfuerzo: revertir moves/copies; restore de papelera donde `trash` lo permita)
- Visor: `request_preview(id, priority)`, `request_full_decode(id)` (cancelable), `set_visible_range(from, to)`
- Ajustes: `get/set_project_settings`, `get/set_app_settings`, `get/set_keybindings`, `reveal_in_explorer`, `pick_folder`

**Frontend clave**:
- Stores con runes; mutaciones optimistas reconciliadas con la respuesta del comando.
- Dispatcher de teclado único a nivel `window`: normaliza (`ctrl+shift+p`) → keymap (`commandId -> binding[]`, fusionado con atajos de task tags, conflictos visibles en ajustes) → ejecuta sobre selección/foco.
- **Zoom 100% persistente**: el visor guarda `{scale, cx, cy}` en coordenadas relativas de imagen; al cambiar de foto con zoom activo se reaplica el mismo centro relativo (el bucle de comprobar enfoque en ráfagas).
- **Auto vs diferido por tipo de acción**: mapa `actionMode: {xmpWrite: 'auto'|'commit', delete: 'commit', ...}`. "Auto" en xmpWrite = el backend escribe sidecars con debounce tras marcar `xmp_dirty`; deletes/moves en auto pasan por el mismo código del committer (registrados como commit de una entrada → undo/historial uniforme).

## Hitos (MVP)

| Hito | Contenido | Definición de hecho |
|---|---|---|
| **M0 — Scaffold** (½–1 sem) | Crear repo en `C:\dev\cullant` (git init, LICENSE GPL-3.0, .gitignore), create-tauri-app (Svelte+TS), workspace Rust, rusqlite+migraciones, CI (fmt/clippy/test/build Windows), protocolo `cullant://` de prueba, selector de carpeta | La app arranca, elige carpeta, un comando IPC de ida y vuelta, CI verde |
| **M1 — Scan + thumbs + grid** (2 sem) | Escaneo recursivo (salta `.cullant`, `_trash`, ocultos; detecta placeholders OneDrive), tablas files/groups, EXIF, scheduler de miniaturas visible-first + caché, grid virtual con reciclado DOM, orden por fecha/nombre, preview embebido RAW vía rawler | Carpeta de 5k fotos mixtas (CR3/NEF/ARW/JPEG): grid a 60fps, thumbs visibles primero, segunda apertura instantánea desde caché |
| **M2 — Estado + teclado + filtros** (1–1.5 sem) | Ratings/flags/labels persistidos, badges optimistas, dispatcher de teclado + UI de remapeo, barra de filtros con contadores | Cullear 500 fotos solo con teclado; reiniciar y el estado persiste; remapear X→R funciona |
| **M3 — Visor, filmstrip, compare, zoom** (1.5 sem) | Visor sobre `cullant://preview`, filmstrip virtualizado, toggle 100% con pan persistente entre fotos, compare 2+ con pan/zoom enlazado, decode RAW completo bajo demanda | Zoom al ojo, flechear 20 fotos de ráfaga y el punto de pan se mantiene; comparar 3 imágenes con pan sincronizado |
| **M4 — Modo espejo** (1 sem) | Display agrupado, dispatch fan-out, desacoplar/reacoplar con prompt de sync, badges de par (chip RAW+JPG), comandos por miembro, toggle instantáneo | Puntuar un par → ambos archivos; desacoplar → rechazar solo el JPEG; cambiar a separado → ambos visibles con estado independiente correcto |
| **M5 — Task tags** (½ sem) | CRUD, asignación de atajo con detección de conflictos, ámbito foto/vídeo/ambos, defaults (retouch, trim, stabilize, color grade), filtro por tag | Crear tag "pano" con `Shift+P` solo-fotos; asignar por teclado; filtrar; atajo inerte en pestaña vídeo |
| **M6 — Acciones + commit** (2 sem) | Encolar delete/move/copy con semántica de par, motor de reglas ("rating ≥ 5 → mover /selects"), escritura/merge XMP (verificada en Lightroom/C1), diálogo de commit (resumen agrupado + lista expandible + avisos), ejecutor con resultados por entrada, 3 modos de borrado, historial + undo, modo auto-vs-commit por acción | Sesión completa: rechazar 50 / pick 20 / puntuar el resto → el diálogo muestra el plan exacto → ejecutar → archivos movidos/reciclados, sidecars legibles en Lightroom, undo restaura un move |
| **M7 — Pestaña vídeo** (1 sem) | Scan MP4/MOV, metadatos ffprobe, posters, `<video>` vía protocolo con Range, paridad completa de culling/tags/commit, tags de ámbito vídeo activos | Pestaña con posters; puntuar/etiquetar/encolar-borrar/commit de un vídeo de punta a punta |
| **M8 — Pulido + empaquetado** (1–1.5 sem) | Paneles de ajustes (modo borrado, modos de acción, preferencia decode RAW, ubicación BD), reconciliación de archivos perdidos al re-escanear, toasts de error, instalador NSIS/MSI vía `tauri bundle`, README/capturas, CONTRIBUTING.md. *Stretch (solo si sale gratis)*: pHash en el scan + agrupación de ráfagas por ventana temporal | El instalador funciona en un Windows 11 limpio; sesión real de culling de 20 min sin crashes |

**Post-MVP** (diseñado pero no construido): culling IA (clusters de similitud, mejor-de-ráfaga con blur/exposición — columnas `embedding`/`file_analysis` reservadas), XMP embebido en JPEG/DNG, HEIC, feature LibRaw, segunda ventana/monitor dual, renombrado por plantilla en commit.

## Riesgos y mitigaciones

1. **OneDrive** — (a) *repo de desarrollo*: resuelto — el repo vive en `C:\dev\cullant`, fuera de OneDrive; (b) *proyectos de fotos del usuario en OneDrive*: los placeholders Files-On-Demand pueden disparar descargas al leer → el scanner detecta el atributo y marca `cloud-only` en vez de atascar el pool; el WAL de SQLite provoca churn de sync → ajuste de BD en appdata y considerar `journal_mode=TRUNCATE` si se detecta ruta OneDrive.
2. **Huecos de CR3 en rawler** (API no SemVer, quirks por modelo) → pin exacto, trait `RawDecoder`, corpus de RAWs reales en M1, feature LibRaw como escape.
3. **Memoria de WebView2 con grids enormes** → reciclado DOM estricto (~100 celdas montadas), thumbs JPEG pequeños, `Cache-Control` del protocolo en vez de blob URLs, limpiar `src` al reciclar. Objetivo <600MB con 50k fotos.
4. **HEIC en Windows**: WebView2 no lo renderiza ni con el códec del SO → excluir del MVP (listar como "no soportado"); post-MVP transcodificar vía WIC (crate `windows`).
5. **MOV HEVC puede no reproducir en WebView2** → detectar códec con ffprobe; mostrar poster + botón "generar proxy H.264" (ffmpeg-sidecar) en vez de fallar en silencio.
6. **Quirks de interop XMP** (Lightroom es quisquilloso con la estructura) → tests golden-file contra sidecars exportados de Lightroom/C1; nunca destruir propiedades desconocidas al fusionar.
7. **Rutas Windows**: rutas >260 chars y nombres no-ASCII → APIs conscientes de `\\?\`, tests con nombres Unicode.
8. **Restore de papelera no fiable programáticamente** → el historial presenta los borrados a papelera como "restaurar manualmente desde la Papelera" en vez de prometer undo automático.

## Verificación

- Cada hito tiene su definición de hecho (tabla de hitos) — se verifica ejecutando la app real con una carpeta de fotos de prueba con mezcla RAW+JPEG.
- Tests unitarios Rust: emparejado espejo, fan-out/decouple, motor de reglas, merge XMP (golden files de Lightroom), invalidación de caché de thumbs.
- Test de interop manual en M6: importar sidecars generados en Lightroom Classic y Capture One reales.
- Test de rendimiento en M1: carpeta sintética de 5k+ imágenes, scroll fluido y prioridad visible-first.

## Keymap por defecto (investigado sobre PM/FRV/Narrative/Aftershoot/LR)

Principio: **las teclas de Lightroom como capa base** (mayor memoria muscular; Narrative y Aftershoot ya convergieron en ellas), con ideas de Photo Mechanic/FastRawViewer encima. Post-MVP: presets alternativos "Photo Mechanic" y "FastRawViewer" (precedente en Aftershoot y FRV). Todo remapeable.

**Navegación y vistas**: `←/→` (y `↑/↓` en grid) siguiente/anterior · `Home/End` · `G` grid / `E` visor / `C` compare / `N` survey · `F` fullscreen · `I` ciclo de overlay de info · `\` barra de filtros · `Ctrl+L` activar/desactivar filtros.

**Clasificación** (todas respetan auto-advance): `P` pick / `X` reject / `U` unflag / `` ` `` toggle · `1-5` estrellas, `0` limpiar · `6 7 8 9 -` colores (rojo/amarillo/verde/azul/morado), `Ctrl+0` limpiar color · **Task tags como acorde**: `T` y luego `1-9` aplica el tag N; `Shift+T` limpia (además del atajo directo configurable por tag).

**Auto-advance** (semántica exacta de Lightroom): **Caps Lock ON = avanzar tras clasificar**; `Shift+tecla` = avance puntual (Caps off) o suprimir avance para apilar flag+estrella+color (Caps on). Además, checkboxes por tipo de acción en ajustes (estilo Photo Mechanic).

**Zoom e inspección**: `Z` o `Space` toggle ajuste↔100% (mantener `Space` = zoom temporal, estilo PM) · `Ctrl +/-` · `=` sincronizar pan/zoom en compare (FRV).

**Pares RAW+JPEG (diferenciador de Cullant)**: `J` alternar mitad mostrada (RAW↔JPEG) · `Ctrl+J` desacoplar/reacoplar · `Delete` marcar par para borrar (diferido, estilo FRV — nunca inmediato por defecto) · `Alt+Delete` borrar solo RAW · `Shift+Delete` borrar solo JPEG.

**Sesión**: `Ctrl+Enter` diálogo de commit · `Ctrl+Z`/`Ctrl+Y` undo/redo · `[`/`]` rotar · `M` toggle modo espejo.

Teclas reservadas libres para futuro: `S`, `V`, `O`, `Q` (PM usa `V` para 2-up, Aftershoot `S` para swap de duplicado).

### Features de flujo adoptadas de la investigación (más allá de lo básico)

- **Carpeta `_Rejected`/`_trash` sin diálogos de confirmación** en el flujo rápido (FRV): siempre reversible; el borrado real es un segundo paso explícito (nuestro commit).
- **Filtro de atributos siempre visible** con contadores clicables (Narrative + LR).
- **Pan/zoom sincronizado en compare** — esencial para A/B de nitidez entre casi-duplicados.
- Post-MVP: **estadísticas de exposición RAW + focus peaking** (FRV, único que lo tiene; encaja con nuestra capa de decode en Rust), **zoom a caras detectadas** con `Space` + flechas (Narrative; viable con un modelo ONNX pequeño = puerta de entrada barata al culling semi-IA), **navegación dentro de set de duplicados** `,`/`.` (Aftershoot).

### Referencias open source a estudiar

- **RapidRAW** (github.com/CyberTimon/RapidRAW) — Rust+Tauri, editor RAW con culling; el primo arquitectónico más cercano (estudiar su pipeline de decode/thumbnails).
- **QuickRawPicker** (github.com/RawLabo/QuickRawPicker) — referencia de interop de sidecars XMP/PP3.
- **PhotoSort** (github.com/duartebarbosadev/PhotoSort) — detección de blur y clustering de similitud (referencia para el futuro IA).
- Conclusión clave: **no existe ningún culler open source keyboard-first pulido — el nicho está genuinamente vacío.**

## Interop XMP (detalles verificados)

**SQLite es la fuente de verdad; XMP es la capa de exportación.** Reglas concretas:

- **Nombre del sidecar: `IMG_001.xmp`** (basename sin extensión original). Es lo que leen Lightroom y Capture One; el estilo darktable `IMG_001.CR3.xmp` NO lo leen.
- **Sidecars solo para RAW propietarios.** Lightroom embebe el XMP dentro de JPEG/TIFF/DNG y no lee sidecars para ellos. Consecuencia para pares RAW+JPEG: `IMG_001.xmp` pertenece al RAW — **nunca escribir un sidecar "para el JPEG"**. Exportar metadatos a JPEGs = embeber XMP en el archivo (opt-in, post-MVP, más arriesgado).
- **Campos a escribir**:
  - `xmp:Rating` = 0–5 (el -1 de "rechazada" es válido en la spec y lo entienden Bridge/FastRawViewer, pero Lightroom NO lo mapea a su bandera reject → no usarlo para LR; ofrecerlo como convención opcional).
  - `xmp:Label` = strings en inglés del set de LR: `Red`, `Yellow`, `Green`, `Blue`, `Purple` (baseline de interop; los LR localizados usan strings traducidos — documentarlo). C1 moderno los lee; C1 tiene 7 colores y no existe pick/reject en C1.
  - **Banderas: desde LR Classic 13.2 (feb 2024) sí van en XMP**: `xmpDM:pick` (1/0/-1) + `xmpDM:good` (True/ausente/False). Escribirlas además de rating/label; para LR antiguos, ofrecer mapeo opcional (reject → label rojo o rating).
  - `dc:subject` para task tags como keywords + namespace `cullant:` para round-trip sin pérdida.
- **Regla crítica**: si ya existe un sidecar (p.ej. LR guardó ajustes de revelado en `crs:`), **leer-modificar-escribir tocando solo nuestras propiedades** — jamás sobreescribir con la plantilla.
- Plantilla mínima aceptada por LR (booleanos XMP capitalizados, `<?xpacket?>` opcional en sidecars):

```xml
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Cullant 0.1.0">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/"
    xmp:Rating="3" xmp:Label="Red"
    xmpDM:pick="1" xmpDM:good="True"/>
 </rdf:RDF>
</x:xmpmeta>
```

- Documentar en la ayuda: C1 requiere activar "Prefer Sidecar XMP over Embedded Metadata" / "Auto Sync Sidecar XMP: Full Sync".

## Licencia

**Recomendación: GPL-3.0-or-later.** Motivo: `rawler` (el mejor decodificador RAW en Rust puro: CR3, X-Trans, 300+ cámaras, sin toolchain C) es LGPL-2.1 y en Rust se enlaza estáticamente; LGPL-2.1 es elevable a GPL y el resto de dependencias (Tauri, rusqlite, image, trash — todas MIT/Apache) son GPL-compatibles → cero notas de cumplimiento. Es además la norma del sector (darktable, RawTherapee, digiKam son GPL) y Cullant es una app final, no una librería embebible.

Alternativa si se quisiera MIT/Apache-2.0: renunciar a rawler y usar LibRaw bajo CDDL o enlazado dinámico (`libraw.dll`) — dependencia C, FFI y más fricción. No recomendado.

Nota: `ffmpeg-sidecar` descarga un binario ffmpeg (GPL) — coherente con app GPL; mencionarlo en el README.

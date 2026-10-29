# egui_bench

A bench-instrument look for [egui](https://github.com/emilk/egui): chassis greys,
engraved legends, amber readouts, cyan traces. Pulled out of two programs that had
grown the same panel twice, [waveshark](https://github.com/v0l/super-radio) and
cycler, and merged into one crate.

![panel](https://raw.githubusercontent.com/v0l/egui_bench/master/docs/panel.png)

## The grammar

Two accents, and they mean something rather than decorate.

- **Amber (`READOUT`) is what you set.** Setpoints, limits, thresholds, the tuned
  frequency, the handle on a fader, the rule across a plot, the tab you are on.
- **Cyan (`TRACE`) is what the instrument measured.** Levels, cell voltages,
  received signal, anything off the air or off the shunt.
- `OK` green and `FAULT` red are states, not readings. `LEGEND` grey labels,
  `VALUE` off-white reads back.

Surfaces step in one direction and only one: `WELL` is recessed, `CHASSIS` is the
case, `PANEL` is proud of it, `ETCH` is the engraved edge. Corners are 2 px,
because an instrument panel is milled, not moulded. Unit tests in `src/theme.rs`
hold the contrast ratios and the surface ordering, so a palette edit that breaks
legibility fails `cargo test`.

Fonts are IBM Plex, embedded, so every machine renders the same: condensed for
panel legends (uppercase, tracked out, silkscreen), mono semibold for figures
(tabular, so a changing digit never shifts the ones beside it).

## Use

```toml
[dependencies]
egui_bench = "0.5"
```

```rust,ignore
// once, at startup
egui_bench::install(&cc.egui_ctx);

use egui_bench::prelude::*;

card(ui, Some(READOUT), |ui| { Line::new().legend("tuned").show(ui); }, |ui| {
    hero(ui, "centre", "145.5250", "MHz", READOUT);
    readouts(ui, &[
        ("mode", "NFM".into(), READOUT),
        ("heard", "-93.4 dBm".into(), TRACE),
    ]);
});
```

`install` sets fonts, visuals, spacing and text styles. Everything else is
opt-in: the components are plain functions and `egui::Widget` impls, so they
compose with `add_sized`, `add_enabled` and the rest of egui's layout.

## What is in it

### `theme` - palette, fonts, style

`install(ctx)`, `fonts(ctx)`, the colour constants, and `legend_font(size)` /
`figure(size)` / `mono(size)` for painting your own galleys in the panel's faces.

### `text` - words on a panel

`legend()`, `value()`, `action()` return themed `RichText`. `note()` and `hint()`
are wrapped prose. `help(ui, text)` is a "?" that carries the explanation on
hover, which keeps a settings dialog a column of settings.

`Line` is the important one. A legend beside a value is two fonts at two sizes,
and two labels in a `ui.horizontal` are two galleys that egui centres against
each other, so the row sits a pixel out everywhere it appears. `Line` lays every
span into one `LayoutJob` and paints it on a fixed baseline:

```rust,ignore
Line::new().legend("squelch").column(ui, 98.0).set("-78 dB").show(ui);
Line::new().legend("heard").measured("-93.4 dBm").show(ui);
```

`.set()` is amber, `.measured()` is cyan, `.value()` is neutral, `.note()` is
prose. `.column(ui, x)` starts the next span at a fixed offset so a stack of
readings aligns. `.show()`, `.elided()` and `.wrapped()` end it, and
`.hanging(ui, x, value)` wraps a long value under its own column.

### `panel` - blocks and state

- `card(ui, rail, header, body)`: recessed header, body under it, optional
  coloured rail down the left carrying the card's state.
- `section(ui, label, note, body)`: a card whose header is a legend and a
  right-aligned sentence saying what it is for.
- `lamp(ui, text, on, fault)`: a key-shaped state lamp, lit or dark.
- `toggle(ui, text, on)`: the same shape, clickable, amber when set.
- `status(ui, ok, text)`: a dot and a sentence, for validation in a dialog.
- `stage_rail(ui, stages, at, now, height, muted)`: a real sequence drawn as one,
  stages behind you dim, the running one lit, the ones ahead outlined.
- `chip(ui, text, tint, lit)`: a small state tag in any colour, for a card corner.
- `heading(ui, label)`: an engraved legend with a rule running to the edge, for
  a group of cards.
- `segments(ui, &mut current, &[(value, label, count)])`: a joined filter bar,
  the chosen segment amber and underlined, each with an optional count.
- `exit_note`, `tabs`.

### `key` - push buttons

`Key::new("launch").solid(VALUE)` is a 30 px button with an engraved label,
`.outline(tint)` the quieter one beside it, `Key::more()` the three-dot menu key.
They take keyboard focus and draw an amber ring when they have it. `ui.add(key)`
lays one out; `key.paint(ui, rect)` puts it in a rect you have measured, with
`.id_salt(..)` keeping its focus when the layout shifts. `busy(ui, rect, done,
label, tint)` is the same shape filling with progress, or sweeping when the end
is unknown, so a key can turn into its own progress bar.

### `markdown` - agent replies as a panel (feature `markdown`)

`markdown::show(ui, &mut cache, text, salt)` draws CommonMark in the panel's faces:
headings, bold, italic, strikethrough, inline code, fenced blocks with a copy key,
tables, nested and task lists, quotes, rules and links. Links open only if they are
http, https or mailto. The `Cache` parses a reply once and forgets it when it stops
being drawn, and an unfinished fence while a reply streams is already a code block.

### `agent` - a coding agent in a window (feature `agent`)

A chat with any OpenAI-compatible server that has tool calls, filling the central
panel, with a workspace folder the model reads and writes in. It brings the loop,
the pane, the settings page and these tools:

| tools | what they do |
|---|---|
| `list_files`, `read_file`, `write_file`, `edit_file`, `search_files`, `workspace` | files in the workspace |
| `request_access` | asks the user to share a folder outside it |
| `bash`, `bash_output`, `bash_kill` | a shell command in the sandbox; with `background` it answers at once with a handle to read from and stop |
| `code_outline`, `code_find`, `code_diagnostics` | code by its units, through a language server: a file's outline, a unit's source and everything that uses it, the errors in a file |
| `web_search`, `web_fetch` | DuckDuckGo, Bing and Brave in turn; a page as markdown, read in pieces, saved to disk, or answered by a small model |
| `load_skill`, `find_skills` | reads `SKILL.md` files from `~/.agents/skills` and the workspace; searches the skills.sh directory for one to install |
| `todo_write` | a checklist for long work, drawn in the chat |

File tools cannot leave the workspace unless the user shares a folder: `request_access` puts the
path and the model's reason in a card, and the user picks read only, read and write, or deny. A
share covers that folder and what is under it, never its parent or neighbours, follows no link out
of itself, and ends when the chat is cleared or the user clicks it away. Credential folders
(`~/.ssh`, `~/.aws`, browser profiles, the app's own config), the whole home folder and a drive
root are refused without asking. `Agent::needs_you()` says when a card is waiting, so the app can
flag its tab.

`bash` and the language servers run in an OS sandbox through `nono` (Landlock on Linux, Seatbelt
on macOS): the system and the tools are readable, only the workspace and a scratch folder are
writable, the rest of the user's files are hidden, the environment is scrubbed and the network is
open. A language server also reads the toolchain it needs (`~/.cargo`, `~/.rustup`, the Go module
cache, nvm's nodes), never writes there. Tools that need the sandbox are left out of the tool list
where there is none, which includes Windows. Background commands belong to the chat that started
them and stop when it is cleared.

Everything runs on tokio. With no runtime around it the crate starts a small one of its own
(`agent::runtime()`); inside a tokio app it uses the app's. An app supplies two things:

```rust,ignore
struct MyApp { agent: egui_bench::agent::Agent<MyApp>, /* ... */ }

#[egui_bench::agent::async_trait]
impl egui_bench::agent::Backend for Mine {
    fn app(&self) -> &'static str { "my-app" }
    fn brief(&self, workspace: &Path, skills: &[Skill]) -> String { /* system prompt */ }
    fn welcome(&self) -> Welcome { /* empty-state text */ }
    fn toolkit(&self) -> &Toolkit { /* the app's own tools */ }
    fn leave_out(&self) -> Vec<&'static str> { vec!["web_search"] /* built-ins to drop */ }
}

impl egui_bench::agent::Host for MyApp {
    fn agent(&mut self) -> &mut Agent<Self> { &mut self.agent }
    fn call(&mut self, ctx: &egui::Context, name: &str, args: &Value) -> Handled<Self> {
        // runs on the UI thread, at the top of a frame: Handled::Now(reply), or
        // Handled::Later(poll) for something that finishes on a later frame
    }
}
```

A tool in the toolkit is async: `Tool::new(name, about, schema, |args, cx| async move { .. })`,
`Tool::blocking` for code that blocks (it runs on tokio's blocking pool), `Tool::host` to hand
the call to the UI thread, where `Host::call` answers it. `cx.ask(name, args).await` reaches the
host from any tool, and `cx.stop` is cancelled when the user stops the chat. `Chat::with_services`
swaps in a `Services` with an app's own language server settings or a `web::Reader` that answers
`web_fetch`'s `prompt`.

Each frame: `agent.poll()`, `agent.show(ui)` when its page is open, and
`egui_bench::agent::serve(self, ctx)`. The binary must call
`egui_bench::agent::sandbox::run_helper()` first thing in `main` and exit with its
code when it returns `Some`, because the sandbox re-runs the app's own executable.
Settings live in `<config dir>/<app>/agent`, readable by the user only.

The tools are usable without the window too, by an agent of your own: `agent::local::run(backend,
&cx, name, &args).await` runs one, `agent::tools::offered(backend)` lists the specs to send, and
`agent::code::Code`, `agent::jobs::Jobs` and `agent::web` work alone. `spawn::Confine::Open` runs
commands and language servers without the sandbox, for a host that trusts its agent.

#### MCP servers (feature `mcp`)

`agent::mcp::Mcp` reaches the MCP servers the user set up, over stdio or streamable HTTP, behind
one `mcp` tool so a dozen servers do not put hundreds of tools in every request. With no arguments
the tool lists the servers; with `server` it starts that one and lists its tools and their
arguments; with `server`, `tool` and `args` it calls one. A server starts the first time it is
used, in the workspace, and stops after 30 idle minutes.

Servers are read from `~/.agents/mcp.json`, then from each name in `Settings::project_files`
inside the workspace (a later file replaces a server of the same name), in the format most
clients share:

```json
{
  "mcpServers": {
    "kicad": { "command": "node", "args": ["~/mcp/kicad/index.js"], "env": { "KEY": "${KICAD_KEY}" } },
    "docs": { "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer ${DOCS_TOKEN}" } }
  }
}
```

`${VAR}` and `${VAR:-default}` are filled in from the environment, `bearerToken` or
`bearerTokenEnv` becomes an `Authorization` header, `disabled` switches a server off and `timeout`
sets a call's limit in seconds (300 by default). The old SSE transport and OAuth sign-in are not
supported. MCP servers run as the user, outside the sandbox, so only configure ones you trust.

A bench app adds `agent::mcp::tool(Arc::new(Mcp::new(settings)))` to its toolkit. A host with its
own loop calls `Mcp::run`, which gives back the text and any pictures (`Image::part()` is the
`image_url` content part to send in the next user message), and `Mcp::section` for a line for
the system prompt naming the servers.

#### Voice for the agent window (`voice`, and feature `speech`)

Speech both ways, for a chat you talk to and that talks back. The helpers need no feature at all, and with `agent` you also get a client, with nothing heavy: `voice::Server { url, key }` (with `agent`) posts a take to any server with
OpenAI's `/audio/transcriptions` (`transcribe`, 16 kHz mono samples in, words out) and a sentence
to `/audio/speech` (`speak`, samples and their rate out). Both block, so call them off the UI
thread. Around them, `voice::speakable` turns a markdown reply into words (links read as their
text, code and URLs left out), `voice::chunks` cuts text at sentences so the first can play while
the rest are made, and `wav_bytes`, `read_wav`, `resample` and `to_whisper_rate` move samples
about. `voice::VOICES` and `voice::LOCAL_MODELS` list the Kokoro voices and the Whisper models.

Feature `speech` adds the models themselves, run in the app through candle: `speech::Transcriber`
(Whisper, the models in `LOCAL_MODELS`) and `speech::Speaker` (Kokoro, about 330 MB, the voices in
`VOICES`). Each fetches its files from Hugging Face the first time (`download`, which blocks) into
a folder you give it, then stays loaded. An app serves them to other machines by answering the two
endpoints above, which is how a phone uses the voices on a desktop. Add `speech-cuda` to run on an
NVIDIA card when there is one; it still starts on a machine with none (this wants the candle
patch in waveshark's workspace, which loads the CUDA runtime lazily). `cargo run --release
--features speech --example say -- "hello" <model folder>` says something to `/tmp/said.wav`.
The Kokoro engine is ported from waveshark's `tts` crate.

`speech::Voicer` is the better voice to read with: Phonon (`gradium/phonon-31m-en`, Apache 2.0),
ported to candle from gradium's `ptts` crate (MIT or Apache 2.0) and checked against it step by
step. It spells out numbers, versions, units, dates and URLs as a person reads them, comes with
25 English voices (`voices()`, `pick`), and streams: `say` hands over each frame's samples as
they are made, about 80 ms of speech each. The model is 100 MB and a voice 250 KB, fetched with
`download` and `fetch_voice`. On a GPU it makes 21 seconds of speech in 0.4; on a laptop CPU,
in about 4. `cargo run --release --features speech --example voicer -- <empty folder> "hello"`
fetches it and says something.

### `drop` - a file over the window

`drop::hovering(ctx)` says whether files are being dragged over the window and
which, and `drop::overlay(ctx, image, headline, note, tint)` dims the window
and says what letting go will do.

### `readout` - figures

- `hero(ui, label, v, unit, tint)`: the one number you read from across the room,
  unit small beside it, legend under it.
- `readout` / `readouts`: captioned figures on a fixed pitch, so three in one
  card line up with three in the card beside it, wrapping when the pane narrows.
- `reading(ui, label, text)`: a label-column row built on `Line`, the value
  wrapping under its column when it is too long for the pane.

### `meter` - levels

- `vu(painter, rect, peak)`: a level bar on a square-root scale, green to amber
  to red, painted into a rect you own.
- `Fader::new(&mut value, peak)`: a volume control whose track is its own meter,
  so what you set and what it is producing read in one glance.
- `Threshold::new(&mut value, lo, hi, measured, open)`: a squelch-style threshold
  drawn over what it is deciding against.
- `bar`, `progress` (determinate or a sweeping stripe when the total is unknown).

### `trace` - one line in a well

`Trace::new(&history).size(v).ratio(1.0, 3.0)` plots against a dashed rule on a
log axis, for margins where 2x above has to look like half below. `.range(lo, hi)`
is the linear version, `.filled(true)` shades under it.

### `comb` - a channel array

`Comb::new(&channels)` draws every channel as a level in the window they are
actually working in, with `.rule(name, at)` limits ruled across in amber. The
highest and lowest are the only two labelled, because they are the only two any
decision is made on. Generalised from cycler's per-cell voltage comb: anything
with N similar channels and a limit works.

### `table` - painted rows

A thousand rows of widgets is a thousand allocations a frame. `Table` paints a
header and striped rows and hands you a painter, a rect and the column offsets;
`cell()` clips one field to its column.

### `form` - dialogs

`row`, `row_help`, `switch`, `choice`, `field`, `secret`, `prose`, `footer`,
`modal_title`. The text fields are set into the panel as wells with an etched
edge and tabular figures, rather than egui's default grey box, which on this
chassis is the one thing that looks pasted on.

### `browser` - an open dialog

`Browser::new(title, dir)` is a modal that walks folders and lists the files
whose names end in one of `.suffixes(...)`. Call `show(ctx)` each frame while it
is up; it returns `Picked::File` or `Picked::Folder` (with `.folders(true)`) once
a choice is made, and `Picked::Cancelled` on cancel or escape.

### `viewer3d` - a 3D viewport (feature `viewer3d`)

```toml
egui_bench = { version = "0.4", features = ["viewer3d"] }
```

The shaded model view out of gcad and agentee: key and fill light, edges drawn at
a fixed pixel width and lifted toward the eye so they never poke through walls,
section cuts with capped solids, see-through and selected parts, and a software
renderer for headless screenshots. Drawing goes through wgpu on egui's own
device, so the app runs `eframe` with `wgpu` and hands the viewer its render
state once at start:

```rust,ignore
if let Some(state) = &cc.wgpu_render_state {
    egui_bench::viewer3d::install(state);
}
```

Each viewport renders into its own 4x multisampled texture, so the app needs no
depth buffer or multisampling of its own. Without `install` the viewer falls
back to the software renderer.

A `Scene` is triangles (`Surface`) and polylines (`Lines`) in groups. Each frame
a `Look` per group places it (a column-major 4x4), fades it, or selects it.

Materials are the app's own type. Anything `Copy + Eq + Hash + Default` becomes
one by saying how it shades, and surfaces that share a group, material and
texture go to the GPU as one draw:

```rust,ignore
use egui_bench::viewer3d::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Finish { #[default] Mask, Copper, Silk }

impl Material for Finish {
    fn shading(&self) -> Shading {
        match self {
            Finish::Copper => Shading { gloss: 0.46, sharpness: 82.0, metal: 0.8, ..Default::default() },
            _ => Shading::default(),
        }
    }
}

let mut viewer = Viewer::new(scene.clone(), self.camera).looks(looks).cut(cut);
viewer.navigate(ui, &response, &mut self.orbit);
self.camera = viewer.camera();
viewer.paint(ui, rect);
let hit = viewer.pick(rect, pointer);
```

`navigate` orbits about the point under the pointer, pans with shift or the
secondary button, and zooms toward the pointer. `Camera::framed` fits a region,
`Projector` maps between world and screen for overlays, and
`Viewer::render_soft(w, h)` gives a `ColorImage` without a GPU.

#### Lighting

`Style::lighting` holds a key and a fill light, each with an azimuth,
elevation, intensity, colour and softness, plus an ambient sky and ground
colour and an exposure. With `follow` set the lights turn with the camera;
without it they stay put in the world, Z up. `Lighting::STUDIO` is the default
look, and `viewer3d::lighting_controls(ui, &mut lighting)` draws a panel of
sliders for all of it.

`Preset` names three setups: `Studio`, `Overcast` (soft light from a bright
cloudy sky) and `Outdoor { hour }`, a sun that rises in the east (+X), peaks
in the south (-Y) and sets in the west, warm and low at the ends of the day,
with moonlight and a dark blue sky at night. `Preset::lighting()` gives the
`Lighting`, and `viewer3d::preset_controls(ui, &mut preset)` is a picker with
a time of day slider.

```rust,ignore
let viewer = Viewer::new(scene, camera).lighting(self.lighting);
```

#### Ray tracing

`Viewer::trace(Some(Trace { samples, bounces, per_frame }))` path traces the
view once the camera stops: soft shadows from both lights, ambient occlusion,
reflections off metal and light bounced between parts. While the view moves it
draws as usual; once still, it adds `per_frame` samples a frame until it has
`samples`, and swaps to the traced picture after the first 16. Edges and
polylines are still drawn on top, cuts are capped, and see-through parts let
light through. `paint` returns a `Painted` saying how far it has got.
`Viewer::edges(false)` leaves the `Lines` out, in this and every other mode.

On GPUs with ray tracing hardware it uses wgpu's ray queries, which egui's
device does not ask for by default. Request them in the app's options:

```rust,ignore
let options = eframe::NativeOptions {
    wgpu_options: egui_wgpu::WgpuConfiguration {
        wgpu_setup: egui_bench::viewer3d::wgpu_setup(),
        ..Default::default()
    },
    ..Default::default()
};
```

Without them it traces a BVH in a compute shader, which gives the same picture
much more slowly, so keep `per_frame` low there. `viewer3d::hardware_tracing()`
says which one you got, and `Viewer::render_gpu(w, h)` renders a still at any
size on the GPU, with every sample taken when tracing.

#### Materials

`Shading` is the quick way to say how a material looks. For more, implement
`Material::pbr` and return a `Pbr`: `roughness`, `metallic`, `specular`
(0.5 is the usual 4% reflection of plastic and paint), `emission` (linear
radiance, so a lamp glows and lights what is near it), `transmission` and
`ior` for glass that refracts, and `clearcoat` with its own
`clearcoat_roughness` for lacquer over a base colour. Without an override,
`pbr()` is worked out from `shading()`.

```rust,ignore
impl Material for Finish {
    fn pbr(&self) -> Pbr {
        match self {
            Finish::Glass => Pbr { transmission: 1.0, roughness: 0.02, ..Pbr::default() },
            Finish::Chrome => Pbr { metallic: 1.0, roughness: 0.08, ..Pbr::default() },
            Finish::Paint => Pbr { clearcoat: 1.0, roughness: 0.6, ..Pbr::default() },
            _ => self.shading().pbr(),
        }
    }
}
```

A `Surface` can also carry a `normal_map` and a `metal_rough_map` (glTF
layout: roughness in green, metal in blue, both multiplying the `Pbr`
values), as indexes into `Scene::images`. Tangents are worked out from the
UVs. `Image::normal_from_height(strength)` turns a grey height map into a
normal map, which is how to do bump mapping. Textures repeat outside 0..1.

The tracer does all of it properly. The raster view and the software
renderer do normal and metal/rough maps, emission and the clear coat
highlight, and draw glass see-through; reflections, refraction and light
from glowing surfaces only show once traced.

#### Environment

`Viewer::environment(Some(EnvironmentLight::new(map)))` lights the scene from
an image all around it instead of the sky and ground colours, with `turn`
about Z, `strength`, and `backdrop` to show it behind the model.
`Environment::from_hdr(bytes)` reads a Radiance `.hdr` panorama (Z up, the
top row overhead), `Environment::new(w, h, rgb)` takes linear pixels, and
`Environment::studio()` is a dark room with three softboxes, good for metal
and glass. The tracer samples bright parts of the map directly, so a small
sun in an HDR does not turn into noise. The raster view lights from the
map's average above and below and draws the backdrop.

#### Debug views

`Viewer::mode(RenderMode::...)` swaps the shading for a view that shows how the
model is built: `Wireframe` (triangle edges only, see-through), `Triangles`
(edges over the shaded model), `Normals` (vertex normals as colour), `Facets`
(flat shaded from the triangles themselves, so bad vertex normals stand out),
`Depth`, `Uv` (a checker over the texture coordinates), `Groups` and `Batches`
(a colour per group or per GPU draw), `Backfaces` (front blue, back red),
`Overdraw` (brighter where more layers cover a pixel) and `Traversal`, a heat
map of ray tracing cost: BVH steps on the compute tracer, candidate hits on ray
query. All but `Traversal` work in the software renderer too.
`viewer3d::mode_controls(ui, &mut mode)` is a picker for them.

### `update` - GitHub release updates (feature `update`)

An `Updater` per repo asks `/releases/latest` on a thread, compares the tag with
the version you are running, and fetches an asset with a progress bar, checking
the size and the sha256 GitHub publishes for it. What happens to the download is
yours: the closure you pass gets the file. `Updater::cached` keeps the last
answer on disk and asks again with its ETag, so a check that finds nothing new
does not count against GitHub's 60 an hour, and a check made after the limit
is spent still has an answer.

```rust,ignore
let up = Updater::new("v0l/gcad", Some(env!("CARGO_PKG_VERSION").into()));
up.check(ctx);
if let State::Newer(r) = up.state()
    && let Some(asset) = r.asset("gcad-x86_64-unknown-linux-gnu.tar.gz")
{
    up.install(ctx, &r, asset, |file| {
        let dir = file.with_extension("unpacked");
        update::unpack(file, &dir)?;
        update::replace_self(&update::find_file(&dir, "gcad").ok_or("no gcad")?)
    });
}
up.status(ui);
```

`unpack` reads `.tar.gz`, `.zip`, `.dmg` (macOS) or takes a bare binary.
`replace` swaps a file or an `.app` folder by renaming, so a running program can
be replaced, Windows included; `sweep` clears the `.old` copies Windows would not
let go of. `replace_self` and `relaunch` are the self-update pair, and
`probe_version` reads `--version` from a binary you did not build.

## Gallery

```sh
cargo run --example gallery          # panel
cargo run --example gallery meters
cargo run --example gallery data
cargo run --example viewer3d --features viewer3d
```

![meters](https://raw.githubusercontent.com/v0l/egui_bench/master/docs/meters.png)
![data](https://raw.githubusercontent.com/v0l/egui_bench/master/docs/data.png)
![viewer3d](https://raw.githubusercontent.com/v0l/egui_bench/master/docs/viewer3d.png)

## Licence

MIT. The embedded IBM Plex faces are under the SIL Open Font License, see
`assets/OFL.txt`.

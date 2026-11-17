# RustEditor (RustEditor Engine)

The video engine behind RustEditor. Rust, no GC, no hidden control flow.

## Layout

| Crate | What it is | Dependencies |
|---|---|---|
| `rusteditor-core` | Time, arena, frames, timeline model. The vocabulary every other crate speaks. | **none** (std only) |
| `rusteditor-media` | Getting pixels and samples in and out of files. Links FFmpeg (libav*), and is the only crate that knows it exists. | `rusteditor-core`, ffmpeg-the-third |
| `rusteditor-project` | The edit itself: document model, operations as commands, undo, rusteditor.json IO. | serde, serde_json |
| `rusteditor-render` | Turning a timeline plus a timestamp into one finished frame. | `rusteditor-core`, wgpu (optional) |
| `rusteditor-effects` | Effect packages: manifests, WGSL shaders for pictures and FFmpeg chain templates for sound, the catalogue. Each built-in effect is a folder under `packages/`. | `rusteditor-project`, serde, toml |
| `rusteditor-export` | Timeline to file: flatten, the frame-by-frame render loop, the paused monitor's true frame. | `rusteditor-core`, `rusteditor-media`, `rusteditor-render`, `rusteditor-project`, `rusteditor-effects` |
| `rusteditor-api` | The RustEditor API: JSON requests in, responses and events out. Projects, edits, media, the catalogue, templates, exports as jobs, frames. | `rusteditor-host`, `rusteditor-export`, `rusteditor-effects`, `rusteditor-project` |
| `rusteditor-server` | The API on a socket: JSON-RPC lines over TCP or a Unix socket, and gRPC behind the `grpc` feature. One dispatcher, many callers. | `rusteditor-api`; tonic and tokio with `grpc` |
| `rusteditor-cli` | A binary to drive the above: `probe`, `render`, `api` (the API over stdin and stdout) and `serve` (the API on a socket). | `rusteditor-server`, `rusteditor-api`, the engine crates |
| `rusteditor-host` | What the window needs that is not the edit: sessions, project folders, previews, playback, templates, job slots. | `rusteditor-media`, `rusteditor-project`, `rusteditor-export`, cpal |
| `rusteditor-speech` | Transcription (whisper.cpp, in-process) and text to speech (Kokoro via sherpa-onnx). | `rusteditor-host`, `rusteditor-media`, whisper-rs, sherpa-onnx |
| `rusteditor` | The editor window: every pane, dialog and primitive, in Slint. The app a user launches. | slint, `rusteditor-host`, `rusteditor-speech` |

The dependency arrows point one way:

```
rusteditor -> rusteditor-speech -> rusteditor-host -> {export, project, media} -> core
                                       -> render -> core
                                          export -> effects -> project
rusteditor-cli -> rusteditor-server -> rusteditor-api -> rusteditor-host
```

If you ever find yourself wanting `core` to depend on `media`, something has
been put in the wrong crate.

`rusteditor` is the app. It opens project folders through `rusteditor-host`, reads
the engine's project to draw the bin and the lanes, and writes every edit as
a `rusteditor-project` command.

## Build and run

```sh
cargo build
cargo test
cargo run -p rusteditor-cli -- probe some-video.mp4
cargo run -p rusteditor-cli -- render some-video.mp4 out.mp4 --frames 120
cargo run -p rusteditor-cli -- api '{"method":"catalogue.list","kind":"filter"}'
cargo run -p rusteditor-cli -- api < edits.jsonl   # one JSON-RPC call per line; see below
cargo run -p rusteditor-cli -- serve               # the same API on 127.0.0.1:7420
cargo run -p rusteditor-cli --features grpc -- serve --grpc 127.0.0.1:7421
cargo run -p rusteditor                    # the editor window, debug
cargo run --profile quick -p rusteditor    # optimised, rebuilds in seconds: for trying changes
cargo build --profile app -p rusteditor    # the shipping binary: fat LTO, panic=abort, stripped
```

Nothing is spawned at run time: FFmpeg is linked, whisper.cpp and sherpa-onnx
are compiled in. A build needs the FFmpeg 7+ development libraries (headers
and import libraries) - `brew install ffmpeg` on macOS, a BtbN `shared` build
unpacked and pointed at with `FFMPEG_DIR` on Windows and on Linux
distributions whose packaged FFmpeg is older than 7 - plus cmake and a C++
toolchain for whisper.cpp. The window draws with Slint's Skia renderer on
every platform, from prebuilt Skia binaries fetched at build time.
On Linux, Skia needs the fontconfig and freetype headers at build time (see
`.github/workflows/build-app.yml` for the package list).

On Nix, the flake at the repository root builds the window with every native
dependency pinned: `nix build` (then `./result/bin/rusteditor`), `nix run`, or
`nix develop` for a shell with the toolchain and libraries in it. Linux
x86_64 and aarch64.


## Driving RustEditor without the window

The full reference - every method, every edit command, every type, and a
page per transport - lives on the website at
[rusteditor.standalones.org/docs](https://rusteditor.standalones.org/docs). What follows
is the short version.

Everything the window does to a project, a script can do through the RustEditor
API: `rusteditor-api` is the one dispatcher, and the transports only carry it.
On a line - stdin, TCP, a Unix socket - a call is JSON-RPC 2.0, one object
per line, and the response comes back with the call's id:

```jsonl
{"jsonrpc":"2.0","id":1,"method":"project.open","params":{"path":"/edits/Reel"}}
{"jsonrpc":"2.0","id":1,"result":{"project":{...},"canUndo":false,...}}
{"jsonrpc":"2.0","id":2,"method":"edit.apply","params":{"path":"/edits/Reel","command":{"op":"addTextClip","start":1.5}}}
{"jsonrpc":"2.0","id":2,"result":{...}}
{"jsonrpc":"2.0","id":3,"method":"export.run","params":{"path":"/edits/Reel","output":"/edits/reel.mp4"}}
{"jsonrpc":"2.0","id":3,"result":{"job":"j1","path":"/edits/Reel","output":"/edits/reel.mp4"}}
{"jsonrpc":"2.0","method":"export.progress","params":{"job":"j1","path":"/edits/Reel","frame":30,"total":900,"stage":"video"}}
{"jsonrpc":"2.0","method":"export.done","params":{"job":"j1","path":"/edits/Reel","output":"/edits/reel.mp4","width":1920,"height":1080}}
```

An `edit.apply` carries a `rusteditor-project` command as it is, so every edit
the window can make is one a caller can make, with the same refusals. An
error is `{"code": -32001, "message": "...", "data": {"code": "notOpen"}}`:
the number is JSON-RPC's, the name in `data` is the API's - `parse`,
`invalid`, `notOpen`, `notFound`, `refused`, `busy`, `cancelled`, `failed`,
`unauthorized`. Exports are jobs: the response names one at once and its
events follow, to every connected caller, each naming its job and project.

`rusteditor-cli serve` puts the same lines on a socket. It binds loopback unless
told otherwise, and every connection, loopback included, presents a token
as its first line, `{"jsonrpc":"2.0","id":0,"method":"auth","params":{"token":"..."}}`:
the one given with `--token` (or `RUSTEDITOR_API_TOKEN`), or else one minted
at start and printed under the addresses, so only whoever started the
server can hand it out. There is no encryption; a bind off loopback belongs
behind something that has it. With the `grpc` feature the same API is
served over HTTP/2 from `crates/rusteditor-server/proto/rusteditor.proto`, a thin
envelope carrying the same JSON, with the token as `authorization: Bearer
...` metadata. `version` is the call to make first: its reply's
`capabilities` names what the build serves (`events`, `json-rpc`,
`unix-socket`, `grpc`, `gpu`) before anything is asked of it.

## Reading this codebase cold

1. The layout table and dependency arrows above - the map of the whole system.
2. `crates/*/src/lib.rs` - every crate opens with a `//!` block saying what it is for.
3. `cargo doc --open` - the generated API map of the whole engine.

## Conventions

- **Time is exact.** All timestamps are `rusteditor_core::time::Rational` seconds, never `f64`.
  Frame-accurate editing and floating point do not mix.
- **Graphs use handles, not pointers.** `rusteditor_core::arena` explains why.
- **Shallow generics.** Concrete types until three call sites demand otherwise.
- **Threads, not async.** The render path is CPU-bound; `async` buys nothing here.

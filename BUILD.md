# Building Orpheus from source (Oberon-style)

This distribution ships the complete source alongside the compiled `latte` binary. The system
is meant to be modified and rebuilt in place, the way an Oberon system is recompiled from itself.

## Rebuild the whole system
Requires Rust (rustc/cargo) 1.75+ and no network (zero external crates):

    cargo build --offline --release

The binary appears at `target/release/latte`; copy it over `./latte`.

## Recompile just the libraries (no Rust rebuild)
The `.lat` libraries in `lib/` are the system's own code. You can edit and reload them
*without* rebuilding the binary:

- at the command line:   `latte eval --lib NAME=lib/NAME.lat "(your expr)"`
- from the GUI:          open `/compile`, paste a `core NAME …` module, press **Compile & load**
- over the network:      `POST /api/compile` with the module source, or `POST /api/lib`

A reloaded library is immediately importable and callable across the running system — the
Oberon loop of edit → compile → use, without leaving the image.

## Layout
- `src/`        Rust host (VM `loom`, compiler `latte`, server `serve`/Hymn, tools)
- `lib/*.lat`   the Latte standard libraries and apps (std, num, tensor, ml, plan, chess, chessml, …)
- `lib/site/`   the GUI (System console, editor, charts, planner, compiler)
- `docs/`       guides (adding libraries, visualization & ML)

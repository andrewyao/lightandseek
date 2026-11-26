# CLAUDE.md

lightandseek: a multiplayer hide-and-seek game in Rust. The browser client (wasm32, Leptos chrome, wgpu canvas on WebGPU or WebGL2) talks over a WebSocket to an axum room server.

## Commands

```sh
just check                  # every gate a commit must pass: test, clippy -D warnings, wasm check, trunk build
cargo test -p game <name>   # one test by name substring
just serve                  # release web build, then the server on :8080 (PORT, LIGHTSEEK_DIST override)
```

Run `just check` before committing. `trunk` must be installed for the `web` gate.

## Crates

- `game`: the rules, worlds, bots, and the whole sync protocol, with no I/O. No tokio, no wasm-bindgen, no clock, no OS randomness. `rng` (seeded PCG32) is the only randomness, and time comes in as arguments, so a whole round runs headless in a test. `authority::Room` is the server half, `replica::Replica` the client half, and `wire` is the byte format. Shells treat `Frame` as opaque.
- `engine`: the renderer. It knows nothing about the game; callers describe a frame as lit shapes and wedges seen through a `View`. It also renders headless PNGs natively.
- `server`: a thin tokio + axum shell around `game::Room`. `GET /ws/{room}` upgrades; everything else serves `crates/client/dist`.
- `client`: `lib.rs` holds the pure parts, built for the host too so `cargo test` covers them; `main.rs` is the wasm shell.

Keep game logic in `game`, not in the shells. Logic shared by server, bots, and prediction (such as `physics`) has one implementation.

## Seeing a change

```sh
cargo run -p client --example scenes -- [out-dir]   # park, city, kitchen to PNGs, top-down and seeker camera
cargo run -p engine --example scene -- [out.png]    # hand-built engine sample
```

In the browser, `window.__lightseek.state()` returns the JSON in `client::probe`, and `input(fwd, strafe, yaw)` / `release()` steer without a keyboard (`crates/client/src/debug.rs`). Use them for automated browser checks.

## Rust rules

- **Input from the network is hostile.** A malformed or oversized frame, a bad room name, or a client that stops reading must give an error or a disconnect, never a panic or unbounded memory. Decode through `wire` and return `WireError`. Use `get()` and checked or saturating math on input-derived numbers, and cap anything sized by input.
- **Non-test code doesn't panic on fallible paths.** Return a `Result` with `?`, or use `let … else`/`if let`. `expect` only for a true internal invariant, with a message saying why it holds (`"checked above"`). No `unwrap()` outside tests.
- **No `unsafe`.** The workspace sets `unsafe_code = "forbid"`.
- **Determinism.** `game` code must give the same result from the same seed and inputs. Use `BTreeMap`/`BTreeSet` rather than hash maps, and no wall-clock time or thread RNG there.
- **Prove it:** a bug fix lands with a test that failed before it. Fix a lint rather than `#[allow]` it; an allow carries a one-line reason.

## Commit messages

Do not add a `Co-Authored-By` (or similar co-author) trailer to commit messages or PR descriptions.

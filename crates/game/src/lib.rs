//! lightandseek `game`: the rules, the world, and the whole sync protocol,
//! with no I/O.
//!
//! - `geom`       ground-plane vectors, footprints, the yaw convention.
//! - `model`      ids, roles, `Input`, `Phase`, `LobbyConfig`, the roster.
//! - `rng`        a seeded PCG32. The only source of randomness.
//! - `world`      scene generators, `World`, `ray_hit`, the server-only nav grid.
//! - `physics`    one fixed step, shared by server, bots, and prediction.
//! - `rules`      speeds, the catch test, phase lengths, limits.
//! - `bots`       seeker and hider brains that emit `Input`s.
//! - `authority`  the server half. `Room`: lobby, membership, rounds, ticks.
//! - `replica`    the client half. Prediction, reconciliation, interpolation.
//! - `wire`       the byte format both halves speak. Shells see `Frame` only.
//!
//! `Room` and `Replica` are plain structs with `&mut self` methods, so a whole
//! round runs headlessly in a test. No tokio, no wasm-bindgen, no clock, no
//! OS randomness. The shell passes the seed to `Room::new`, calls
//! `Room::tick` every 50 ms, and passes `now_ms` to the `Replica`.

mod authority;
mod bots;
pub mod geom;
mod model;
pub mod physics;
mod replica;
mod rng;
pub mod rules;
mod wire;
mod world;

pub use authority::{Room, Step, To};
pub use geom::{Aabb2, Footprint, Vec2, Vec3};
pub use model::{
    AvatarState, Body, ConnId, Control, Input, Link, LobbyConfig, Member, Phase, PlayerId, Refusal,
    Role, SceneKind, Seat, Status, Tick,
};
pub use replica::{AvatarView, Event, Replica, ViewState};
pub use wire::{Frame, WireError};
pub use world::{Material, Pad, Prop, Shape, Solid, World};

//! lightandseek `game`: the rules, the world, and the whole sync protocol,
//! with no I/O.
//!
//! - `geom`       ground-plane vectors and footprints. Floats are finite here.
//! - `model`      ids, roles, `Input`, `Phase`, `LobbyConfig`, the roster.
//! - `world`      scene generators, `World`, and the server-only nav grid.
//! - `physics`    one fixed step, shared by server, bots, and prediction.
//! - `rules`      speeds, the catch test, phase lengths.
//! - `wire`       the byte format both halves speak. Shells see `Frame` only.
//!
//! The server half is `Room` and the client half is `Replica`. Both are plain
//! structs with `&mut self` methods, so a whole round runs headlessly in a
//! test. No tokio, no wasm-bindgen, no clock, no OS randomness: the shell
//! passes the seed in and calls `Room::tick` every 50 ms.

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
pub use world::{Prop, Shape, Solid, World};

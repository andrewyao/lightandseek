//! The renderer. It knows nothing about the game: callers describe a frame
//! as lit shapes and ground wedges (`draw`) seen through a `View`.
//!
//! - `draw`    the draw model, plus the avatar figure.
//! - `camera`  follow and orbit cameras.

pub mod camera;
pub mod draw;

pub use camera::{FollowCamera, OrbitCamera, View};
pub use draw::{
    AVATAR_HEIGHT, Environment, Instance, SceneDraw, Shape, Wedge, avatar_instances, facing,
};

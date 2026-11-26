//! A frame of the game as the engine draws it: props and avatars as lit
//! shapes, the seeker's catch wedge, and each scene's sky and ground.
//!
//! `game::Vec2(x, y)` is world `(x, 0, y)`; both crates put yaw 0 on +Z.

use engine::{
    Environment, FollowCamera, Instance, OrbitCamera, SceneDraw, Shape, View, Wedge,
    avatar_instances,
};
use game::rules::{CATCH_DIST, CATCH_HALF_ANGLE};
use game::{AvatarView, PlayerId, Role, SceneKind, Status, Vec2, ViewState, World};
use glam::Vec3;

pub const SEEKER: [f32; 3] = [0.9, 0.16, 0.12];
const WEDGE: [f32; 4] = [0.95, 0.05, 0.05, 0.8];

/// Hider colours, none of them close to the seeker's red.
const PALETTE: [[f32; 3]; 8] = [
    [0.22, 0.45, 0.95],
    [0.98, 0.8, 0.15],
    [0.2, 0.75, 0.35],
    [0.62, 0.35, 0.9],
    [0.98, 0.55, 0.1],
    [0.15, 0.8, 0.85],
    [0.95, 0.45, 0.75],
    [0.95, 0.95, 0.95],
];

/// A player's colour for this round: red when seeking, else fixed by id.
pub fn color(id: PlayerId, role: Role) -> [f32; 3] {
    match role {
        Role::Seeker => SEEKER,
        Role::Hider => PALETTE[id.0 as usize % PALETTE.len()],
    }
}

pub fn ground(p: Vec2) -> Vec3 {
    Vec3::new(p.x, 0.0, p.y)
}

pub fn environment(kind: SceneKind) -> Environment {
    let sun = Vec3::new(0.45, 1.0, 0.3);
    match kind {
        SceneKind::Park => Environment {
            sky: [0.6, 0.78, 0.95],
            ground: [0.4, 0.62, 0.3],
            sun,
            fog: [30.0, 95.0],
        },
        SceneKind::City => Environment {
            sky: [0.72, 0.75, 0.8],
            ground: [0.3, 0.31, 0.34],
            sun: Vec3::new(-0.5, 1.0, 0.25),
            fog: [25.0, 85.0],
        },
        SceneKind::Kitchen => Environment {
            // Ambient light mixes sky and ground, so a bright ceiling-cream
            // sky keeps the shaded side of the walls from going brown.
            sky: [0.8, 0.74, 0.66],
            ground: [0.74, 0.74, 0.72],
            sun: Vec3::new(0.2, 1.0, 0.4),
            fog: [35.0, 90.0],
        },
    }
}

/// Owned instances and wedges, so a frame can borrow them as a `SceneDraw`.
#[derive(Clone, Debug)]
pub struct Scene {
    pub instances: Vec<Instance>,
    pub wedges: Vec<Wedge>,
    pub env: Environment,
}

impl Scene {
    pub fn draw(&self) -> SceneDraw<'_> {
        SceneDraw {
            instances: &self.instances,
            wedges: &self.wedges,
            env: self.env,
        }
    }
}

/// Every avatar in `view`: yours, predicted, then everyone else.
pub fn avatars(view: &ViewState<'_>) -> Vec<AvatarView> {
    view.me.iter().chain(&view.others).copied().collect()
}

/// `world` with `avatars` standing in it. Caught players are not drawn.
pub fn scene(world: &World, avatars: &[AvatarView]) -> Scene {
    let mut instances: Vec<Instance> = world
        .props
        .iter()
        .map(|p| {
            let shape = match p.shape {
                game::Shape::Box => Shape::Box,
                game::Shape::Cylinder => Shape::Cylinder,
            };
            let c = Vec3::new(p.center.x, p.center.y, p.center.z);
            let size = Vec3::new(p.size.x, p.size.y, p.size.z);
            Instance::new(shape, c, size, 0.0, p.color.map(|v| f32::from(v) / 255.0))
        })
        .collect();
    let mut wedges = Vec::new();
    for a in avatars.iter().filter(|a| a.status == Status::Active) {
        let feet = ground(a.body.pos);
        instances.extend(avatar_instances(feet, a.body.yaw, color(a.id, a.role)));
        if a.role == Role::Seeker {
            wedges.push(Wedge {
                center: feet + Vec3::Y * 0.02,
                yaw: a.body.yaw,
                half_angle: CATCH_HALF_ANGLE,
                radius: CATCH_DIST,
                color: WEDGE,
            });
        }
    }
    Scene {
        instances,
        wedges,
        env: environment(world.kind),
    }
}

/// Third person behind `feet`, looking along `yaw`, pulled in front of any
/// solid between the avatar and the eye.
pub fn follow(world: &World, feet: Vec2, yaw: f32, aspect: f32) -> View {
    let cam = FollowCamera::new(ground(feet), yaw);
    cam.view_clipped(aspect, |from, to| {
        let g = |v: Vec3| game::Vec3::new(v.x, v.y, v.z);
        world.ray_hit(g(from), g(to)).unwrap_or(f32::INFINITY)
    })
}

/// A spectator's view: circling `feet` from above at `yaw`.
pub fn orbit(feet: Vec2, yaw: f32, aspect: f32) -> View {
    let mut cam = OrbitCamera::new(ground(feet));
    cam.yaw = yaw;
    cam.pitch = 0.75;
    cam.distance = 14.0;
    cam.view(aspect)
}

#[cfg(test)]
mod tests {
    use game::{Body, Control, Prop, SceneKind};

    use super::*;

    fn avatar(id: u32, role: Role, status: Status, pos: Vec2) -> AvatarView {
        AvatarView {
            id: PlayerId(id),
            body: Body { pos, yaw: 0.5 },
            role,
            status,
            control: Control::Human,
        }
    }

    fn centre(i: &Instance) -> Vec3 {
        i.model.transform_point3(Vec3::ZERO)
    }

    #[test]
    fn props_become_instances_with_their_colour_and_place() {
        let mut world = World::open(SceneKind::Park);
        world.props.push(Prop {
            shape: game::Shape::Cylinder,
            center: game::Vec3::new(1.0, 2.0, 3.0),
            size: game::Vec3::new(0.5, 4.0, 0.5),
            color: [255, 0, 51],
        });
        let s = scene(&world, &[]);
        assert_eq!(s.instances.len(), 1);
        let i = s.instances[0];
        assert_eq!(i.shape, Shape::Cylinder);
        assert_eq!(i.color, [1.0, 0.0, 0.2]);
        assert!(centre(&i).abs_diff_eq(Vec3::new(1.0, 2.0, 3.0), 1e-6));
        assert!(s.wedges.is_empty());
    }

    #[test]
    fn the_seeker_is_red_with_a_catch_wedge_and_the_caught_are_not_drawn() {
        let world = World::open(SceneKind::City);
        let seeker = avatar(1, Role::Seeker, Status::Active, Vec2::new(2.0, -1.0));
        let hider = avatar(2, Role::Hider, Status::Active, Vec2::new(5.0, 5.0));
        let caught = avatar(3, Role::Hider, Status::Caught, Vec2::new(-5.0, 0.0));
        let s = scene(&world, &[seeker, hider, caught]);

        assert_eq!(s.instances.len(), 6, "two figures of three shapes");
        assert_eq!(s.instances[0].color, SEEKER);
        assert!(centre(&s.instances[0]).x == 2.0 && centre(&s.instances[0]).z == -1.0);
        assert_ne!(s.instances[3].color, SEEKER);
        assert!(
            s.instances.iter().all(|i| centre(i).x > -4.0),
            "no caught figure"
        );

        assert_eq!(s.wedges.len(), 1);
        let w = s.wedges[0];
        assert_eq!(
            (w.radius, w.half_angle, w.yaw),
            (CATCH_DIST, CATCH_HALF_ANGLE, 0.5)
        );
        assert!(w.center.x == 2.0 && w.center.z == -1.0);
        assert!(w.color[0] > 0.9 && w.color[1] < 0.1);
    }

    #[test]
    fn hider_colours_are_fixed_per_player_and_never_red() {
        for id in 0..16 {
            let c = color(PlayerId(id), Role::Hider);
            assert_eq!(c, color(PlayerId(id), Role::Hider));
            assert_ne!(c, SEEKER);
        }
        assert_ne!(
            color(PlayerId(1), Role::Hider),
            color(PlayerId(2), Role::Hider)
        );
    }

    #[test]
    fn each_scene_has_its_own_environment() {
        let [p, c, k] = SceneKind::ALL.map(environment);
        assert!(p != c && c != k && p != k);
    }

    #[test]
    fn the_follow_camera_stays_on_the_near_side_of_a_wall() {
        let mut world = World::generate(SceneKind::Kitchen, 3);
        world.solids.clear();
        let open = follow(&world, Vec2::ZERO, 0.0, 1.5);
        world.solids.push(game::Solid {
            shape: game::Footprint::Aabb(game::Aabb2::centered(
                Vec2::new(0.0, -2.0),
                Vec2::new(5.0, 0.2),
            )),
            height: 3.0,
        });
        let walled = follow(&world, Vec2::ZERO, 0.0, 1.5);
        assert!(open.eye.z < -2.5, "behind the avatar: {}", open.eye);
        assert!(
            walled.eye.z > -1.8,
            "pulled in front of the wall: {}",
            walled.eye
        );
    }
}

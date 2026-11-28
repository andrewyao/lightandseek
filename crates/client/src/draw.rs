//! A frame of the game as the engine draws it: props and avatars as lit
//! shapes, the seeker's catch wedge, and each scene's sky and ground.
//!
//! `game::Vec2(x, y)` is world `(x, 0, y)`; both crates put yaw 0 on +Z.

use engine::{
    Environment, FollowCamera, Instance, Material, SceneDraw, Shape, View, Wedge, avatar_instances,
};
use game::rules::{CATCH_DIST, CATCH_HALF_ANGLE};
use game::{AvatarView, Body, PlayerId, Role, SceneKind, Status, Vec2, ViewState, World, Zone};
use glam::Vec3;

use crate::gait::Gaits;

pub const SEEKER: [f32; 3] = [0.9, 0.16, 0.12];
const WEDGE: [f32; 4] = [0.95, 0.05, 0.05, 0.8];
const PAD: [f32; 3] = [0.25, 0.95, 1.0];
const FOOD: [f32; 3] = [1.0, 0.62, 0.18];
const FOOD_SIZE: f32 = 0.35;
const DANGER: [f32; 3] = [1.0, 0.12, 0.1];
/// The zone's edge is drawn as glowing strips this wide.
const EDGE_WIDTH: f32 = 0.15;
/// Rings rise off each pad, this many at once, each over this long.
const PAD_RINGS: usize = 3;
const RING_MS: f64 = 1200.0;
const RING_RISE: f32 = 2.5;

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

/// Where `body`'s feet are in world space.
pub fn feet(body: Body) -> Vec3 {
    Vec3::new(body.pos.x, body.y, body.pos.y)
}

pub fn environment(kind: SceneKind) -> Environment {
    let sun = Vec3::new(0.45, 1.0, 0.3);
    match kind {
        SceneKind::Park => Environment {
            sky: [0.6, 0.78, 0.95],
            ground: [0.4, 0.62, 0.3],
            sun,
            ground_material: Material::Grass,
            fog: [30.0, 95.0],
        },
        SceneKind::City => Environment {
            sky: [0.72, 0.75, 0.8],
            ground: [0.3, 0.31, 0.34],
            sun: Vec3::new(-0.5, 1.0, 0.25),
            ground_material: Material::Asphalt,
            fog: [25.0, 85.0],
        },
        SceneKind::Kitchen => Environment {
            // Ambient light mixes sky and ground, so a bright ceiling-cream
            // sky keeps the shaded side of the walls from going brown.
            sky: [0.8, 0.74, 0.66],
            ground: [0.74, 0.74, 0.72],
            // Near overhead, so 40 u walls do not shade the whole floor.
            sun: Vec3::new(0.1, 1.0, 0.15),
            ground_material: Material::Concrete,
            fog: [60.0, 140.0],
        },
    }
}

fn material(m: game::Material) -> Material {
    match m {
        game::Material::Plain => Material::Plain,
        game::Material::Wood => Material::Wood,
        game::Material::Brick => Material::Brick,
        game::Material::Plaster => Material::Plaster,
        game::Material::Concrete => Material::Concrete,
        game::Material::Asphalt => Material::Asphalt,
        game::Material::Tile => Material::Tile,
        game::Material::Grass => Material::Grass,
        game::Material::Foliage => Material::Foliage,
        game::Material::Fabric => Material::Fabric,
        game::Material::Metal => Material::Metal,
        game::Material::Water => Material::Water,
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
    /// Adds ripe `food` and the edge of `zone`: red strips on the ground
    /// along its sides and a red frame at its ceiling.
    pub fn with_supplies(mut self, zone: Option<Zone>, food: &[game::Vec3]) -> Scene {
        for f in food {
            let at = Vec3::new(f.x, f.y + FOOD_SIZE / 2.0, f.z);
            self.instances.push(
                Instance::new(Shape::Box, at, Vec3::splat(FOOD_SIZE), 0.6, FOOD)
                    .with_material(Material::Glow),
            );
        }
        let Some(z) = zone.filter(|z| z.half < Zone::EVERYWHERE.half) else {
            return self;
        };
        let (h, w) = (z.half, EDGE_WIDTH);
        for y in [0.03, z.ceiling] {
            for (center, size) in [
                (Vec3::new(0.0, y, h), Vec3::new(2.0 * h, w, w)),
                (Vec3::new(0.0, y, -h), Vec3::new(2.0 * h, w, w)),
                (Vec3::new(h, y, 0.0), Vec3::new(w, w, 2.0 * h)),
                (Vec3::new(-h, y, 0.0), Vec3::new(w, w, 2.0 * h)),
            ] {
                self.instances.push(
                    Instance::new(Shape::Box, center, size, 0.0, DANGER)
                        .with_material(Material::Glow),
                );
            }
        }
        self
    }

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

/// `world` with `avatars` standing in it, each mid-stride as `gaits` has
/// it. Caught players are not drawn, and neither is `hidden`'s figure,
/// though a seeker's wedge still is.
pub fn scene(
    world: &World,
    avatars: &[AvatarView],
    hidden: Option<PlayerId>,
    gaits: &Gaits,
) -> Scene {
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
                .with_material(material(p.material))
        })
        .collect();
    let mut wedges = Vec::new();
    for d in &world.pads {
        let at = Vec3::new(d.center.x, d.y, d.center.y);
        let disc = Vec3::new(2.0 * d.radius, 0.04, 2.0 * d.radius);
        instances.push(
            Instance::new(Shape::Cylinder, at + Vec3::Y * 0.02, disc, 0.0, PAD)
                .with_material(Material::Glow),
        );
        wedges.extend(pad_rings(at, d.radius, gaits.at_ms()));
    }
    for a in avatars.iter().filter(|a| a.status == Status::Active) {
        let feet = feet(a.body);
        if hidden != Some(a.id) {
            instances.extend(avatar_instances(
                feet,
                a.body.yaw,
                color(a.id, a.role),
                gaits.pose(a.id),
            ));
        }
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

/// Translucent rings rising off a pad at `at` and fading as they go.
fn pad_rings(at: Vec3, radius: f32, now_ms: f64) -> impl Iterator<Item = Wedge> {
    (0..PAD_RINGS).map(move |i| {
        let t = ((now_ms / RING_MS + i as f64 / PAD_RINGS as f64).fract()) as f32;
        let [r, g, b] = PAD;
        Wedge {
            center: at + Vec3::Y * (0.05 + t * RING_RISE),
            yaw: 0.0,
            half_angle: std::f32::consts::PI,
            radius: radius * (1.0 - 0.3 * t),
            color: [r, g, b, 0.45 * (1.0 - t)],
        }
    })
}

/// Third person behind `body`, looking along `yaw`.
pub fn follow(body: Body, yaw: f32) -> FollowCamera {
    FollowCamera::new(feet(body), yaw)
}

/// A spectator's camera: higher and further back than `follow`, circling
/// `body` at `yaw`.
pub fn orbit(body: Body, yaw: f32) -> FollowCamera {
    FollowCamera {
        pitch: 0.6,
        distance: 9.0,
        ..FollowCamera::new(feet(body), yaw)
    }
}

/// How far back `cam` can sit without a solid between the eye and the
/// watched avatar.
pub fn free_distance(world: &World, cam: &FollowCamera) -> f32 {
    cam.free_distance(|from, to| {
        let g = |v: Vec3| game::Vec3::new(v.x, v.y, v.z);
        world.ray_hit(g(from), g(to)).unwrap_or(f32::INFINITY)
    })
}

/// `cam` as far back as `world` lets it, with no easing.
pub fn clipped(world: &World, cam: FollowCamera, aspect: f32) -> View {
    cam.view_at(free_distance(world, &cam), aspect)
}

/// How long the eye takes to get about two thirds of the way back out after
/// a wall stops pulling it in.
const EASE_OUT_MS: f64 = 350.0;

/// The eye's distance from the followed avatar across frames. A wall pulls
/// it in at once, so the eye never sees through it; once the way clears it
/// eases back out, so passing a pole does not make it pop.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Zoom {
    pub distance: f32,
    at_ms: f64,
}

impl Zoom {
    /// This frame's distance, given `free_distance` now.
    pub fn next(prev: Option<Zoom>, free: f32, now_ms: f64) -> Zoom {
        let distance = match prev {
            Some(z) if z.distance < free => {
                let dt = (now_ms - z.at_ms).max(0.0);
                let k = 1.0 - (-dt / EASE_OUT_MS).exp() as f32;
                z.distance + (free - z.distance) * k
            }
            _ => free,
        };
        Zoom {
            distance,
            at_ms: now_ms,
        }
    }
}

/// What `cam` sees at `zoom`'s distance. `own` is your avatar when `cam`
/// follows it, left out when the eye is too close to it.
pub fn shot(
    world: &World,
    avatars: &[AvatarView],
    cam: &FollowCamera,
    zoom: Zoom,
    own: Option<PlayerId>,
    aspect: f32,
    gaits: &Gaits,
) -> (View, Scene) {
    let hidden = own.filter(|_| zoom.distance < engine::camera::CLOSE);
    (
        cam.view_at(zoom.distance, aspect),
        scene(world, avatars, hidden, gaits),
    )
}

#[cfg(test)]
mod tests {
    use game::{Body, Control, Prop, SceneKind};

    use super::*;

    fn avatar(id: u32, role: Role, status: Status, pos: Vec2) -> AvatarView {
        AvatarView {
            id: PlayerId(id),
            body: Body {
                pos,
                yaw: 0.5,
                ..Body::default()
            },
            role,
            status,
            control: Control::Human,
        }
    }

    fn centre(i: &Instance) -> Vec3 {
        i.model.transform_point3(Vec3::ZERO)
    }

    #[test]
    fn props_become_instances_with_their_colour_material_and_place() {
        let mut world = World::open(SceneKind::Park);
        world.props.push(Prop {
            shape: game::Shape::Cylinder,
            center: game::Vec3::new(1.0, 2.0, 3.0),
            size: game::Vec3::new(0.5, 4.0, 0.5),
            color: [255, 0, 51],
            material: game::Material::Brick,
        });
        let s = scene(&world, &[], None, &Gaits::default());
        assert_eq!(s.instances.len(), 1);
        let i = s.instances[0];
        assert_eq!(i.shape, Shape::Cylinder);
        assert_eq!(i.color, [1.0, 0.0, 0.2]);
        assert_eq!(i.material, Material::Brick);
        assert!(centre(&i).abs_diff_eq(Vec3::new(1.0, 2.0, 3.0), 1e-6));
        assert!(s.wedges.is_empty());
    }

    #[test]
    fn pads_glow_and_send_up_rings_that_fade_as_they_rise() {
        let mut world = World::open(SceneKind::Park);
        world.pads.push(game::Pad {
            center: Vec2::new(3.0, -2.0),
            radius: 0.7,
            y: 1.5,
            boost: 10.0,
        });
        let mut gaits = Gaits::default();
        gaits.update(&[], 400.0);
        let s = scene(&world, &[], None, &gaits);
        assert_eq!(s.instances.len(), 1);
        let disc = s.instances[0];
        assert_eq!(disc.material, Material::Glow);
        assert!(centre(&disc).abs_diff_eq(Vec3::new(3.0, 1.52, -2.0), 1e-5));
        assert_eq!(s.wedges.len(), PAD_RINGS);
        let mut rings = s.wedges.clone();
        rings.sort_by(|a, b| a.center.y.total_cmp(&b.center.y));
        assert!(rings.iter().all(|w| w.center.y > 1.5), "above the pad");
        assert!(
            rings.windows(2).all(|w| w[0].color[3] > w[1].color[3]),
            "higher rings are fainter"
        );
        gaits.update(&[], 700.0);
        let later = scene(&world, &[], None, &gaits);
        assert_ne!(later.wedges[0].center.y, s.wedges[0].center.y, "they move");
    }

    #[test]
    fn the_seeker_is_red_with_a_catch_wedge_and_the_caught_are_not_drawn() {
        let world = World::open(SceneKind::City);
        let seeker = avatar(1, Role::Seeker, Status::Active, Vec2::new(2.0, -1.0));
        let hider = avatar(2, Role::Hider, Status::Active, Vec2::new(5.0, 5.0));
        let caught = avatar(3, Role::Hider, Status::Caught, Vec2::new(-5.0, 0.0));
        let s = scene(&world, &[seeker, hider, caught], None, &Gaits::default());
        let parts = avatar_instances(Vec3::ZERO, 0.0, SEEKER, engine::Pose::default()).len();

        assert_eq!(s.instances.len(), 2 * parts, "two figures");
        assert_eq!(s.instances[0].color, SEEKER);
        assert!(centre(&s.instances[0]).x == 2.0 && centre(&s.instances[0]).z == -1.0);
        assert_ne!(s.instances[parts].color, SEEKER);
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
    fn ripe_food_glows_and_the_zone_edge_is_drawn_on_the_ground_and_at_its_ceiling() {
        let world = World::open(SceneKind::Park);
        let base = scene(&world, &[], None, &Gaits::default());
        let food = [game::Vec3::new(3.0, 0.0, -2.0)];
        let zone = Zone {
            half: 15.0,
            ceiling: 6.0,
        };
        let s = base.clone().with_supplies(Some(zone), &food);
        let added = &s.instances[base.instances.len()..];
        assert_eq!(added.len(), 1 + 8, "one food, four strips twice");
        assert!((centre(&added[0]) - Vec3::new(3.0, FOOD_SIZE / 2.0, -2.0)).length() < 1e-4);
        assert!(added.iter().all(|i| i.material == Material::Glow));
        let heights: Vec<f32> = added[1..].iter().map(|i| centre(i).y).collect();
        assert!(heights.contains(&6.0) && heights.contains(&0.03));
        assert!(
            added[1..]
                .iter()
                .all(|i| centre(i).x.abs().max(centre(i).z.abs()) == 15.0)
        );

        let lobby = base.clone().with_supplies(Some(Zone::EVERYWHERE), &[]);
        assert_eq!(
            lobby.instances.len(),
            base.instances.len(),
            "no edge to draw"
        );
    }

    #[test]
    fn each_scene_has_its_own_environment() {
        let [p, c, k] = SceneKind::ALL.map(environment);
        assert!(p != c && c != k && p != k);
    }

    #[test]
    fn a_jumping_avatar_is_drawn_and_followed_at_its_height() {
        let world = World::open(SceneKind::Park);
        let mut a = avatar(1, Role::Seeker, Status::Active, Vec2::ZERO);
        a.body.y = 2.0;
        let s = scene(&world, &[a], None, &Gaits::default());
        let low = s
            .instances
            .iter()
            .map(|i| centre(i).y)
            .fold(f32::INFINITY, f32::min);
        assert!(low > 2.0 && low < 2.2, "figure stands at y = 2: {low}");
        assert!((s.wedges[0].center.y - 2.02).abs() < 1e-5, "wedge too");
        let cam = follow(a.body, 0.0);
        assert_eq!(cam.target.y, 2.0);
    }

    #[test]
    fn the_avatar_figure_is_as_tall_as_the_physics_body() {
        assert_eq!(engine::AVATAR_HEIGHT, game::physics::HEIGHT);
    }

    #[test]
    fn the_follow_camera_stays_on_the_near_side_of_a_wall() {
        let mut world = World::generate(SceneKind::Kitchen, 3);
        world.solids.clear();
        let open = clipped(&world, follow(Body::default(), 0.0), 1.5);
        world.solids.push(game::Solid {
            shape: game::Footprint::Aabb(game::Aabb2::centered(
                Vec2::new(0.0, -2.0),
                Vec2::new(5.0, 0.2),
            )),
            height: 3.0,
            base: 0.0,
        });
        let walled = clipped(&world, follow(Body::default(), 0.0), 1.5);
        assert!(open.eye.z < -2.5, "behind the avatar: {}", open.eye);
        assert!(
            walled.eye.z > -1.8,
            "pulled in front of the wall: {}",
            walled.eye
        );
    }

    /// Your avatar's head as a share of the screen height: its projected
    /// radius. None when it is not drawn.
    fn head_size(view: &View, scene: &Scene) -> Option<f32> {
        let skull = scene.instances.iter().find(|i| i.shape == Shape::Sphere)?;
        let radius = skull.model.transform_vector3(Vec3::X).length() / 2.0;
        let dist = centre(skull).distance(view.eye);
        if dist <= radius {
            return Some(f32::INFINITY);
        }
        let angle = (radius / dist).asin();
        Some(angle.tan() / (engine::camera::FOV_Y / 2.0).tan() / 2.0)
    }

    fn backed_against_a_wall(gap: f32) -> World {
        let mut world = World::generate(SceneKind::City, 3);
        world.solids.clear();
        world.props.clear();
        world.pads.clear();
        world.solids.push(game::Solid {
            shape: game::Footprint::Aabb(game::Aabb2::centered(
                Vec2::new(0.0, -gap - 0.5),
                Vec2::new(6.0, 0.5),
            )),
            height: 12.0,
            base: 0.0,
        });
        world
    }

    #[test]
    fn backed_against_a_wall_your_own_head_never_fills_the_view() {
        let mut me = avatar(1, Role::Seeker, Status::Active, Vec2::ZERO);
        me.body.yaw = 0.0;
        let cam = follow(me.body, 0.0);
        let mut drawn_at = Vec::new();
        for gap in [0.3, 0.6, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 6.0] {
            let world = backed_against_a_wall(gap);
            let zoom = Zoom::next(None, free_distance(&world, &cam), 0.0);
            let (view, scene) = shot(
                &world,
                &[me],
                &cam,
                zoom,
                Some(me.id),
                1.6,
                &Gaits::default(),
            );
            assert!(view.eye.z > -gap, "gap {gap}: eye {} in the wall", view.eye);
            assert_eq!(scene.wedges.len(), 1, "gap {gap}: your wedge still shows");
            if let Some(size) = head_size(&view, &scene) {
                assert!(size < 0.1, "gap {gap}: head is {size} of the screen");
                drawn_at.push(gap);
            }
        }
        assert!(
            drawn_at.contains(&6.0),
            "with room behind you, you are drawn"
        );
    }

    #[test]
    fn the_eye_snaps_in_at_once_and_eases_back_out() {
        let z = Zoom::next(None, 4.5, 0.0);
        let z = Zoom::next(Some(z), 0.8, 16.0);
        assert_eq!(z.distance, 0.8, "never lags behind a wall");
        let z = Zoom::next(Some(z), 4.5, 32.0);
        assert!(z.distance > 0.8 && z.distance < 1.2, "{}", z.distance);
        let z = Zoom::next(Some(z), 4.5, 1500.0);
        assert!(z.distance > 4.4, "{}", z.distance);
    }

    #[test]
    fn every_distance_the_eye_eases_out_through_is_clear_of_walls() {
        let g = |v: Vec3| game::Vec3::new(v.x, v.y, v.z);
        let cam = follow(Body::default(), 0.0);
        for height in [0.9, 1.3, 1.6, 2.5] {
            for gap in [0.2, 0.3, 0.5, 0.8, 1.5] {
                let mut world = backed_against_a_wall(gap);
                world.solids[0].height = height;
                let free = free_distance(&world, &cam);
                let blocked = (1..)
                    .map(|i| i as f32 * 0.02)
                    .take_while(|&d| d < free)
                    .find(|&d| {
                        world
                            .ray_hit(g(cam.pivot()), g(cam.view_at(d, 1.0).eye))
                            .is_some()
                    });
                assert_eq!(blocked, None, "{height} tall, {gap} behind, free to {free}");
            }
        }
    }
}

//! Headless renders, read back pixel by pixel.

use std::f32::consts::{FRAC_PI_6, PI};

use engine::{
    Environment, Instance, Offscreen, OrbitCamera, Pose, SceneDraw, Shape, View, Wedge,
    avatar_instances,
};
use glam::Vec3;

const SIZE: [u32; 2] = [160, 120];
const BLUE: [f32; 3] = [0.2, 0.35, 0.95];
const RED: [f32; 3] = [0.95, 0.1, 0.1];

/// Looking down -Z from the +Z side at the origin, slightly from above.
fn camera(off: &Offscreen) -> View {
    let mut cam = OrbitCamera::new(Vec3::new(0.0, 0.5, 0.0));
    cam.yaw = PI;
    cam.pitch = 0.2;
    cam.distance = 6.0;
    cam.view(off.aspect())
}

fn render(off: &mut Offscreen, view: &View, instances: &[Instance], wedges: &[Wedge]) -> Vec<u8> {
    off.render(
        view,
        &SceneDraw {
            instances,
            wedges,
            env: Environment::default(),
        },
    )
}

fn pixel(rgba: &[u8], view: &View, p: Vec3) -> [u8; 3] {
    let [x, y] = view.project(p, SIZE).expect("in front of the eye");
    let i = (y as usize * SIZE[0] as usize + x as usize) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

#[test]
fn a_box_in_front_of_an_avatar_hides_it() {
    let mut off = Offscreen::new(SIZE).unwrap();
    let view = camera(&off);
    let avatar = avatar_instances(Vec3::ZERO, 0.0, BLUE, Pose::default());
    // Boxes draw before capsules and spheres, so only the depth test can
    // keep the avatar behind this one.
    let wall = Instance::new(
        Shape::Box,
        Vec3::new(0.0, 0.6, 2.0),
        Vec3::new(1.4, 1.2, 0.3),
        0.0,
        RED,
    );
    let body = Vec3::new(0.0, 0.4, 0.0);

    let both = render(&mut off, &view, &[&avatar[..], &[wall]].concat(), &[]);
    let wall_only = render(&mut off, &view, &[wall], &[]);
    let avatar_only = render(&mut off, &view, &avatar, &[]);

    let seen = pixel(&both, &view, body);
    assert_eq!(seen, pixel(&wall_only, &view, body));
    assert_ne!(seen, pixel(&avatar_only, &view, body));
    let [r, _, b] = seen;
    assert!(r > b, "the wall's red shows, not the avatar's blue: {seen:?}");
}

#[test]
fn the_visor_shows_only_when_the_avatar_faces_the_camera() {
    let mut off = Offscreen::new(SIZE).unwrap();
    let head = Vec3::new(0.0, engine::AVATAR_HEIGHT - 0.18, 0.0);
    let mut cam = OrbitCamera::new(head);
    cam.yaw = PI;
    cam.pitch = 0.1;
    cam.distance = 2.5;
    let view = cam.view(off.aspect());
    // The camera sits on +Z; yaw 0 faces it.
    let toward = render(
        &mut off,
        &view,
        &avatar_instances(Vec3::ZERO, 0.0, BLUE, Pose::default()),
        &[],
    );
    let away = render(
        &mut off,
        &view,
        &avatar_instances(Vec3::ZERO, PI, BLUE, Pose::default()),
        &[],
    );

    let visor = pixel(&toward, &view, head);
    assert!(visor.iter().all(|&c| c < 60), "dark visor: {visor:?}");
    let [r, g, b] = pixel(&away, &view, head);
    assert!(b > 120 && b > r && b > g, "the back of a blue head: {:?}", [r, g, b]);
}

#[test]
fn a_wedge_tints_the_ground_it_opens_toward_and_nothing_behind() {
    let mut off = Offscreen::new(SIZE).unwrap();
    let mut cam = OrbitCamera::new(Vec3::ZERO);
    cam.pitch = 1.2;
    cam.distance = 8.0;
    let view = cam.view(off.aspect());
    let wedge = Wedge {
        center: Vec3::ZERO,
        yaw: 0.0,
        half_angle: FRAC_PI_6,
        radius: 2.0,
        color: [1.0, 0.0, 0.0, 0.6],
    };
    let ahead = Vec3::new(0.0, 0.0, 1.5);
    let behind = Vec3::new(0.0, 0.0, -1.5);
    let beside = Vec3::new(1.5, 0.0, 0.0);

    let with = render(&mut off, &view, &[], &[wedge]);
    let without = render(&mut off, &view, &[], &[]);

    let [r, g, _] = pixel(&with, &view, ahead);
    assert!(r > g, "red over the grass ahead: {:?}", [r, g]);
    let [r0, g0, _] = pixel(&without, &view, ahead);
    assert!(g0 > r0, "bare grass is green: {:?}", [r0, g0]);
    for p in [behind, beside] {
        assert_eq!(pixel(&with, &view, p), pixel(&without, &view, p), "{p}");
    }
}

//! The running app: the session, the controls, the camera, the GPU, and the
//! signals the Leptos chrome reads.
//!
//! Frames are applied in the socket handler, which also pumps the 50 ms
//! input clock, so a background tab whose rAF is throttled still tracks the
//! room and keeps sending input. rAF only draws.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use client::draw;
use client::gait::Gaits;
use client::input::{Controls, Ticker};
use client::ui::{self, Camera, Hud, Screen};
use game::{Event, LobbyConfig, Phase};
use leptos::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::collab::{Got, Session, Socket};

pub type Shared = Rc<RefCell<Play>>;

const TOAST_MS: f64 = 3000.0;
const MAX_DPR: f64 = 2.0;

pub fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lobby {
    pub is_host: bool,
    pub config: LobbyConfig,
    pub roster: Vec<String>,
    /// Why Start would be refused right now.
    pub blocked: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub text: String,
    pub until_ms: f64,
}

/// Plain copies of app state for the chrome.
#[derive(Copy, Clone, Debug)]
pub struct Ui {
    pub screen: RwSignal<Screen>,
    pub gpu: RwSignal<String>,
    pub name: RwSignal<String>,
    pub lobby: RwSignal<Option<Lobby>>,
    /// The room's last refusal, cleared by the next roster change.
    pub refused: RwSignal<Option<String>>,
    pub hud: RwSignal<Option<Hud>>,
    pub winner: RwSignal<Option<&'static str>>,
    pub toasts: RwSignal<Vec<Toast>>,
}

impl Ui {
    pub fn new(name: String) -> Ui {
        Ui {
            screen: RwSignal::new(Screen::Connecting),
            gpu: RwSignal::new("starting GPU".into()),
            name: RwSignal::new(name),
            lobby: RwSignal::new(None),
            refused: RwSignal::new(None),
            hud: RwSignal::new(None),
            winner: RwSignal::new(None),
            toasts: RwSignal::new(Vec::new()),
        }
    }
}

fn set<T: PartialEq + Send + Sync + 'static>(s: RwSignal<T>, v: T) {
    if s.with_untracked(|old| *old != v) {
        s.set(v);
    }
}

#[derive(Debug)]
pub struct Play {
    pub ui: Ui,
    pub session: Session,
    pub controls: Controls,
    ticker: Ticker,
    pub camera: Camera,
    /// The spectator camera's turn away from behind its target.
    pub orbit_yaw: f32,
    /// Last frame's camera distance; None restarts it at its full reach.
    zoom: Option<draw::Zoom>,
    gaits: Gaits,
    /// Turn the controls to face the avatar's spawn once it appears.
    align: bool,
    pub gpu: Option<engine::Gpu>,
    pub gpu_status: String,
    pub canvas: Option<HtmlCanvasElement>,
    size: [u32; 2],
    toasts: Vec<Toast>,
}

impl Play {
    pub fn new(ui: Ui, session: Session) -> Play {
        Play {
            ui,
            session,
            controls: Controls::default(),
            ticker: Ticker::default(),
            camera: Camera::Follow,
            orbit_yaw: 0.0,
            zoom: None,
            gaits: Gaits::default(),
            align: true,
            gpu: None,
            gpu_status: "starting".into(),
            canvas: None,
            size: [0, 0],
            toasts: Vec::new(),
        }
    }

    /// Applies one frame. False means the socket broke the protocol.
    fn on_frame(&mut self, bytes: &[u8]) -> bool {
        let now = now_ms();
        let event = match self.session.receive(bytes, now) {
            Err(_) => return false,
            Ok(Got::Welcome) => {
                self.align = true;
                None
            }
            Ok(Got::Event(e)) => e,
        };
        match event {
            Some(Event::RoundStart) => {
                self.align = true;
                self.camera = Camera::Follow;
                self.zoom = None;
            }
            Some(Event::Caught { hider, .. }) => {
                let roster = self.session.replica().map(|r| r.view(now).roster.to_vec());
                let name = ui::name_of(&roster.unwrap_or_default(), hider);
                self.toasts.push(Toast {
                    text: format!("{name} was caught"),
                    until_ms: now + TOAST_MS,
                });
            }
            Some(Event::Refused(why)) => self.ui.refused.set(Some(ui::refusal(why))),
            Some(Event::Roster) => self.ui.refused.set(None),
            Some(Event::RoundEnd { .. } | Event::Replaced) | None => {}
        }
        self.pump(now);
        true
    }

    /// Sends every input tick that is due: one per 50 ms, zero moves too.
    fn pump(&mut self, now: f64) {
        for _ in 0..self.ticker.due(now) {
            let Some(r) = self.session.replica_mut() else {
                continue;
            };
            if self.align
                && let Some(me) = r.view(now).me
            {
                self.controls.set_yaw(me.body.yaw);
                self.align = false;
            }
            let (mv, yaw) = self.controls.intent();
            if let Some(f) = r.input(mv, yaw, self.controls.take_jump()) {
                self.session.send(&f);
            }
        }
    }

    /// Draws one animation frame.
    fn frame(&mut self, now: f64) {
        self.pump(now);
        self.fit_canvas();
        let Some(gpu) = &mut self.gpu else { return };
        let aspect = gpu.aspect();
        let view = self.session.replica().map(|r| r.view(now));
        let Some((view, world)) = view.and_then(|v| v.world.map(|w| (v, w))) else {
            let mut cam = engine::OrbitCamera::new(glam::Vec3::ZERO);
            cam.yaw = (now / 9000.0) as f32;
            let empty = engine::SceneDraw {
                instances: &[],
                wedges: &[],
                env: engine::Environment::default(),
            };
            gpu.draw(&cam.view(aspect), &empty);
            return;
        };
        if view.blind() {
            return;
        }
        let avatars = draw::avatars(&view);
        self.gaits.update(&avatars, now);
        let mode = self.camera.resolve(&view);
        let cam = match mode {
            Camera::Follow => view.me.map(|me| engine::FollowCamera {
                look: self.controls.look(),
                ..draw::follow(me.body, self.controls.intent().1)
            }),
            Camera::Spectate(id) => avatars
                .iter()
                .find(|a| a.id == id)
                .map(|a| draw::orbit(a.body, a.body.yaw + self.orbit_yaw)),
        };
        let cam = cam.unwrap_or_else(|| draw::orbit(game::Body::default(), self.orbit_yaw));
        let zoom = draw::Zoom::next(self.zoom, draw::free_distance(world, &cam), now);
        self.zoom = Some(zoom);
        let own = view.me.filter(|_| mode == Camera::Follow).map(|me| me.id);
        let (camera, scene) = draw::shot(world, &avatars, &cam, zoom, own, aspect, &self.gaits);
        gpu.draw(&camera, &scene.draw());
    }

    fn fit_canvas(&mut self) {
        let Some(c) = &self.canvas else { return };
        let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio().min(MAX_DPR));
        let w = (f64::from(c.client_width()) * dpr).round().max(1.0) as u32;
        let h = (f64::from(c.client_height()) * dpr).round().max(1.0) as u32;
        if [w, h] != self.size {
            c.set_width(w);
            c.set_height(h);
            self.size = [w, h];
            if let Some(g) = &mut self.gpu {
                g.resize(w, h);
            }
        }
    }

    /// Tab: watch the next active player.
    pub fn cycle_camera(&mut self) {
        if let Some(r) = self.session.replica() {
            self.camera = self.camera.cycle(&r.view(now_ms()));
            self.zoom = None;
        }
    }

    pub fn configure(&self, config: LobbyConfig) {
        self.session.send(&game::Replica::configure(config));
    }

    pub fn start(&self) {
        self.session.send(&game::Replica::start());
    }
}

/// Copies app state into the chrome's signals. Call with no borrow held.
pub fn sync(sh: &Shared) {
    let now = now_ms();
    let mut p = sh.borrow_mut();
    p.toasts.retain(|t| t.until_ms > now);
    let ui = p.ui;
    let toasts = p.toasts.clone();
    let gpu = p.gpu_status.clone();
    let camera = p.camera;
    let (screen, lobby, hud, winner, camera) = match p.session.replica() {
        None => (Screen::Connecting, None, None, None, camera),
        Some(r) => {
            let view = r.view(now);
            let camera = camera.resolve(&view);
            let lobby = Lobby {
                is_host: view.host == Some(view.you),
                config: view.config,
                roster: view
                    .roster
                    .iter()
                    .map(|m| ui::roster_line(m, view.you, view.host))
                    .collect(),
                blocked: ui::start_blocked(view.config, view.roster).map(ui::refusal),
            };
            let winner = match view.phase {
                Phase::Results { winner, .. } => Some(ui::winner(winner)),
                _ => None,
            };
            (
                Screen::of(Some(&view)),
                Some(lobby),
                Some(ui::hud(&view, camera)),
                winner,
                camera,
            )
        }
    };
    p.camera = camera;
    drop(p);
    set(ui.screen, screen);
    set(ui.lobby, lobby);
    set(ui.hud, hud);
    set(ui.winner, winner);
    set(ui.toasts, toasts);
    set(ui.gpu, gpu);
}

/// Opens a socket to the room and keeps reopening it with backoff.
pub fn connect(sh: &Shared) {
    let url = crate::collab::url(&sh.borrow().session.room);
    let w: Weak<RefCell<Play>> = Rc::downgrade(sh);
    let (w1, w2, w3) = (w.clone(), w.clone(), w);
    let socket = Socket::open(
        &url,
        Rc::new(move || {
            if let Some(sh) = w1.upgrade() {
                sh.borrow().session.opened();
            }
        }),
        Rc::new(move |bytes: Vec<u8>| {
            if let Some(sh) = w2.upgrade() {
                let ok = sh.borrow_mut().on_frame(&bytes);
                if !ok {
                    lost(&sh);
                }
                sync(&sh);
            }
        }),
        Rc::new(move || {
            if let Some(sh) = w3.upgrade() {
                lost(&sh);
            }
        }),
    );
    match socket {
        Some(s) => sh.borrow_mut().session.attach(s),
        None => lost(sh),
    }
}

fn lost(sh: &Shared) {
    let wait = sh.borrow_mut().session.closed();
    sync(sh);
    let sh = sh.clone();
    set_timeout(move || connect(&sh), Duration::from_millis(u64::from(wait)));
}

/// Changes the name by reconnecting as the same player.
pub fn rename(sh: &Shared, name: String) {
    sh.borrow_mut().session.name = name;
    connect(sh);
}

/// Draws every animation frame for the life of the page.
pub fn animate(sh: Shared) {
    fn tick(sh: Shared) {
        request_animation_frame(move || {
            sh.borrow_mut().frame(now_ms());
            sync(&sh);
            tick(sh);
        });
    }
    tick(sh);
}

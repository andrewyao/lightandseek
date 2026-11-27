use std::cell::RefCell;
use std::rc::Rc;

use client::input::Dir;
use client::ui::{self, Camera, Count, Screen};
use game::{LobbyConfig, SceneKind};
use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent, MouseEvent};

use crate::collab::{self, Session};
use crate::debug;
use crate::play::{self, Play, Shared, Ui};

type Sv = StoredValue<Shared, LocalStorage>;

const NAME_KEY: &str = "lightseek.name";
/// Spectator orbit radians per pixel of drag.
const ORBIT_PER_PX: f32 = 0.006;

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn load_name() -> String {
    local_storage()
        .and_then(|s| s.get_item(NAME_KEY).ok().flatten())
        .unwrap_or_default()
}

fn save_name(name: &str) {
    if let Some(s) = local_storage() {
        let _ = s.set_item(NAME_KEY, name);
    }
}

fn typing_into(e: &KeyboardEvent) -> bool {
    e.target()
        .and_then(|t| t.dyn_into::<Element>().ok())
        .is_some_and(|el| matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA"))
}

fn pointer_locked() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some()
}

fn on_key(sv: Sv, e: &KeyboardEvent, down: bool) {
    if typing_into(e) {
        return;
    }
    if e.code() == "Tab" {
        e.prevent_default();
        if down {
            sv.with_value(|sh| sh.borrow_mut().cycle_camera());
        }
        return;
    }
    if let Some(d) = Dir::of_code(&e.code()) {
        e.prevent_default();
        sv.with_value(|sh| sh.borrow_mut().controls.key(d, down));
    }
}

/// Mouse travel turns you, or circles a spectator's camera. Pointer lock
/// reports travel without a button; otherwise a held button drags.
fn on_mouse(sv: Sv, e: &MouseEvent) {
    if !pointer_locked() && e.buttons() & 1 == 0 {
        return;
    }
    let (dx, dy) = (f64::from(e.movement_x()), f64::from(e.movement_y()));
    sv.with_value(|sh| {
        let mut p = sh.borrow_mut();
        match p.camera {
            Camera::Follow if !p.controls.scripted() => p.controls.turn(dx, dy),
            Camera::Follow => {}
            Camera::Spectate(_) => p.orbit_yaw -= dx as f32 * ORBIT_PER_PX,
        }
    });
}

#[component]
pub fn App() -> impl IntoView {
    let hash = web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .unwrap_or_default();
    let room = collab::room_from_hash(&hash);
    let name = load_name();
    let ui = Ui::new(name.clone());
    let sh: Shared = Rc::new(RefCell::new(Play::new(
        ui,
        Session::new(room.clone(), name),
    )));
    let sv: Sv = StoredValue::new_local(sh);
    let canvas = NodeRef::<html::Canvas>::new();
    let started = StoredValue::new(false);

    Effect::new(move |_| {
        let Some(el) = canvas.get() else { return };
        if started.get_value() {
            return;
        }
        started.set_value(true);
        let sh = sv.get_value();
        let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio().min(2.0));
        el.set_width((f64::from(el.client_width()) * dpr).max(1.0) as u32);
        el.set_height((f64::from(el.client_height()) * dpr).max(1.0) as u32);
        sh.borrow_mut().canvas = Some(el.clone());
        let gpu_sh = sh.clone();
        spawn_local(async move {
            let status = match engine::Gpu::from_canvas(el).await {
                Ok(gpu) => {
                    gpu_sh.borrow_mut().gpu = Some(gpu);
                    "ready".to_string()
                }
                Err(e) => format!("unavailable: {e}"),
            };
            gpu_sh.borrow_mut().gpu_status = status;
            play::sync(&gpu_sh);
        });
        play::connect(&sh);
        debug::install(&sh);
        play::animate(sh);
    });

    let keys = window_event_listener(ev::keydown, move |e| on_key(sv, &e, true));
    let keys_up = window_event_listener(ev::keyup, move |e| on_key(sv, &e, false));
    let blur = window_event_listener(ev::blur, move |_| {
        sv.with_value(|sh| sh.borrow_mut().controls.release_keys());
    });
    let hash_change = window_event_listener(ev::hashchange, move |_| {
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
    });
    on_cleanup(move || {
        keys.remove();
        keys_up.remove();
        blur.remove();
        hash_change.remove();
    });

    let lock = move |_: MouseEvent| {
        let follow = sv.with_value(|sh| sh.borrow().camera == Camera::Follow);
        if follow
            && ui.screen.get_untracked() == Screen::Game
            && let Some(c) = canvas.get_untracked()
        {
            c.request_pointer_lock();
        }
    };

    let room_label = room.clone();
    view! {
        <main class="app">
            <canvas
                node_ref=canvas
                class="stage"
                data-testid="stage"
                on:click=lock
                on:mousemove=move |e: MouseEvent| on_mouse(sv, &e)
                on:contextmenu=move |e: MouseEvent| e.prevent_default()
            ></canvas>
            {move || match ui.screen.get() {
                Screen::Connecting => {
                    view! {
                        <section class="card center" data-testid="connecting">
                            {format!("Connecting to #{room_label}…")}
                        </section>
                    }
                        .into_any()
                }
                Screen::Lobby => view! { <LobbyPanel sv ui room=room.clone()/> }.into_any(),
                Screen::Game => view! { <HudPanel ui/> }.into_any(),
                Screen::Results => view! { <ResultsPanel ui/> }.into_any(),
            }}
            <ul class="toasts" data-testid="toasts">
                {move || {
                    ui.toasts
                        .get()
                        .into_iter()
                        .map(|t| view! { <li>{t.text}</li> })
                        .collect_view()
                }}
            </ul>
            <footer class="status" data-testid="gpu-status">
                {move || format!("GPU {}", ui.gpu.get())}
            </footer>
        </main>
    }
}

#[component]
fn Stepper(
    label: &'static str,
    testid: &'static str,
    count: Count,
    config: Signal<LobbyConfig>,
    send: Callback<LobbyConfig>,
) -> impl IntoView {
    let value = move || match count {
        Count::Humans => config.get().humans,
        Count::Bots => config.get().bots,
    };
    let step = move |up: bool| ui::stepped(config.get(), count, up);
    view! {
        <div class="stepper">
            <span class="label">{label}</span>
            <button
                data-testid=format!("{testid}-minus")
                disabled=move || step(false).is_none()
                on:click=move |_| {
                    if let Some(c) = step(false) {
                        send.run(c);
                    }
                }
            >
                "−"
            </button>
            <output data-testid=testid>{value}</output>
            <button
                data-testid=format!("{testid}-plus")
                disabled=move || step(true).is_none()
                on:click=move |_| {
                    if let Some(c) = step(true) {
                        send.run(c);
                    }
                }
            >
                "+"
            </button>
        </div>
    }
}

fn scene_name(scene: Option<SceneKind>) -> &'static str {
    match scene {
        None => "Random",
        Some(SceneKind::Park) => "Park",
        Some(SceneKind::City) => "City",
        Some(SceneKind::Kitchen) => "Kitchen",
    }
}

const SCENES: [Option<SceneKind>; 4] = [
    None,
    Some(SceneKind::Park),
    Some(SceneKind::City),
    Some(SceneKind::Kitchen),
];

#[component]
fn LobbyPanel(sv: Sv, ui: Ui, room: String) -> impl IntoView {
    let server = Signal::derive(move || ui.lobby.get().map(|l| l.config).unwrap_or_default());
    // The host's latest edit, shown until the room echoes it back. Without it,
    // a second edit made before the first echo starts from the stale config
    // and silently undoes the first.
    let draft = RwSignal::new(None::<LobbyConfig>);
    Effect::new(move |_| {
        if draft.get_untracked() == Some(server.get()) {
            draft.set(None);
        }
    });
    let config = Signal::derive(move || draft.get().unwrap_or_else(|| server.get()));
    let is_host = move || ui.lobby.get().is_some_and(|l| l.is_host);
    let send = Callback::new(move |c: LobbyConfig| {
        draft.set(Some(c));
        sv.with_value(|sh| sh.borrow().configure(c));
    });
    let blocked = move || ui.lobby.get().and_then(|l| l.blocked);
    let number = |e: &ev::Event| event_target_value(e).trim().parse::<u64>().ok();
    let share = web_sys::window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();

    let host_controls = move || {
        view! {
            <div class="controls" data-testid="host-controls">
                <Stepper label="Humans" testid="humans" count=Count::Humans config send/>
                <Stepper label="Bots" testid="bots" count=Count::Bots config send/>
                <div class="row">
                    <span class="label">"Scene"</span>
                    {SCENES
                        .into_iter()
                        .map(|s| {
                            view! {
                                <button
                                    class:on=move || config.get().scene == s
                                    data-testid=format!("scene-{}", scene_name(s).to_lowercase())
                                    on:click=move |_| {
                                        send.run(LobbyConfig { scene: s, ..config.get() })
                                    }
                                >
                                    {scene_name(s)}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
                <label class="row">
                    <span class="label">"Seed"</span>
                    <input
                        data-testid="seed"
                        placeholder="random"
                        prop:value=move || config.get().seed.map(|s| s.to_string()).unwrap_or_default()
                        on:change=move |e| send.run(LobbyConfig { seed: number(&e), ..config.get() })
                    />
                </label>
                <label class="row">
                    <span class="label">"Hide (s)"</span>
                    <input
                        type="number"
                        min="1"
                        max="255"
                        data-testid="hide-secs"
                        prop:value=move || config.get().hide_secs.to_string()
                        on:change=move |e| {
                            if let Some(v) = number(&e) {
                                let hide_secs = v.clamp(1, 255) as u8;
                                send.run(LobbyConfig { hide_secs, ..config.get() });
                            }
                        }
                    />
                </label>
                <label class="row">
                    <span class="label">"Seek (s)"</span>
                    <input
                        type="number"
                        min="1"
                        max="3600"
                        data-testid="seek-secs"
                        prop:value=move || config.get().seek_secs.to_string()
                        on:change=move |e| {
                            if let Some(v) = number(&e) {
                                let seek_secs = v.clamp(1, 3600) as u16;
                                send.run(LobbyConfig { seek_secs, ..config.get() });
                            }
                        }
                    />
                </label>
                <button
                    class="start"
                    data-testid="start"
                    disabled=move || blocked().is_some()
                    title=move || blocked().unwrap_or_default()
                    on:click=move |_| sv.with_value(|sh| sh.borrow().start())
                >
                    "Start"
                </button>
                <p class="note">{move || blocked().unwrap_or_default()}</p>
            </div>
        }
    };
    let read_only = move || {
        let c = config.get();
        let seed = c.seed.map_or_else(|| "random".into(), |s| s.to_string());
        view! {
            <p class="readonly" data-testid="config">
                {format!(
                    "{} humans, {} bots · {} · hide {} s, seek {} s · seed {seed}",
                    c.humans,
                    c.bots,
                    scene_name(c.scene),
                    c.hide_secs,
                    c.seek_secs,
                )}
            </p>
            <p class="note">"Waiting for the host to start."</p>
        }
    };

    view! {
        <section class="card lobby" data-testid="lobby">
            <h1>"lightandseek"</h1>
            <p class="room">
                {format!("Room #{room}")}
                <span class="share">{format!("Share {share}")}</span>
            </p>
            <label class="row">
                <span class="label">"Your name"</span>
                <input
                    data-testid="name"
                    maxlength="24"
                    prop:value=move || ui.name.get()
                    on:change=move |e| {
                        let name = event_target_value(&e).trim().to_owned();
                        save_name(&name);
                        ui.name.set(name.clone());
                        play::rename(&sv.get_value(), name);
                    }
                />
            </label>
            <h2>"Players"</h2>
            <ul class="roster" data-testid="roster">
                {move || {
                    ui.lobby
                        .get()
                        .map(|l| l.roster)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|line| view! { <li>{line}</li> })
                        .collect_view()
                }}
            </ul>
            {move || if is_host() { host_controls().into_any() } else { read_only().into_any() }}
            <p class="refused" data-testid="refused">{move || ui.refused.get()}</p>
        </section>
    }
}

fn clock(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[component]
fn HudPanel(ui: Ui) -> impl IntoView {
    let hud = move || ui.hud.get();
    view! {
        {move || {
            hud()
                .map(|h| {
                    let seeker = h.role == "Seeker";
                    view! {
                        <div class="hud" data-testid="hud">
                            <span class="role" class:seeker=seeker data-testid="role">
                                {h.role.clone()}
                            </span>
                            <span data-testid="phase">{h.phase}</span>
                            <span class="clock" data-testid="clock">{clock(h.seconds)}</span>
                            {h
                                .hiders_left
                                .map(|n| {
                                    view! {
                                        <span data-testid="hiders-left">
                                            {format!("{n} hiders left")}
                                        </span>
                                    }
                                })}
                        </div>
                        {h
                            .watching
                            .clone()
                            .map(|name| {
                                view! {
                                    <div class="watching" data-testid="watching">
                                        {format!("Watching {name} · Tab to switch")}
                                    </div>
                                }
                            })}
                        {h
                            .blind
                            .then(|| {
                                view! {
                                    <div class="blind" data-testid="blind">
                                        <p>{format!("Hiding… {}", h.seconds)}</p>
                                        <p class="note">"Hiders are hiding. You can look soon."</p>
                                    </div>
                                }
                            })}
                        <p class="hint">"WASD or arrows to move · click to look with the mouse"</p>
                    }
                })
        }}
    }
}

#[component]
fn ResultsPanel(ui: Ui) -> impl IntoView {
    view! {
        <section class="card center results" data-testid="results">
            <h1 data-testid="winner">{move || ui.winner.get().unwrap_or_default()}</h1>
            <p>
                {move || {
                    ui.hud
                        .get()
                        .map(|h| format!("Next round in {} s", h.seconds))
                        .unwrap_or_default()
                }}
            </p>
            <ul class="roster">
                {move || {
                    ui.lobby
                        .get()
                        .map(|l| l.roster)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|line| view! { <li>{line}</li> })
                        .collect_view()
                }}
            </ul>
        </section>
    }
}

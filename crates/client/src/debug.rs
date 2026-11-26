//! `window.__lightseek`, for automated browser checks.
//!
//! - `state()` plain JSON, shaped by `client::probe`, computed at call time
//!   so a background tab answers with the room's latest frame.
//! - `input(fwd, strafe, yaw)` steers in place of the keyboard and mouse
//!   until `release()`. Values are as `game::Input` reads them.

use client::input::Script;
use client::probe;
use game::Vec2;
use js_sys::{JSON, Object, Reflect};
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

use crate::play::{Shared, now_ms};

fn state(sh: &Shared) -> JsValue {
    let p = sh.borrow();
    let view = p.session.replica().map(|r| r.view(now_ms()));
    let camera = view.as_ref().map_or(p.camera, |v| p.camera.resolve(v));
    let probe = probe::probe(view.as_ref(), camera, &p.gpu_status);
    let json = serde_json::to_string(&probe).unwrap_or_default();
    JSON::parse(&json).unwrap_or(JsValue::NULL)
}

fn set(target: &Object, name: &str, f: &JsValue) {
    let _ = Reflect::set(target, &JsValue::from_str(name), f);
}

pub fn install(sh: &Shared) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let hook = Object::new();

    let s = sh.clone();
    let c = Closure::<dyn Fn() -> JsValue>::new(move || state(&s));
    set(&hook, "state", c.as_ref());
    c.forget();

    let s = sh.clone();
    let c = Closure::<dyn Fn(f64, f64, f64)>::new(move |fwd: f64, strafe: f64, yaw: f64| {
        let mv = Vec2::new(fwd as f32, strafe as f32);
        s.borrow_mut().controls.script(Some(Script {
            mv,
            yaw: yaw as f32,
        }));
    });
    set(&hook, "input", c.as_ref());
    c.forget();

    let s = sh.clone();
    let c = Closure::<dyn Fn()>::new(move || s.borrow_mut().controls.script(None));
    set(&hook, "release", c.as_ref());
    c.forget();

    set(&window, "__lightseek", &hook);
}

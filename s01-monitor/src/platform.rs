//! The few things that differ between a computer and a browser: saving a
//! file, opening a file, and where the live feed comes from. On a computer
//! these use the system file dialogs. In a browser a save is a download and
//! an open is a file picker whose answer arrives a moment later, so an open
//! only asks, and the answer is collected with `take_opened`.

use std::path::PathBuf;
use std::sync::Mutex;

/// A file the person picked.
pub struct Opened {
    /// What it was asked for, so the right screen picks it up.
    pub tag: &'static str,
    pub name: String,
    pub bytes: Result<Vec<u8>, String>,
    /// The path on a computer. A browser has none.
    pub path: Option<PathBuf>,
}

static OPENED: Mutex<Vec<Opened>> = Mutex::new(Vec::new());

fn push(opened: Opened) {
    OPENED
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push(opened);
}

/// Files picked since the last call, oldest first.
pub fn take_opened() -> Vec<Opened> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(|poison| poison.into_inner()))
}

/// A recording a browser still holds from a page that closed or was killed
/// before it was saved. Found when the page starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Recoverable {
    pub id: String,
    pub name: String,
    /// About how many bytes it holds.
    pub bytes: u64,
}

static SAVED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static RECOVERABLE: Mutex<Vec<Recoverable>> = Mutex::new(Vec::new());

/// What the browser said about recordings saved since the last call, such as
/// "your downloads as recording-....log", oldest first.
pub fn take_saved() -> Vec<String> {
    std::mem::take(&mut *SAVED.lock().unwrap_or_else(|poison| poison.into_inner()))
}

/// The recordings left in this browser, as last asked for.
pub fn recoverable() -> Vec<Recoverable> {
    RECOVERABLE.lock().unwrap_or_else(|poison| poison.into_inner()).clone()
}

/// The browser's storage for a recording in progress, or `None` on a computer
/// (which writes a file) or in a browser without storage (which keeps it in
/// memory).
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub fn chunk_store() -> Option<Box<dyn crate::sink::ChunkStore>> {
    None
}

/// Builds the file from what the browser stored, plus `tail`, and downloads it.
/// The answer comes back through `take_saved`. A computer has nothing to do.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_chunks(_id: &str, _name: &str, _tail: Vec<u8>) {}

/// Asks the browser what recordings it still holds. The answer is read with
/// `recoverable`.
#[cfg(not(target_arch = "wasm32"))]
pub fn refresh_recoverable() {}

/// Downloads a recording left in the browser, or deletes it.
#[cfg(not(target_arch = "wasm32"))]
pub fn recover_save(_id: &str, _name: &str) {}
#[cfg(not(target_arch = "wasm32"))]
pub fn recover_discard(_id: &str) {}

/// Saves `bytes` as a file. Returns where it went, or `None` when the person
/// cancelled the dialog.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(name: &str, _mime: &str, label: &str, exts: &[&str], bytes: &[u8]) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new().set_file_name(name);
    if !exts.is_empty() {
        dialog = dialog.add_filter(label, exts);
    }
    let Some(mut path) = dialog.save_file() else {
        return Ok(None);
    };
    if path.extension().is_none() {
        if let Some(ext) = exts.first() {
            path.set_extension(ext);
        }
    }
    std::fs::write(&path, bytes).map_err(|err| format!("Could not save the file: {err}"))?;
    Ok(Some(path.display().to_string()))
}

/// Asks the person to pick a file. The answer comes back through `take_opened`.
#[cfg(not(target_arch = "wasm32"))]
pub fn open(tag: &'static str, label: &str, exts: &[&str]) {
    let Some(path) = rfd::FileDialog::new().add_filter(label, exts).pick_file() else {
        return;
    };
    let name = path
        .file_name()
        .map_or_else(|| "file".to_string(), |name| name.to_string_lossy().into_owned());
    let bytes = std::fs::read(&path).map_err(|err| format!("Could not read {name}: {err}"));
    push(Opened {
        tag,
        name,
        bytes,
        path: Some(path),
    });
}

/// Where the feed comes from when the page is served by the scooter's own
/// device. A computer has no such address.
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub fn device_url() -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
pub use web::{chunk_store, device_url, open, recover_discard, recover_save, refresh_recoverable, save, save_chunks};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{push, Opened, Recoverable, RECOVERABLE, SAVED};
    use crate::sink::ChunkStore;
    use eframe::wasm_bindgen::closure::Closure;
    use eframe::wasm_bindgen::{JsCast, JsValue};
    use js_sys::{Array, Function, Reflect};
    use std::time::Duration;

    pub fn save(name: &str, mime: &str, _label: &str, _exts: &[&str], bytes: &[u8]) -> Result<Option<String>, String> {
        let fail = |what: &str| format!("Could not start the download ({what})");
        let array = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::new();
        parts.push(&array);
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(mime);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(|_| fail("blob"))?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| fail("link"))?;
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| fail("page"))?;
        let anchor = document
            .create_element("a")
            .map_err(|_| fail("anchor"))?
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(|_| fail("anchor"))?;
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::sleep(Duration::from_secs(30)).await;
            let _ = web_sys::Url::revoke_object_url(&url);
        });
        Ok(Some(format!("your downloads as {name}")))
    }

    pub fn open(tag: &'static str, _label: &str, exts: &[&str]) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Ok(element) = document.create_element("input") else {
            return;
        };
        let Ok(input) = element.dyn_into::<web_sys::HtmlInputElement>() else {
            return;
        };
        input.set_type("file");
        let accept: Vec<String> = exts.iter().map(|ext| format!(".{ext}")).collect();
        input.set_accept(&accept.join(","));
        let reader = input.clone();
        let on_change = Closure::once_into_js(move |_event: web_sys::Event| {
            let Some(file) = reader.files().and_then(|files| files.get(0)) else {
                return;
            };
            let name = file.name();
            wasm_bindgen_futures::spawn_local(async move {
                let bytes = match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                    Ok(buffer) => Ok(js_sys::Uint8Array::new(&buffer).to_vec()),
                    Err(_) => Err(format!("Could not read {name}")),
                };
                push(Opened {
                    tag,
                    name,
                    bytes,
                    path: None,
                });
            });
        });
        input.set_onchange(Some(on_change.unchecked_ref()));
        input.click();
    }

    /// Calls `window.s01Rec.<name>(...)`, the recording storage in `web/index.html`.
    fn call(name: &str, args: &[JsValue]) -> Option<JsValue> {
        let window = web_sys::window()?;
        let store = Reflect::get(&window, &JsValue::from_str("s01Rec")).ok()?;
        if store.is_undefined() || store.is_null() {
            return None;
        }
        let function = Reflect::get(&store, &JsValue::from_str(name)).ok()?.dyn_into::<Function>().ok()?;
        let array = Array::new();
        for arg in args {
            array.push(arg);
        }
        function.apply(&store, &array).ok()
    }

    /// The page's IndexedDB storage. It holds no JavaScript values, only the
    /// names to call.
    #[derive(Debug)]
    struct PageStore;

    impl ChunkStore for PageStore {
        fn begin(&mut self, id: &str, name: &str) {
            let _ = call("begin", &[JsValue::from_str(id), JsValue::from_str(name)]);
        }

        fn append(&mut self, id: &str, seq: u32, text: &str) {
            let _ = call("append", &[JsValue::from_str(id), JsValue::from_f64(f64::from(seq)), JsValue::from_str(text)]);
        }

        fn status(&self, id: &str) -> Option<String> {
            call("status", &[JsValue::from_str(id)])?.as_string().filter(|text| !text.is_empty())
        }
    }

    pub fn chunk_store() -> Option<Box<dyn ChunkStore>> {
        if call("available", &[])?.as_bool()? {
            Some(Box::new(PageStore))
        } else {
            None
        }
    }

    fn say(message: String) {
        SAVED.lock().unwrap_or_else(|poison| poison.into_inner()).push(message);
    }

    /// Asks the page to build the recording and download it. It answers in a
    /// moment, through `take_saved`.
    fn download(id: String, name: String, tail: String) {
        wasm_bindgen_futures::spawn_local(async move {
            let promise = call("save", &[JsValue::from_str(&id), JsValue::from_str(&name), JsValue::from_str(&tail)]);
            let message = match promise {
                Some(promise) => match wasm_bindgen_futures::JsFuture::from(promise.unchecked_into::<js_sys::Promise>()).await {
                    Ok(answer) => answer.as_string().unwrap_or_default(),
                    Err(_) => "nowhere: the browser could not build the file".to_string(),
                },
                None => "nowhere: the browser's recording storage is not there".to_string(),
            };
            say(message);
            refresh_recoverable();
        });
    }

    pub fn save_chunks(id: &str, name: &str, tail: Vec<u8>) {
        download(id.to_string(), name.to_string(), String::from_utf8_lossy(&tail).into_owned());
    }

    pub fn recover_save(id: &str, name: &str) {
        download(id.to_string(), name.to_string(), String::new());
    }

    pub fn recover_discard(id: &str) {
        let id = id.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(promise) = call("discard", &[JsValue::from_str(&id)]) {
                let _ = wasm_bindgen_futures::JsFuture::from(promise.unchecked_into::<js_sys::Promise>()).await;
            }
            refresh_recoverable();
        });
    }

    pub fn refresh_recoverable() {
        wasm_bindgen_futures::spawn_local(async move {
            let Some(promise) = call("recoverable", &[]) else {
                return;
            };
            let Ok(list) = wasm_bindgen_futures::JsFuture::from(promise.unchecked_into::<js_sys::Promise>()).await else {
                return;
            };
            let mut found = Vec::new();
            for item in Array::from(&list).iter() {
                let text = |key: &str| Reflect::get(&item, &JsValue::from_str(key)).ok();
                let (Some(id), Some(name)) = (text("id").and_then(|v| v.as_string()), text("name").and_then(|v| v.as_string())) else {
                    continue;
                };
                let bytes = text("chars").and_then(|v| v.as_f64()).unwrap_or(0.0).max(0.0) as u64;
                found.push(Recoverable { id, name, bytes });
            }
            *RECOVERABLE.lock().unwrap_or_else(|poison| poison.into_inner()) = found;
        });
    }

    /// `ws://host/ws` for the host that served this page.
    pub fn device_url() -> Option<String> {
        let location = web_sys::window()?.location();
        let host = location.host().ok()?;
        if host.is_empty() {
            return None;
        }
        let scheme = if location.protocol().ok().as_deref() == Some("https:") { "wss" } else { "ws" };
        Some(format!("{scheme}://{host}/ws"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_files_come_back_once_in_order() {
        push(Opened { tag: "a", name: "one".into(), bytes: Ok(vec![1]), path: None });
        push(Opened { tag: "b", name: "two".into(), bytes: Err("no".into()), path: None });
        let got = take_opened();
        assert!(got.len() >= 2);
        let mine: Vec<&str> = got.iter().filter(|o| o.name == "one" || o.name == "two").map(|o| o.name.as_str()).collect();
        assert_eq!(mine, ["one", "two"]);
        assert!(take_opened().is_empty());
    }
}

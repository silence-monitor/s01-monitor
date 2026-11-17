#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod chart_image;
mod clock;
mod compare;
mod decode;
mod explain;
mod export;
mod formats;
mod frames;
mod paramdiff;
mod params;
mod platform;
mod png;
mod readings;
mod sim;
mod signals;
mod sink;
mod slcan;
mod snapshot;
mod source;
#[cfg(target_arch = "wasm32")]
mod socket;
mod stored;
mod views;


#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 860.0])
            .with_min_inner_size([360.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Silence S01",
        native_options,
        Box::new(|cc| Ok(Box::new(app::MonitorApp::new(cc)))),
    )
}

/// In a browser the page has a canvas called `the_canvas_id`; the app draws
/// into it. The page's script starts this through the generated module.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;
    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("the_canvas_id"))
            .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("the page has no canvas called the_canvas_id");
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(app::MonitorApp::new(cc)))),
            )
            .await;
        if let Err(err) = result {
            if let Some(status) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id("status"))
            {
                status.set_text_content(Some(&format!("The app could not start: {err:?}")));
            }
        }
    });
}

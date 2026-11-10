//! Envoy — a Windows environment variable manager.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialogs;
mod model;
mod store;
mod theme;
mod transfer;
mod validate;
mod views;
mod widgets;
mod winenv;

use app::{App, StartArgs};

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Elevated helper mode: write the system changes handed over by the normal instance, then exit.
    if args.first().map(String::as_str) == Some("--apply-elevated") {
        let code = match args.get(1) {
            Some(file) => winenv::elevated_apply_main(std::path::Path::new(file)),
            None => 2,
        };
        std::process::exit(code);
    }

    let mut start = StartArgs::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--session" => start.session = it.next().map(Into::into),
            "--from-user" => start.from_user = it.next(),
            _ => {}
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Envoy — Environment Variables")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([1040.0, 620.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Envoy",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            theme::install_fonts(&cc.egui_ctx);
            theme::install_icons(&cc.egui_ctx);
            Ok(Box::new(App::new(&cc.egui_ctx, start)))
        }),
    )
}

/// The accent square from the design's title bar, on the ink colour.
fn app_icon() -> egui::IconData {
    const N: usize = 64;
    let mut rgba = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let inner = (16..48).contains(&x) && (16..48).contains(&y);
            rgba.extend_from_slice(if inner { &[0xec, 0x30, 0x13, 0xff] } else { &[0x20, 0x1e, 0x1d, 0xff] });
        }
    }
    egui::IconData { rgba, width: N as u32, height: N as u32 }
}

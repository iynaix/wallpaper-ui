#![allow(non_snake_case)]
use app_state::PreviewMode;
use clap::Parser;
use components::{editor::handle_arrow_keys_keyup, save_button::save_image};
use dioxus::desktop::Config;
use dioxus::prelude::*;
use wallpaper_ui::config::WallpaperConfig;

pub mod app_state;
pub mod cli;
pub mod components;

use crate::{
    app_state::{UiMode, UiState, Wallpapers},
    components::{
        app_header::AppHeader,
        editor::{handle_editor_shortcuts, Editor},
        filelist::FileList,
        palette::Palette,
    },
};

fn main() {
    let args = cli::WallpaperUIArgs::parse();
    if args.version {
        println!("wallpaper-ui {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    // use a custom index.html to set the height of body to the full height of the window
    LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_background_color((30, 30, 46, 255))
                .with_menu(None)
                // disable on release builds
                .with_disable_context_menu(!cfg!(debug_assertions))
                .with_custom_index(
                    r#"<!DOCTYPE html>
<html>
    <head>
        <title>Dioxus app</title>
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <link rel="stylesheet" href="public/tailwind.css">
    </head>
    <body>
        <div id="main" style="height: 100vh;"></div>
    </body>
</html>"#
                        .to_string(),
                ),
        )
        .launch(App);
}

fn handle_shortcuts(
    evt: &Event<KeyboardData>,
    wallpapers: &mut Signal<Wallpapers>,
    ui: &mut Signal<UiState>,
) {
    match evt.key() {
        Key::Character(shortcut) => {
            let shortcut = shortcut.as_str();

            match shortcut {
                "/" => {
                    ui.with_mut(app_state::UiState::toggle_filelist);
                }

                // ctrl+f
                "f" => {
                    if evt.modifiers().ctrl() {
                        ui.with_mut(app_state::UiState::toggle_filelist);
                    }
                }

                // ctrl+s
                "s" => {
                    if evt.modifiers().ctrl() && !wallpapers().files.is_empty() {
                        save_image();
                    }
                }

                // palette
                "p" => {
                    if evt.modifiers().ctrl() && !wallpapers().files.is_empty() {
                        ui.with_mut(app_state::UiState::toggle_palette);
                    }
                }
                _ => {
                    if ui().mode == UiMode::Editor {
                        handle_editor_shortcuts(evt, wallpapers, ui);
                    }
                }
            }
        }
        _ => {
            if ui().mode == UiMode::Editor {
                handle_editor_shortcuts(evt, wallpapers, ui);
            }
        }
    };
}

// define a component that renders a div with the text "Hello, world!"
fn App() -> Element {
    let config = use_context_provider(|| Signal::new(WallpaperConfig::new()));
    let mut wallpapers = use_context_provider(|| Signal::new(Wallpapers::from_args(&config())));
    let mut ui = use_context_provider(|| {
        let walls = wallpapers();
        let has_multiple_candidates =
            walls.current.cropper().crop_candidates(&walls.ratio).len() > 1;

        Signal::new(UiState {
            show_faces: config().show_faces,
            preview_mode: if has_multiple_candidates {
                PreviewMode::Candidate(None)
            } else {
                PreviewMode::Pan
            },
            ..UiState::default()
        })
    });

    let has_files = !wallpapers().files.is_empty();

    if !has_files {
        return rsx! {
            main {
                class: "dark flex items-center h-full justify-center bg-base overflow-hidden",
                div {
                    h1 { class: "mt-4 text-4xl font-bold tracking-tight text-text text-center h-full",
                        "No more wallpapers to process! 🎉"
                    }
                }
            }
        };
    }

    rsx! {
        main {
            class: "dark flex flex-col h-full bg-base overflow-hidden",
            tabindex: 0,
            autofocus: true,
            onkeydown: move |evt| {
                handle_shortcuts(&evt, &mut wallpapers, &mut ui);
            },
            onkeyup: move |evt| {
                handle_arrow_keys_keyup(&evt.key(), &mut ui);
            },

            AppHeader { }

            div {
                class: "flex p-4 gap-4",

                if ui().mode == UiMode::FileList {
                    FileList { }
                } else if ui().mode == UiMode::Palette {
                    Palette { }
                } else if ui().mode == UiMode::Editor {
                    Editor { wallpapers_path: config().wallpapers_dir }
                }
            }
        }
    }
}

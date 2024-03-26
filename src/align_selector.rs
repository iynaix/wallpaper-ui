#![allow(non_snake_case)]
use dioxus::prelude::*;
use dioxus_free_icons::icons::{
    md_action_icons::MdPanTool,
    md_editor_icons::{
        MdFormatAlignCenter, MdFormatAlignLeft, MdFormatAlignRight, MdVerticalAlignBottom,
        MdVerticalAlignCenter, MdVerticalAlignTop,
    },
};
use dioxus_free_icons::Icon;
use wallpaper_ui::{cropper::Direction, geometry::Geometry};

use crate::{
    app_state::{PreviewMode, UiState, Wallpapers},
    buttons::Button,
};

#[component]
fn AlignButton(
    class: String,
    geom: Geometry,
    ui: Signal<UiState>,
    wallpapers: Signal<Wallpapers>,
    children: Element,
) -> Element {
    let current_geom = (wallpapers)().get_geometry();

    rsx! {
        Button {
            class,
            active: current_geom == geom,
            onclick: move |_| {
                wallpapers.with_mut(|wallpapers| {
                    wallpapers.set_geometry(&geom);
                });
                ui.with_mut(|ui| {
                    ui.preview_mode = PreviewMode::Candidate(None);
                });
            },
            {children}
        }
    }
}

#[component]
pub fn AlignSelector(
    class: Option<String>,
    wallpapers: Signal<Wallpapers>,
    ui: Signal<UiState>,
) -> Element {
    let info = wallpapers().current;
    let ratio = wallpapers().ratio;
    let align = ui().preview_mode;
    let geom: Geometry = wallpapers().get_geometry();
    let (img_w, img_h) = info.image_dimensions();
    let dir = info.direction(&geom);

    rsx! {
        div { class: "flex gap-x-6",
            span {
                class: "isolate inline-flex rounded-md shadow-sm",
                AlignButton {
                    class: "text-sm rounded-l-md",
                    geom: wallpapers().source.get_geometry(&ratio),
                    wallpapers,
                    ui,
                    "Source"
                }
                AlignButton {
                    class: "text-sm rounded-r-md",
                    geom: info.cropper().crop(&ratio),
                    wallpapers,
                    ui,
                    "Default"
                }
            }

            span {
                class: "isolate inline-flex rounded-md shadow-sm",
                class: class.unwrap_or_default(),
                AlignButton {
                    class: "text-sm rounded-l-md",
                    geom: geom.align_start(img_w, img_h),
                    wallpapers,
                    ui,
                    if dir == Direction::X {
                        Icon { fill: "white", icon:  MdFormatAlignLeft }
                    } else {
                        Icon { fill: "white", icon: MdVerticalAlignTop }
                    }
                }
                AlignButton {
                    class: "text-sm -ml-px",
                    geom: geom.align_center(img_w, img_h),
                    wallpapers,
                    ui,
                    if dir == Direction::X {
                        Icon { fill: "white", icon:  MdFormatAlignCenter }
                    } else {
                        Icon { fill: "white", icon: MdVerticalAlignCenter }
                    }
               }
                AlignButton {
                    class: "text-sm rounded-r-md",
                    geom: geom.align_end(img_w, img_h),
                    wallpapers,
                    ui,
                    if dir == Direction::X {
                        Icon { fill: "white", icon:  MdFormatAlignRight }
                    } else {
                        Icon { fill: "white", icon: MdVerticalAlignBottom }
                    }
                }
            }

            span {
                class: "isolate inline-flex rounded-md shadow-sm",
                Button {
                    class: "text-sm rounded-md",
                    active: align == PreviewMode::Manual,
                    onclick: move |_| {
                        ui.with_mut(|ui| {
                            ui.preview_mode = if matches!(&ui.preview_mode, PreviewMode::Manual) {
                                PreviewMode::Candidate(None)
                            } else {
                                PreviewMode::Manual
                            }
                        });
                    },
                    Icon { fill: "white", icon: MdPanTool }
                }
            }
        }
    }
}

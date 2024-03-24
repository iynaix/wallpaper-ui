#![allow(non_snake_case)]
use std::path::PathBuf;

use dioxus::prelude::*;
use wallpaper_ui::{
    filename,
    wallpapers::{WallInfo, WallpapersCsv},
};

use crate::switch::Switch;

fn prev_wall(wall_files: &[PathBuf], info_path: &PathBuf) -> Option<WallInfo> {
    let pos = wall_files
        .iter()
        .position(|f| *f == *info_path)
        .expect("could not index current wallpaper");
    let prev_wall = if pos == 0 {
        wall_files.last()
    } else {
        wall_files.get(pos - 1)
    };

    let wallpapers_csv = WallpapersCsv::new();

    prev_wall
        .and_then(|prev_wall| wallpapers_csv.get(&filename(prev_wall)))
        .cloned()
}

fn next_wall(wall_files: &[PathBuf], info_path: &PathBuf) -> Option<WallInfo> {
    let pos = wall_files
        .iter()
        .position(|f| *f == *info_path)
        .expect("could not index current wallpaper");

    let next_wall = if pos == wall_files.len() - 1 {
        wall_files.first()
    } else {
        wall_files.get(pos + 1)
    };

    let wallpapers_csv = WallpapersCsv::new();

    next_wall
        .and_then(|next_wall| wallpapers_csv.get(&filename(next_wall)))
        .cloned()
}

#[derive(Clone, PartialEq, Props)]
pub struct AppHeaderProps {
    show_faces: Signal<bool>,
    show_filelist: Signal<bool>,
    wall_info: Signal<WallInfo>,
    wallpaper_files: Signal<Vec<PathBuf>>,
}

#[allow(clippy::too_many_lines)]
pub fn AppHeader(mut props: AppHeaderProps) -> Element {
    let info = (props.wall_info)();

    let pagination_cls = "relative inline-flex items-center rounded-md bg-surface1 py-1 px-2 text-sm font-semibold text-text ring-1 ring-inset ring-surface2 hover:bg-oveylay0 focus-visible:outline-offset-0 cursor-pointer";

    rsx! {
        header { class: "bg-surface0",
            nav {
                "aria-label": "Global",
                class: "mx-auto flex max-w-full items-center justify-between py-6 px-4",
                div { class: "flex gap-x-4 items-center",
                    a { class: pagination_cls,
                        onclick: {
                            let prev_wall_info = prev_wall(&(props.wallpaper_files)(), &info.path()).expect(
                                "could not get previous wallpaper info");
                            move |_| {
                                props.wall_info.set(prev_wall_info.clone());
                            }
                        },
                        "<"
                    }
                    a { class: "text-sm font-semibold leading-6 text-white",
                        onclick: move |_| {
                            props.show_filelist.set(!(props.show_filelist)());
                        },
                        {info.filename.clone()}
                    }
                    a { class: pagination_cls,
                        onclick: {
                            let next_wall_info = next_wall(&(props.wallpaper_files)(), &info.path()).expect(
                                "could not get next wallpaper info");
                            move |_| {
                                props.wall_info.set(next_wall_info.clone());
                            }
                        },
                        ">"
                    }
                    // done checkbox
                    div { class: "relative flex items-start",
                        div { class: "flex h-6 items-center",
                            input {
                                r#type: "checkbox",
                                name: "comments",
                                "aria-describedby": "comments-description",
                                class: "h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-600",
                                id: "comments"
                            }
                        }
                        div { class: "ml-3 text-sm leading-6",
                            label { r#for: "comments", class: "font-medium text-gray-900", "New comments" }
                            span { class: "text-white", id: "comments-description",
                                span { class: "sr-only", "New comments " }
                                "so you always know what's happening."
                            }
                        }
                    }
                }
                div { class: "gap-8 flex flex-1 justify-end",
                    Switch {
                        label: rsx! {
                            span {
                                class: "ml-3 text-md",
                                span {
                                    class: "font-medium text-white",
                                    "Faces ({info.faces.len()})"
                                }
                            }
                        },
                        checked: props.show_faces,
                    },

                    a {
                        class: "rounded-md bg-indigo-600 px-3 py-2 text-sm font-semibold text-white shadow-sm hover:bg-indigo-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-600 cursor-pointer",
                        onclick: {
                            move |_| {
                                let mut wallpapers_csv = WallpapersCsv::new();
                                wallpapers_csv.insert(info.filename.clone(), info.clone());
                                wallpapers_csv.save();
                            }
                        },
                        "Save"
                    }
                }
            }
        }
    }
}

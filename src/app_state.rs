use clap::Parser;
use itertools::Itertools;
use std::path::PathBuf;

use wallpaper_ui::{
    aspect_ratio::AspectRatio,
    cli::WallpaperUIArgs,
    config::WallpaperConfig,
    cropper::Direction,
    filename, filter_images,
    geometry::Geometry,
    is_image,
    wallpapers::{WallInfo, WallpapersCsv},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiMode {
    Editor,
    FileList,
    Palette,
}

impl Default for UiMode {
    fn default() -> Self {
        Self::Editor
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UiState {
    pub mode: UiMode,
    pub preview_mode: PreviewMode,
    pub show_faces: bool,
    pub is_saving: bool,
    pub arrow_key_start: Option<std::time::Instant>,
}

impl UiState {
    pub fn toggle_filelist(&mut self) {
        self.mode = match self.mode {
            UiMode::FileList => UiMode::Editor,
            _ => UiMode::FileList,
        };
    }

    pub fn toggle_palette(&mut self) {
        self.mode = match self.mode {
            UiMode::Palette => UiMode::Editor,
            _ => UiMode::Palette,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewMode {
    Pan,
    /// stores the last mouseover geometry
    Candidate(Option<Geometry>),
}

impl Default for PreviewMode {
    fn default() -> Self {
        Self::Candidate(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wallpapers {
    pub files: Vec<PathBuf>,
    // the original wallinfo before any modifications
    pub source: WallInfo,
    pub current: WallInfo,
    pub index: usize,
    pub ratio: AspectRatio,
    pub resolutions: Vec<(String, AspectRatio)>,
}

impl Wallpapers {
    /// parse an optional comma separated list of resolutions
    fn resolution_arg(
        resolution_arg: Option<&str>,
        resolutions: &[AspectRatio],
    ) -> Vec<AspectRatio> {
        match resolution_arg {
            None => Vec::new(),
            Some("all") => resolutions.to_vec(),
            Some(res_arg) => res_arg
                .split(',')
                .map(|s| {
                    std::convert::TryInto::<AspectRatio>::try_into(s.trim())
                        .unwrap_or_else(|()| panic!("Invalid resolution {s} provided."))
                })
                .collect(),
        }
    }

    pub fn from_args(wall_dir: &PathBuf) -> Self {
        let args = WallpaperUIArgs::parse();
        let resolution_pairs = WallpaperConfig::new().resolutions;
        let resolutions: Vec<_> = resolution_pairs.iter().map(|(_, r)| r.clone()).collect();

        let mut modified_filters = Self::resolution_arg(args.modified.as_deref(), &resolutions);
        if !modified_filters.is_empty() {
            modified_filters = resolutions
                .iter()
                .filter(|r| !modified_filters.contains(r))
                .cloned()
                .collect();
        }

        let unmodified_filters = Self::resolution_arg(args.unmodified.as_deref(), &resolutions);

        let mut all_files = Vec::new();
        if let Some(paths) = args.paths {
            paths.iter().flat_map(std::fs::canonicalize).for_each(|p| {
                if p.is_file() {
                    if let Some(p) = is_image(&p) {
                        all_files.push(p);
                    }
                } else {
                    all_files.extend(filter_images(&p));
                }
            });
        }

        if all_files.is_empty() {
            // defaults to wallpaper directory
            if !wall_dir.exists() {
                eprintln!("Wallpaper directory does not exist: {:?}", wall_dir);
                std::process::exit(1);
            }

            all_files.extend(filter_images(&wall_dir));
        }

        let wallpapers_csv = WallpapersCsv::load();

        // filter only wallpapers that still use the default crops if needed
        all_files.retain(|f| {
            let fname = filename(f);
            if let Some(info) = wallpapers_csv.get(&fname) {
                if args.filter.is_some()
                    && !fname.to_lowercase().contains(
                        &args
                            .filter
                            .as_ref()
                            .expect("no --filter provided")
                            .to_lowercase(),
                    )
                {
                    return false;
                }

                // check if wallpaper uses default crop for a resolution / all resolutions
                if !modified_filters.is_empty() {
                    return info.is_default_crops(&modified_filters);
                }

                if !unmodified_filters.is_empty() {
                    return info.is_default_crops(&unmodified_filters);
                }

                return match args.faces.as_str() {
                    "all" => true,
                    "zero" | "none" => info.faces.is_empty(),
                    "one" | "single" => info.faces.len() == 1,
                    "many" | "multiple" => info.faces.len() > 1,
                    _ => panic!("Invalid faces : {}", args.faces),
                };
            }
            true
        });

        // order by reverse chronological order
        all_files.sort_by_key(|f| {
            f.metadata()
                .unwrap_or_else(|_| panic!("could not get file metadata: {:?}", f))
                .modified()
                .unwrap_or_else(|_| panic!("could not get file mtime: {:?}", f))
        });
        all_files.reverse();

        let fname = filename(
            all_files
                .first()
                .unwrap_or_else(|| panic!("no wallpapers found")),
        );
        let loaded = wallpapers_csv
            .get(&fname)
            .unwrap_or_else(|| panic!("could not get wallpaper info for {fname}"));

        Self {
            index: Default::default(),
            files: all_files,
            source: loaded.clone(),
            current: loaded.clone(),
            ratio: resolutions[0].clone(),
            resolutions: resolution_pairs,
        }
    }

    pub fn prev_wall(&mut self) {
        // loop back to the last wallpaper
        self.index = if self.index == 0 {
            self.files.len() - 1
        } else {
            self.index - 1
        };

        let wallpapers_csv = WallpapersCsv::load();
        let fname = filename(&self.files[self.index]);
        let loaded = wallpapers_csv
            // bounds check is not necessary since the index is always valid
            .get(&fname)
            .unwrap_or_else(|| panic!("could not get wallpaper info for {fname}"));
        self.source = loaded.clone();
        self.current = loaded.clone();
    }

    pub fn next_wall(&mut self) {
        // loop back to the first wallpaper
        self.index = if self.index == self.files.len() - 1 {
            0
        } else {
            self.index + 1
        };

        let wallpapers_csv = WallpapersCsv::load();
        let fname = filename(&self.files[self.index]);
        let loaded = wallpapers_csv
            // bounds check is not necessary since the index is always valid
            .get(&filename(&self.files[self.index]))
            .unwrap_or_else(|| panic!("could not get wallpaper info for {fname}"));
        self.source = loaded.clone();
        self.current = loaded.clone();
    }

    /// removes the current wallpaper from the list
    pub fn remove(&mut self) {
        let current_index = self.index;
        self.next_wall();
        self.files.remove(current_index);
        // current_index is unchanged after removal
        self.index = current_index;
    }

    pub fn set_from_filename(&mut self, fname: &str) {
        let wallpapers_csv = WallpapersCsv::load();
        let loaded = wallpapers_csv
            .get(fname)
            .unwrap_or_else(|| panic!("could not get wallpaper info for {fname}"))
            .clone();
        self.source = loaded.clone();
        self.current = loaded;
        self.index = self
            .files
            .iter()
            .position(|f| filename(f) == fname)
            .unwrap_or_else(|| panic!("could not find wallpaper: {}", fname));
    }

    /// gets geometry for current aspect ratio
    pub fn get_geometry(&self) -> Geometry {
        self.current.get_geometry(&self.ratio)
    }

    /// sets the geometry for current aspect ratio
    pub fn set_geometry(&mut self, geom: &Geometry) {
        self.current.set_geometry(&self.ratio, geom);
    }

    /// returns crop candidates for current ratio and image
    pub fn crop_candidates(&self) -> Vec<Geometry> {
        self.current.cropper().crop_candidates(&self.ratio)
    }

    /// returns cropping ratios for resolution buttons
    pub fn image_ratios(&self) -> Vec<(String, AspectRatio)> {
        self.resolutions
            .clone()
            .into_iter()
            .filter(|(_, ratio)| {
                // do not show resolution if aspect ratio of image is the same,
                // as there is only a single possible crop
                (f64::from(self.current.width) / f64::from(self.current.height) - f64::from(ratio))
                    .abs()
                    > f64::EPSILON
            })
            .collect()
    }

    /// returns the candidate geometries for candidate buttons
    pub fn candidate_geometries(&self) -> Vec<Geometry> {
        self.crop_candidates().into_iter().unique().collect()
    }

    /// moves the crop area of the current wallpaper based on its direction
    pub fn move_geometry_by(&self, delta: i32) -> Geometry {
        let current_geom = self.get_geometry();

        let negative_delta = delta < 0;
        let delta = delta.unsigned_abs();

        match self.current.direction(&current_geom) {
            Direction::X => Geometry {
                x: if negative_delta {
                    current_geom.x.max(delta) - delta
                } else {
                    (current_geom.x + delta).min(self.current.width - current_geom.w)
                },
                ..current_geom
            },
            Direction::Y => Geometry {
                y: if negative_delta {
                    current_geom.y.max(delta) - delta
                } else {
                    (current_geom.y + delta).min(self.current.height - current_geom.h)
                },
                ..current_geom
            },
        }
    }
}

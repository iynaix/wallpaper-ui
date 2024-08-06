use clap::Parser;
use itertools::Itertools;
use std::path::PathBuf;

use wallpaper_ui::{
    aspect_ratio::AspectRatio,
    cli::FacesFilter,
    cli::WallpaperUIArgs,
    config::WallpaperConfig,
    cropper::Direction,
    filename, filter_images,
    geometry::Geometry,
    is_image,
    wallpapers::{WallInfo, WallpapersCsv},
};

use crate::components::use_wallpapers;

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
    pub is_applying_wallpaper: bool,
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

    /// switch to pan mode if there are multiple candidates
    pub fn init_preview_mode(&mut self) {
        let walls = use_wallpapers()();
        let has_multiple_candidates =
            walls.current.cropper().crop_candidates(&walls.ratio).len() > 1;

        self.preview_mode = if has_multiple_candidates {
            PreviewMode::Candidate(None)
        } else {
            PreviewMode::Pan
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
    csv: WallpapersCsv,
    // the original wallinfo before any modifications
    pub source: WallInfo,
    pub current: WallInfo,
    pub index: usize,
    pub ratio: AspectRatio,
    pub resolutions: Vec<(String, AspectRatio)>,
    wall_dir: PathBuf,
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

    pub fn from_args(cfg: &WallpaperConfig) -> Self {
        let args = WallpaperUIArgs::parse();
        let wall_dir = &cfg.wallpapers_dir;
        let resolution_pairs = &cfg.resolutions;
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

        let mut all_files: Vec<PathBuf> = Vec::new();
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

        let wallpapers_csv = WallpapersCsv::load(cfg);

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

                return match args.faces {
                    FacesFilter::All => true,
                    FacesFilter::Zero | FacesFilter::None => info.faces.is_empty(),
                    FacesFilter::One | FacesFilter::Single => info.faces.len() == 1,
                    FacesFilter::Many | FacesFilter::Multiple => info.faces.len() > 1,
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
            csv: wallpapers_csv.clone(),
            source: loaded.clone(),
            current: loaded.clone(),
            ratio: resolutions[0].clone(),
            resolutions: resolution_pairs.clone(),
            wall_dir: wall_dir.clone(),
        }
    }

    #[cfg_attr(test, allow(unused_variables))]
    fn load_from_csv(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let fname = filename(&self.files[self.index]);
        #[cfg(not(test))]
        {
            let loaded = self
                .csv
                .get(&fname)
                .unwrap_or_else(|| panic!("could not get wallpaper info for {fname}"));
            self.source = loaded.clone();
            self.current = loaded.clone();
        }

        #[cfg(test)]
        {
            let loaded = WallInfo {
                filename: fname,
                ..WallInfo::default()
            };
            self.source = loaded.clone();
            self.current = loaded;
        }
    }

    pub fn prev_wall(&mut self) {
        // loop back to the last wallpaper
        self.index = if self.index == 0 {
            self.files.len() - 1
        } else {
            self.index - 1
        };

        self.load_from_csv();
    }

    pub fn next_wall(&mut self) {
        // loop back to the first wallpaper
        self.index = if self.index == self.files.len() - 1 {
            0
        } else {
            self.index + 1
        };

        self.load_from_csv();
    }

    /// removes the current wallpaper from the list
    pub fn remove(&mut self) {
        let current_file = self.files[self.index].clone();
        if self.index == self.files.len() - 1 {
            self.files.remove(self.index);
            self.index = 0;
        } else {
            self.files.remove(self.index);
        }

        self.files.retain(|f| f != &current_file);
        self.load_from_csv();
    }

    pub fn set_from_filename(&mut self, fname: &str) {
        let loaded = self
            .csv
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

    pub fn has_candidates(&self) -> bool {
        let cropper = self.current.cropper();
        self.image_ratios()
            .iter()
            .any(|(_, ratio)| cropper.crop_candidates(ratio).len() > 1)
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

    // returns the full path of the current wallpaper
    pub fn full_path(&self) -> String {
        self.wall_dir
            .join(&self.current.filename)
            .to_str()
            .expect("could not convert full image path to string")
            .to_string()
    }

    // inserts a wallinfo into the csv
    pub fn insert_csv(&mut self, info: &WallInfo) {
        self.csv.insert(info.filename.to_string(), info.clone());
    }

    /// saves the csv
    pub fn save_csv(&self) {
        let resolutions: Vec<_> = self
            .resolutions
            .iter()
            .map(|(_, ratio)| ratio.clone())
            .collect();

        self.csv.save(&resolutions);
    }
}

#[cfg(test)]
impl Wallpapers {
    pub fn create_mock(len: usize, index: usize) -> Self {
        assert!(index < len, "index must be less than len");

        let mut files = Vec::new();
        for i in 0..len {
            files.push(PathBuf::from(format!("{}", i)));
        }

        let current = WallInfo {
            filename: "0".to_string(),
            ..WallInfo::default()
        };

        Self {
            files,
            csv: WallpapersCsv::default(),
            index,
            source: current.clone(),
            current,
            ratio: AspectRatio { w: 16, h: 9 },
            resolutions: Vec::new(),
            wall_dir: PathBuf::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_wall() {
        static LEN: usize = 5;
        let mut walls = Wallpapers::create_mock(LEN, 0);
        walls.next_wall();
        assert_eq!(walls.index, 1);
        assert_eq!(walls.files.len(), LEN);

        // loop around last element
        walls.index = LEN - 1;
        walls.next_wall();

        assert_eq!(walls.index, 0);
        assert_eq!(walls.files.len(), LEN);
    }

    #[test]
    fn test_prev_wall() {
        static LEN: usize = 5;
        let mut walls = Wallpapers::create_mock(LEN, 1);
        walls.prev_wall();
        assert_eq!(walls.index, 0);
        assert_eq!(walls.files.len(), LEN);

        // loop around first element
        walls.index = 0;
        walls.prev_wall();
        assert_eq!(walls.index, LEN - 1);
        assert_eq!(walls.files.len(), LEN);
    }

    #[test]
    fn test_remove_start() {
        static LEN: usize = 5;
        // remove first index
        let mut walls = Wallpapers::create_mock(LEN, 0);
        walls.remove();
        assert_eq!(walls.index, 0, "remove index 0");
        assert_eq!(walls.current.filename, "1", "remove index 0");
        assert_eq!(walls.files.len(), LEN - 1, "remove index 0");
    }

    #[test]
    fn test_remove_middle() {
        // remove middle index
        static LEN: usize = 5;
        let mut walls = Wallpapers::create_mock(LEN, 2);
        walls.remove();
        assert_eq!(walls.index, 2, "remove index 2");
        assert_eq!(walls.current.filename, "3", "remove index 2");
        assert_eq!(walls.files.len(), LEN - 1, "remove index 2");
    }

    #[test]
    fn test_remove_last() {
        // remove last index
        static LEN: usize = 5;
        let mut walls = Wallpapers::create_mock(LEN, LEN - 1);
        walls.remove();
        assert_eq!(walls.index, 0, "remove index {}", LEN - 1);
        assert_eq!(walls.current.filename, "0", "remove index {}", LEN - 1);
        assert_eq!(walls.files.len(), LEN - 1, "remove index {}", LEN - 1);

        walls.prev_wall();
        assert_eq!(walls.index, LEN - 1 - 1, "prev after remove last");
        assert_eq!(walls.current.filename, "3", "prev after remove last");
        assert_eq!(walls.files.len(), LEN - 1, "prev after remove last");
    }

    #[test]
    fn remove_single() {
        let mut walls = Wallpapers::create_mock(1, 0);
        walls.remove();
        assert_eq!(walls.files.len(), 0);
    }
}

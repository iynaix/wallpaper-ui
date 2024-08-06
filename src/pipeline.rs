use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    aspect_ratio::AspectRatio,
    config::WallpaperConfig,
    cropper::Cropper,
    filename, filter_images, run_wallpaper_ui,
    wallpapers::{WallInfo, WallpapersCsv},
    FaceJson, PathBufExt,
};

/// waits for the images to be written to disk
fn wait_for_image(path: &Path) {
    while !path.exists() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// get scale factor for the image
fn get_scale_factor(width: u32, height: u32, min_width: u32, min_height: u32) -> u32 {
    for scale_factor in 1..=4 {
        if width * scale_factor >= min_width && height * scale_factor >= min_height {
            return scale_factor;
        }
    }

    panic!(
        "image is too small to be upscaled to {}x{}",
        min_width, min_height
    );
}

pub fn optimize_webp(infile: &PathBuf, outfile: &PathBuf) {
    Command::new("cwebp")
        .args(["-q", "100", "-m", "6", "-mt", "-af"])
        .arg(infile)
        .arg("-o")
        .arg(outfile)
        // silence output
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("could not spawn cwebp")
        .wait()
        .expect("could not wait for cwebp");
}

pub fn optimize_jpg(infile: &PathBuf, outfile: &Path) {
    Command::new("jpegoptim")
        .arg("--strip-all")
        .arg(infile)
        .arg("--dest")
        .arg(
            outfile
                .parent()
                .unwrap_or_else(|| panic!("could not get parent directory for {infile:?}")),
        )
        // silence output
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("could not spawn jpegoptim")
        .wait()
        .expect("could not wait for jpegoptim");
}

pub fn optimize_png(infile: &PathBuf, outfile: &PathBuf) {
    Command::new("oxipng")
        .args(["--opt", "max"])
        .arg(infile)
        .arg("--out")
        .arg(outfile)
        // silence output
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("could not spawn oxipng")
        .wait()
        .expect("could not wait for oxipng");
}

struct Upscale((PathBuf, u32)); // (src, scale_factor)
struct Optimize(PathBuf);
struct Detect(PathBuf);
struct Preview(PathBuf);

impl Upscale {
    #[must_use]
    pub fn upscale(&self, format: &Option<String>, progress: &str) -> Optimize {
        let Self((src, scale_factor)) = self;

        // nothing to do here
        let mut dest = src.with_directory("/tmp");

        if let Some(ext) = &format {
            dest = dest.with_extension(ext);
        }

        println!("{} Upscaling {}...", progress, &filename(src));

        Command::new("realcugan-ncnn-vulkan")
            .arg("-i")
            .arg(src)
            .arg("-s")
            .arg(scale_factor.to_string())
            .arg("-o")
            .arg(&dest)
            // silence output
            .stderr(Stdio::null())
            .spawn()
            .expect("could not spawn realcugan-ncnn-vulkan")
            .wait()
            .expect("could not wait for realcugan-ncnn-vulkan");
        Optimize(dest)
    }
}

impl Optimize {
    #[must_use]
    pub fn optimize(&self, format: &Option<String>, wall_dir: &PathBuf, progress: &str) -> Detect {
        let Self(src) = self;
        wait_for_image(src);

        let out_img = format
            .as_ref()
            .map_or_else(|| src.clone(), |format| src.with_extension(format))
            .with_directory(wall_dir);

        println!("{} Optimizing {}...", progress, &filename(src));

        if let Some(ext) = out_img.extension() {
            match ext.to_str().expect("could not convert extension to str") {
                "jpg" | "jpeg" => optimize_jpg(src, &out_img),
                "png" => optimize_png(src, &out_img),
                "webp" => optimize_webp(src, &out_img),
                _ => panic!("unsupported image format: {ext:?}"),
            }
        };

        Detect(out_img)
    }
}

#[derive(Default)]
pub struct WallpaperPipeline {
    to_upscale: Vec<Upscale>,
    to_optimize: Vec<Optimize>,
    to_detect: Vec<Detect>,
    to_preview: Vec<Preview>,

    format: Option<String>,
    min_width: u32,
    min_height: u32,
    wall_dir: PathBuf,
    resolutions: Vec<AspectRatio>,
    wallpapers_csv: WallpapersCsv,
}

impl WallpaperPipeline {
    pub fn new(
        cfg: &WallpaperConfig,
        min_width: u32,
        min_height: u32,
        format: Option<String>,
    ) -> Self {
        // create the csv if it doesn't exist
        let wallpapers_csv = WallpapersCsv::open(cfg).unwrap_or_default();

        // do a check for duplicates
        wallpapers_csv.find_duplicates();

        let wall_dir = &cfg.wallpapers_dir;

        // add images from wallpapers dir that are not in the csv
        let to_detect: Vec<_> = filter_images(&wall_dir)
            .filter(|img| wallpapers_csv.get(&filename(img)).is_none())
            .map(Detect)
            .collect();

        Self {
            to_upscale: Vec::new(),
            to_optimize: Vec::new(),
            to_detect,
            to_preview: Vec::new(),
            min_width,
            min_height,
            format,
            wall_dir: cfg.wallpapers_dir.clone(),
            resolutions: cfg.sorted_resolutions(),
            wallpapers_csv,
        }
    }

    pub fn save_csv(&self) {
        self.wallpapers_csv.save(&self.resolutions);
    }

    pub fn add_image(&mut self, img: &PathBuf, force: bool) {
        let (width, height) = image::image_dimensions(img)
            .unwrap_or_else(|_| panic!("could not get image dimensions for {img:?}"));

        let out_path = self
            .format
            .as_ref()
            .map_or_else(|| img.clone(), |ext| img.with_extension(ext.as_str()))
            .with_directory(&self.wall_dir);

        if out_path.exists() && !force {
            // check if corresponding WallInfo exists
            if let Some(info) = self.wallpapers_csv.get(&filename(&out_path)) {
                // image has been edited, re-process the image
                if info.width / width != info.height / height {
                    let scale_factor =
                        get_scale_factor(width, height, self.min_width, self.min_height);
                    if scale_factor == 1 {
                        self.to_optimize.push(Optimize(img.clone()));
                    } else {
                        self.to_upscale.push(Upscale((img.clone(), scale_factor)));
                    }
                    return;
                }

                // re-preview if no / multiple faces detected and still using default crop
                if info.faces.len() != 1 && info.is_default_crops(&self.resolutions) {
                    self.to_preview.push(Preview(out_path));
                    return;
                }
            // no WallInfo, redetect faces to write to csv
            } else {
                self.to_detect.push(Detect(out_path));
                return;
            }
            return;
        }

        let scale_factor = get_scale_factor(width, height, self.min_width, self.min_height);
        if scale_factor == 1 {
            self.to_optimize.push(Optimize(img.clone()));
        } else {
            self.to_upscale.push(Upscale((img.clone(), scale_factor)));
        }
    }

    pub fn upscale_images(&mut self) {
        let total = self.to_upscale.len();
        self.to_optimize.extend(
            self.to_upscale
                .iter()
                .enumerate()
                .map(|(i, img)| img.upscale(&self.format, &format!("[{}/{}]", i + 1, total))),
        );
    }

    pub fn optimize_images(&mut self) {
        println!();
        let total = self.to_optimize.len();
        self.to_detect
            .extend(self.to_optimize.iter().enumerate().map(|(i, img)| {
                img.optimize(
                    &self.format,
                    &self.wall_dir,
                    &format!("[{}/{}]", i + 1, total),
                )
            }));
    }

    pub async fn detect_faces(&mut self) {
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;

        let paths: Vec<_> = self.to_detect.iter().map(|img| img.0.clone()).collect();

        println!();
        if !paths.is_empty() {
            // wait for all images before proceeding
            for path in &paths {
                wait_for_image(path);
            }

            let mut child = Command::new("anime-face-detector")
                .args(&paths)
                .stdout(Stdio::piped())
                .spawn()
                .expect("failed to spawn anime-face-detector");

            let reader = BufReader::new(
                child
                    .stdout
                    .take()
                    .expect("failed to read stdout of anime-face-detector"),
            );
            let mut lines = reader.lines();
            let mut paths_iter = paths.iter();

            // read each line of anime-face-detector's output async
            let total = paths.len();
            let mut idx = 0;
            while let (Some(path), Ok(Some(line))) = (paths_iter.next(), lines.next_line().await) {
                let fname = filename(path);
                println!("[{}/{}] Detecting faces in {fname}...", idx + 1, total);

                let faces: Vec<FaceJson> =
                    serde_json::from_str(&line).expect("could not deserialize faces");
                let faces: Vec<_> = faces
                    .into_iter()
                    .map(|f: FaceJson| FaceJson::to_face(&f))
                    .collect();

                let (width, height) = image::image_dimensions(path)
                    .unwrap_or_else(|_| panic!("could not get image dimensions: {fname:?}"));
                let cropper = Cropper::new(&faces, width, height);

                // create WallInfo and save it
                let wall_info = WallInfo {
                    filename: fname.clone(),
                    width,
                    height,
                    faces,
                    geometries: self
                        .resolutions
                        .iter()
                        .map(|ratio| (ratio.clone(), cropper.crop(ratio)))
                        .collect(),
                    wallust: String::new(),
                };

                // preview both multiple faces and no faces
                if wall_info.faces.len() != 1 {
                    self.to_preview
                        .push(Preview(path.with_directory(&self.wall_dir)));
                }

                self.wallpapers_csv.insert(fname, wall_info);
                idx += 1;
            }
        }

        self.wallpapers_csv.save(&self.resolutions);
    }

    pub fn preview(self) {
        let preview_images: Vec<_> = self.to_preview.into_iter().map(|img| img.0).collect();

        if !preview_images.is_empty() {
            run_wallpaper_ui(preview_images);
        }
    }
}

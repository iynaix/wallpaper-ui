use clap::Parser;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    aspect_ratio::AspectRatio,
    cli::WallpapersAddArgs,
    config::WallpaperConfig,
    cropper::Cropper,
    filename, filter_images, run_wallpaper_ui,
    wallpapers::{WallInfo, WallpapersCsv},
    FaceJson, PathBufExt,
};

/// waits for the images to be written to disk
fn wait_for_image(path: &Path) {
    while !path.exists() {
        std::thread::sleep(std::time::Duration::from_millis(200));
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

#[derive(Debug, Clone)]
pub enum WallpaperInput {
    Upscale((PathBuf, u32)), // (src, scale_factor)
    Optimize(PathBuf),
    Detect(PathBuf),
    Preview(PathBuf),
}

impl WallpaperInput {
    #[must_use]
    pub fn upscale(&self, format: &Option<String>) -> Self {
        match self {
            Self::Upscale((src, scale_factor)) => {
                // nothing to do here
                if *scale_factor == 1 {
                    Self::Optimize(src.clone())
                } else {
                    let mut dest = src.with_directory("/tmp");

                    if let Some(ext) = &format {
                        dest = dest.with_extension(ext);
                    }

                    println!("Upscaling {}...", &filename(src));

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
                    Self::Optimize(dest)
                }
            }
            _ => self.clone(),
        }
    }

    #[must_use]
    pub fn optimize(&self, format: &Option<String>, wall_dir: &PathBuf) -> Self {
        match self {
            Self::Upscale(_) => {
                eprintln!("Optimize: got unprocessed image: {:?}", &self);
                std::process::exit(1);
            }
            Self::Optimize(src) => {
                wait_for_image(src);

                let out_img = format
                    .as_ref()
                    .map_or_else(|| src.clone(), |format| src.with_extension(format))
                    .with_directory(wall_dir);

                println!("Optimizing {}...", &filename(src));

                if let Some(ext) = out_img.extension() {
                    match ext.to_str().expect("could not convert extension to str") {
                        "jpg" | "jpeg" => optimize_jpg(src, &out_img),
                        "png" => optimize_png(src, &out_img),
                        "webp" => optimize_webp(src, &out_img),
                        _ => panic!("unsupported image format: {ext:?}"),
                    }
                };

                Self::Detect(out_img)
            }
            _ => self.clone(),
        }
    }
}

#[derive(Default)]
pub struct WallpaperPipeline {
    pub images: Vec<WallpaperInput>,
    format: Option<String>,
    min_width: u32,
    min_height: u32,
    wall_dir: PathBuf,
    resolutions: Vec<AspectRatio>,
    wallpapers_csv: WallpapersCsv,
}

impl WallpaperPipeline {
    pub fn new(cfg: &WallpaperConfig) -> Self {
        // create the csv if it doesn't exist
        let mut images = Vec::new();
        let wallpapers_csv = WallpapersCsv::open().unwrap_or_default();

        // do a check for duplicates
        wallpapers_csv.find_duplicates();

        let args = WallpapersAddArgs::parse();
        let wall_dir = &cfg.wallpapers_path;

        // add images from wallpapers dir that are not in the csv
        for img in filter_images(&wall_dir) {
            if wallpapers_csv.get(&filename(&img)).is_none() {
                images.push(WallpaperInput::Detect(img.clone()));
            }
        }

        Self {
            images,
            min_width: args.min_width.unwrap_or(cfg.min_width),
            min_height: args.min_height.unwrap_or(cfg.min_height),
            wall_dir: cfg.wallpapers_path.clone(),
            format: args.format,
            resolutions: cfg.sorted_resolutions(),
            wallpapers_csv,
        }
    }

    pub fn save_csv(&self) {
        self.wallpapers_csv.save(&self.resolutions);
    }

    pub fn add_image(&mut self, img: &PathBuf) {
        let (width, height) = image::image_dimensions(img)
            .unwrap_or_else(|_| panic!("could not get image dimensions for {img:?}"));

        let out_path = self
            .format
            .as_ref()
            .map_or_else(|| img.clone(), |ext| img.with_extension(ext))
            .with_directory(&self.wall_dir);

        if out_path.exists() {
            // check if corresponding WallInfo exists
            if let Some(info) = self.wallpapers_csv.get(&filename(&out_path)) {
                // image has been edited, re-process the image
                if info.width / width != info.height / height {
                    self.images.push(WallpaperInput::Upscale((
                        img.clone(),
                        get_scale_factor(width, height, self.min_width, self.min_height),
                    )));
                    return;
                }

                // re-preview if no / multiple faces detected and still using default crop
                if info.faces.len() != 1 && info.is_default_crops(&self.resolutions) {
                    self.images.push(WallpaperInput::Preview(out_path));
                    return;
                }
            // no WallInfo, redetect faces to write to csv
            } else {
                self.images.push(WallpaperInput::Detect(out_path));
                return;
            }
        }

        self.images.push(WallpaperInput::Upscale((
            img.clone(),
            get_scale_factor(width, height, self.min_width, self.min_height),
        )));
    }

    pub fn upscale_images(&mut self) {
        self.images = self
            .images
            .iter()
            .map(|img| img.upscale(&self.format))
            .collect();
    }

    pub fn optimize_images(&mut self) {
        println!();
        self.images = self
            .images
            .iter()
            .map(|img| img.optimize(&self.format, &self.wall_dir))
            .collect();
    }

    pub async fn detect_faces(&mut self) {
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;

        let mut to_preview = Vec::new();
        let paths: Vec<_> = self
            .images
            .iter()
            .filter_map(|img| match img {
                WallpaperInput::Upscale(_) | WallpaperInput::Optimize(_) => {
                    eprintln!("Detect: got unprocessed image: {:?}", &img);
                    std::process::exit(1);
                }
                WallpaperInput::Detect(path) => Some(path),
                WallpaperInput::Preview(_) => {
                    to_preview.push(img.clone());
                    None
                }
            })
            .collect();

        // wait for all images before proceeding
        for path in &paths {
            wait_for_image(path);
        }

        println!();
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
        while let (Some(path), Ok(Some(line))) = (paths_iter.next(), lines.next_line().await) {
            let fname = filename(path);
            println!("Detecting faces in {fname}...");

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
                to_preview.push(WallpaperInput::Preview(path.with_directory(&self.wall_dir)));
            }

            self.wallpapers_csv.insert(fname, wall_info);
        }

        self.wallpapers_csv.save(&self.resolutions);

        self.images = to_preview;
    }

    pub fn preview(self) {
        let preview_images: Vec<_> = self
            .images
            .into_iter()
            .filter_map(|img| match img {
                WallpaperInput::Preview(path) => Some(path),
                _ => None,
            })
            .collect();

        if !preview_images.is_empty() {
            run_wallpaper_ui(preview_images);
        }
    }
}

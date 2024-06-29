use clap::Parser;

use wallpaper_ui::{
    cli::WallpapersAddArgs, config::WallpaperConfig, filter_images, image_ops::WallpaperPipeline,
    is_image,
};

#[tokio::main]
async fn main() {
    let args = WallpapersAddArgs::parse();
    let cfg = WallpaperConfig::new();

    if args.version {
        println!("wallpapers-add {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let wall_dir = &cfg.wallpapers_path;
    let mut all_files = Vec::new();
    if let Some(paths) = args.paths {
        paths.iter().flat_map(std::fs::canonicalize).for_each(|p| {
            if p.is_file() {
                if let Some(p) = is_image(&p) {
                    all_files.push(p);
                }
            } else {
                if p == *wall_dir {
                    eprintln!("Input directory cannot be the same as the wallpapers directory.");
                    std::process::exit(1);
                }
                all_files.extend(filter_images(&p));
            }
        });
    }

    // allow loading and cleaning of wallpapers.csv
    let mut pipeline = WallpaperPipeline::new(&cfg);

    if all_files.is_empty() {
        pipeline.save_csv();

        eprintln!("No files found in input paths.");
        std::process::exit(1);
    }

    for img in all_files {
        pipeline.add_image(&img);
    }

    pipeline.upscale_images();
    pipeline.optimize_images();
    pipeline.detect_faces().await;
    pipeline.preview();
}

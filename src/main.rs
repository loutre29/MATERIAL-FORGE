//! Material Forge: a clean-room PBR 3D texture painter.

// No console window for release builds; --selftest re-attaches to the parent one.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ambientcg;
mod app;
mod bake;
mod brush;
mod camera;
mod document;
mod engine;
mod export;
mod flood;
mod history;
mod imageio;
mod library;
mod logo;
mod mesh;
mod preview;
mod procedurals;
mod project;
mod selftest;
mod studio;
mod style;

use eframe::{egui, egui_wgpu};
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,wgpu_hal=error,wgpu_core=error,sp_retro=info"),
    )
    .init();
    // A crash leaves its reason next to the library (crash.log): the window
    // app has no console to show it.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let path = library::library_dir().join("..").join("crash.log");
        let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let thread = std::thread::current().name().unwrap_or("worker").to_string();
        let text = format!("[{when}] {} panicked on thread {thread}: {info}
{}

", env!("CARGO_PKG_VERSION"), std::backtrace::Backtrace::force_capture());
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().collect();
    // `--mcp`: the MCP server the Claude app or Claude Code starts (stdio;
    // it talks to the open Material Forge window).
    if args.get(1).map(String::as_str) == Some("--mcp") {
        app::mcp_bridge();
    }
    if args.get(1).map(String::as_str) == Some("--bench") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        if let Err(e) = selftest::bench() {
            eprintln!("bench failed: {e:#}");
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--write-logo") {
        let dir = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "assets".into());
        if let Err(e) = logo::write_files(&dir) {
            eprintln!("logo: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--uibench") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let mesh = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "mesh.fbx".into());
        if let Err(e) = app::ui_bench(&mesh) {
            eprintln!("UIBENCH FAILED: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    // `--envpng <hdr|exr> <png>`: a tonemapped preview of an environment.
    if args.get(1).map(String::as_str) == Some("--envpng") {
        let (Some(src), Some(dst)) = (args.get(2), args.get(3)) else { std::process::exit(2) };
        let ok = engine::env::load_hdr(std::path::Path::new(src)).and_then(|(w, h, px)| {
            let img = image::RgbImage::from_fn(w, h, |x, y| {
                let p = px[(y * w + x) as usize];
                let t = |v: f32| ((v / (1.0 + v)).powf(1.0 / 2.2) * 255.0) as u8;
                image::Rgb([t(p[0]), t(p[1]), t(p[2])])
            });
            let img = image::imageops::resize(&img, 1024, 512, image::imageops::FilterType::Triangle);
            img.save(dst)?;
            let mean: f32 = px.iter().map(|p| (p[0] + p[1] + p[2]) / 3.0).sum::<f32>() / px.len() as f32;
            let max = px.iter().map(|p| p[0].max(p[1]).max(p[2])).fold(0.0f32, f32::max);
            println!("{w}x{h} mean {mean:.3} max {max:.1}");
            Ok(())
        });
        std::process::exit(if ok.is_ok() { 0 } else { 1 });
    }
    if args.get(1).map(String::as_str) == Some("--floodbench") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let Some(mesh) = args.get(2) else { std::process::exit(2) };
        let res = args.get(3).and_then(|r| r.parse().ok()).unwrap_or(4096);
        if let Err(e) = selftest::flood_bench(std::path::Path::new(mesh), res) {
            eprintln!("floodbench: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--bakeshot") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let (Some(mesh), Some(out)) = (args.get(2), args.get(3)) else { std::process::exit(2) };
        let res = args.get(4).and_then(|r| r.parse().ok()).unwrap_or(1024);
        if let Err(e) = selftest::bake_shot(std::path::Path::new(mesh), std::path::Path::new(out), res) {
            eprintln!("bakeshot: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--mcpserve") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let (Some(p), Some(out)) = (args.get(2), args.get(3)) else { std::process::exit(2) };
        if let Err(e) = app::mcp_serve(std::path::Path::new(p), std::path::Path::new(out)) {
            eprintln!("mcpserve: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--musetest") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let (Some(p), Some(out), Some(prompt)) = (args.get(2), args.get(3), args.get(4)) else { std::process::exit(2) };
        let sets: Vec<String> = args.get(5).map(|s| s.split(',').map(|n| n.trim().to_string()).collect()).unwrap_or_default();
        if let Err(e) = app::muse_test(std::path::Path::new(p), std::path::Path::new(out), prompt, &sets) {
            eprintln!("musetest: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--museview") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let (Some(p), Some(out)) = (args.get(2), args.get(3)) else { std::process::exit(2) };
        if let Err(e) = app::muse_view_shot(std::path::Path::new(p), std::path::Path::new(out)) {
            eprintln!("museview: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--autouv") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        // --autouv <mesh> <dir> [smart|free|fast]: unwraps, reports, renders a checker.
        let (Some(src), Some(dst)) = (args.get(2), args.get(3)) else {
            eprintln!("usage: --autouv <mesh> <dir> [smart|free|fast]");
            std::process::exit(2);
        };
        let method = args.get(4).map(String::as_str).unwrap_or("smart");
        if let Err(e) = selftest::uv_check(std::path::Path::new(src), std::path::Path::new(dst), method) {
            eprintln!("autouv: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--alphashot") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "alphashot".into());
        if let Err(e) = selftest::alpha_shot(&out, args.get(3).map(std::path::Path::new)) {
            eprintln!("alphashot: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    // `--photofacts <image>...`: what Claude is told of a photo.
    if args.get(1).map(String::as_str) == Some("--photofacts") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        for p in &args[2..] {
            println!("{p}: {}", app::photo_facts(std::path::Path::new(p)));
        }
        std::process::exit(0);
    }
    // `--lightmeasure <image>`: what Claude is told of a photo's light.
    if args.get(1).map(String::as_str) == Some("--lightmeasure") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        for p in &args[2..] {
            match image::open(p) {
                Ok(i) => println!("{p}: {}", studio::photo_light(&i).unwrap_or_default()),
                Err(e) => println!("{p}: {e}"),
            }
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--studioshot") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let (Some(input), Some(out)) = (args.get(2), args.get(3)) else {
            eprintln!("usage: --studioshot <project.sprp | mesh> <out.png>");
            std::process::exit(2);
        };
        if let Err(e) = selftest::studio_shot(std::path::Path::new(input), std::path::Path::new(out)) {
            eprintln!("studioshot: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--matpreviews") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        if let Err(e) = selftest::material_previews(args.get(2).map(String::as_str).unwrap_or("")) {
            eprintln!("matpreviews: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--matshot") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "matshot".into());
        if let Err(e) = selftest::mat_shot(&out, args.get(3).map(String::as_str).unwrap_or("")) {
            eprintln!("matshot: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--layershot") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        // --layershot <project> <dir>: layer thumbnails next to the solo layer.
        let (Some(src), Some(dst)) = (args.get(2), args.get(3)) else {
            eprintln!("usage: --layershot <project> <dir>");
            std::process::exit(2);
        };
        if let Err(e) = selftest::layer_shot(std::path::Path::new(src), std::path::Path::new(dst)) {
            eprintln!("layershot: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--matsheet") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "matsheet".into());
        if let Err(e) = selftest::mat_sheet(&out) {
            eprintln!("matsheet: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--procsheet") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "procsheet".into());
        if let Err(e) = selftest::proc_sheet(&out) {
            eprintln!("procsheet: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if args.get(1).map(String::as_str) == Some("--uicrawl") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "uicrawl_out".into());
        match app::ui_crawl(&out) {
            Ok(n) => std::process::exit(if n == 0 { 0 } else { 2 }),
            Err(e) => {
                eprintln!("UICRAWL FAILED: {e:#}");
                std::process::exit(1);
            }
        }
    }
    if args.get(1).map(String::as_str) == Some("--uitest") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "uitest_out".into());
        match app::ui_smoke(&out) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("UITEST FAILED: {e:#}");
                std::process::exit(1);
            }
        }
    }
    if args.get(1).map(String::as_str) == Some("--selftest") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(pid: u32) -> i32;
            }
            // SAFETY: plain Win32 call; failure just means no console output.
            unsafe { AttachConsole(u32::MAX) };
        }
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "selftest_out".into());
        let mesh = args.get(3).map(std::path::PathBuf::from);
        match selftest::run(&out, mesh.as_deref()) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("SELFTEST FAILED: {e:#}");
                std::process::exit(1);
            }
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Material Forge")
            .with_icon(std::sync::Arc::new(logo::icon_data()))
            .with_inner_size(if std::env::var_os("MF_DEMO").is_some() { [1920.0, 1080.0] } else { [1600.0, 950.0] })
            .with_min_inner_size([1000.0, 600.0])
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            power_preference: wgpu::PowerPreference::HighPerformance,
            // Mailbox and one queued frame: the newest frame shows at the
            // next refresh, without waiting behind older ones (no tearing).
            // Camera and brush follow the mouse with the least delay.
            // MF_VSYNC=1 falls back to classic vsync.
            present_mode: if std::env::var("MF_VSYNC").is_ok() { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::Mailbox },
            desired_maximum_frame_latency: Some(1),
            device_descriptor: Arc::new(|adapter| wgpu::DeviceDescriptor {
                label: Some("sp_retro device"),
                required_features: engine::wanted_features(adapter),
                required_limits: engine::wanted_limits(adapter),
                memory_hints: wgpu::MemoryHints::default(),
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let startup = args.get(1).filter(|a| !a.starts_with("--")).map(std::path::PathBuf::from);
    eframe::run_native(
        "Material Forge",
        options,
        Box::new(move |cc| Ok(Box::new(app::RetroApp::new(cc, startup)?))),
    )
}

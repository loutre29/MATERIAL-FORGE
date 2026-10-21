//! Material Forge: a clean-room PBR 3D texture painter.

// No console window for release builds; --selftest re-attaches to the parent one.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod bake;
mod brush;
mod camera;
mod document;
mod engine;
mod export;
mod history;
mod library;
mod logo;
mod mesh;
mod preview;
mod procedurals;
mod project;
mod selftest;
mod style;

use eframe::{egui, egui_wgpu};
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,wgpu_hal=error,wgpu_core=error,sp_retro=info"),
    )
    .init();

    let args: Vec<String> = std::env::args().collect();
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
            .with_inner_size([1600.0, 950.0])
            .with_min_inner_size([1000.0, 600.0])
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            power_preference: wgpu::PowerPreference::HighPerformance,
            // One queued frame: the brush cursor and strokes follow the pen
            // a frame sooner than the default queue of two.
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

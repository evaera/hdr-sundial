#![cfg_attr(not(test), windows_subsystem = "windows")]

//! HDR Sundial - drive the Windows SDR content brightness slider from the sun.

mod display;
mod gui;
mod solar;
mod startup;
mod sundial_math;

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    /// Degrees; longitude positive east, negative west.
    latitude_deg: f64,
    longitude_deg: f64,

    /// Slider range (0..100), matching Windows Settings. Floor / ceiling.
    night_brightness_percent: f64,
    day_brightness_percent: f64,

    /// Slider→nits calibration. Windows is ~linear: 0 ≈ 80 nits, 100 ≈ 500.
    calibration_nits_at_0: f64,
    calibration_nits_at_100: f64,

    /// Sun elevation where daylight starts ramping and where it's "full".
    elev_low_deg: f64,
    elev_high_deg: f64,

    /// Only count direct sun when it's actually in front of the window.
    heading_aware: bool,
    /// Heading the window/screen faces, degrees clockwise from north
    /// (0=N, 90=E, 180=S, 270=W).
    heading_deg: f64,
    /// Share of the range from skylight alone; direct sun through the window
    /// fills the rest. 0..1. The CONTRIBUTION slider drives this as
    /// `1 - direct_contribution`.
    ambient_fraction: f64,
    /// Sun must clear this many degrees for full direct-sun contribution.
    direct_ramp_deg: f64,

    /// How often (s) to check the target and the slider's actual value.
    tick_seconds: f64,
    /// Leave the slider alone until it's off-target by this many slider %.
    /// Stops needless writes while still correcting drift / manual edits.
    update_threshold_percent: f64,

    /// Show a system tray icon (left-click opens settings; right-click menu).
    show_tray_icon: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // 0,0 is open ocean — a placeholder that nags you to set real coords.
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            night_brightness_percent: 40.0,
            day_brightness_percent: 95.0,
            calibration_nits_at_0: 80.0,
            calibration_nits_at_100: 0.0, // 0 = auto-detect the panel's max on first run
            elev_low_deg: -6.0,
            elev_high_deg: 10.0,
            heading_aware: false,
            heading_deg: 180.0,
            ambient_fraction: 0.55,
            direct_ramp_deg: 5.0,
            tick_seconds: 2.0,
            update_threshold_percent: 0.5,
            show_tray_icon: true,
        }
    }
}

impl Config {
    /// Convert a slider value (0..100) to SDR white level in nits.
    fn brightness_to_nits(&self, brightness: f64) -> f64 {
        self.calibration_nits_at_0
            + (brightness / 100.0) * (self.calibration_nits_at_100 - self.calibration_nits_at_0)
    }

    /// Convert SDR white level in nits back to a slider value (0..100).
    fn nits_to_brightness(&self, nits: f64) -> f64 {
        let span = (self.calibration_nits_at_100 - self.calibration_nits_at_0)
            .abs()
            .max(f64::EPSILON);
        (nits - self.calibration_nits_at_0) / span * 100.0
    }
}

fn smoothstep(x: f64, edge0: f64, edge1: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0).max(f64::EPSILON)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Daylight factor, 0..1. Heading-aware mode blends diffuse skylight with a
/// direct-beam term that only kicks in when the sun faces the window.
fn daylight01(cfg: &Config, pos: solar::SunPosition) -> f64 {
    let location_only = smoothstep(pos.elevation, cfg.elev_low_deg, cfg.elev_high_deg);
    if !cfg.heading_aware {
        return location_only;
    }
    // Irradiance on a vertical surface ∝ cos(elevation)·cos(Δazimuth), clamped
    // to the half-plane the window faces, and gated to above-horizon sun.
    let d_az = ((pos.azimuth - cfg.heading_deg) * std::f64::consts::PI / 180.0).cos();
    let elev_rad = pos.elevation * std::f64::consts::PI / 180.0;
    let direct =
        (elev_rad.cos() * d_az).max(0.0) * smoothstep(pos.elevation, 0.0, cfg.direct_ramp_deg);

    // ambient_fraction is the single ambient↔direct mix (driven by the
    // CONTRIBUTION slider). 1.0 = location only, 0.0 = fully directional.
    (location_only * cfg.ambient_fraction + direct * (1.0 - cfg.ambient_fraction)).clamp(0.0, 1.0)
}

/// Target brightness on the slider scale (0..100) for a given sun position.
fn target_brightness(cfg: &Config, pos: solar::SunPosition) -> f64 {
    cfg.night_brightness_percent
        + (cfg.day_brightness_percent - cfg.night_brightness_percent) * daylight01(cfg, pos)
}

const DEFAULT_CONFIG: &str = include_str!("default_config.toml");

/// Config lives next to the executable, so the folder is self-contained.
fn config_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating current exe")?;
    let dir = exe.parent().context("exe has no parent directory")?;
    Ok(dir.join("sundial.toml"))
}

/// Load config, creating the bundled default next to the exe on first run.
fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        std::fs::write(&path, DEFAULT_CONFIG)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("Created config at {}", path.display());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn current_position(cfg: &Config) -> solar::SunPosition {
    solar::position(cfg.latitude_deg, cfg.longitude_deg, Utc::now())
}

/// Print sun position and per-display current vs. target levels, then exit.
fn run_status(cfg: &Config) -> Result<()> {
    let pos = current_position(cfg);
    let target = target_brightness(cfg, pos);
    println!("Location:      {:.4}, {:.4}", cfg.latitude_deg, cfg.longitude_deg);
    println!("Sun elevation: {:.2}\u{b0}", pos.elevation);
    println!("Sun azimuth:   {:.2}\u{b0}", pos.azimuth);
    if cfg.heading_aware {
        println!("Heading:       {:.0}\u{b0}", cfg.heading_deg);
    }
    println!(
        "Target:        {target:.0}  ({:.0} nits)",
        cfg.brightness_to_nits(target)
    );
    let targets = display::enumerate_hdr_targets()?;
    if targets.is_empty() {
        println!("No HDR-enabled displays found (SDR brightness only applies when HDR is on).");
        return Ok(());
    }
    for (i, t) in targets.iter().enumerate() {
        match t.get_white_level_nits() {
            Ok(nits) => println!(
                "Display {i}:     {:.0}  ({nits:.0} nits) current",
                cfg.nits_to_brightness(nits)
            ),
            Err(e) => println!("Display {i}:     <error: {e}>"),
        }
    }
    Ok(())
}

/// Print the target curve over the next 24 hours (local time), for tuning.
fn run_curve(cfg: &Config) -> Result<()> {
    use chrono::{Local, Timelike};
    println!("  local   elev    az   bright  bar");
    let start = Local::now()
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or_else(Local::now);
    for h in 0..=24 {
        let local = start + chrono::Duration::hours(h);
        let pos = solar::position(cfg.latitude_deg, cfg.longitude_deg, local.with_timezone(&Utc));
        let b = target_brightness(cfg, pos);
        let span = (cfg.day_brightness_percent - cfg.night_brightness_percent)
            .abs()
            .max(f64::EPSILON);
        let frac = (b - cfg.night_brightness_percent) / span;
        let bar = "#".repeat((frac.clamp(0.0, 1.0) * 30.0).round() as usize);
        println!(
            "  {:02}:00  {:5.1} {:5.0}   {:4.0}   {bar}",
            local.hour(),
            pos.elevation,
            pos.azimuth,
            b
        );
    }
    Ok(())
}

/// Apply the target level to every HDR display once, immediately.
fn run_once(cfg: &Config) -> Result<()> {
    let pos = current_position(cfg);
    let target = target_brightness(cfg, pos);
    let nits = cfg.brightness_to_nits(target);
    let targets = display::enumerate_hdr_targets()?;
    if targets.is_empty() {
        println!("No HDR-enabled displays found; nothing to do.");
        return Ok(());
    }
    for t in &targets {
        t.set_white_level_nits(nits)?;
    }
    println!(
        "Set {} display(s) to {target:.0} ({nits:.0} nits) — sun {:.2}\u{b0} elev, {:.0}\u{b0} az.",
        targets.len(),
        pos.elevation,
        pos.azimuth
    );
    Ok(())
}

/// Set every HDR display to a slider value now (manual override).
fn run_set(cfg: &Config, brightness: f64) -> Result<()> {
    let nits = cfg.brightness_to_nits(brightness);
    let targets = display::enumerate_hdr_targets()?;
    if targets.is_empty() {
        println!("No HDR-enabled displays found; nothing to do.");
    }
    for (i, t) in targets.iter().enumerate() {
        t.set_white_level_nits(nits)?;
        println!("Display {i}: set to {brightness:.0} ({nits:.0} nits)");
    }
    Ok(())
}

/// Find somewhere to print without forcing a console at logon: redirected
/// stdout is left alone, a launching terminal gets attached to, and a fresh
/// console window is allocated only if `--console` was passed.
fn setup_console(force: bool) {
    use windows::Win32::System::Console::{
        AllocConsole, AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
    };
    unsafe {
        let have_stdout = GetStdHandle(STD_OUTPUT_HANDLE)
            .map(|h| !h.is_invalid())
            .unwrap_or(false);
        if have_stdout {
            return; // redirected to a pipe/file — don't disturb it
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            return; // attached to the launching terminal
        }
        if force {
            let _ = AllocConsole();
        }
    }
}

/// Drive the Windows SDR content brightness slider from the sun's position.
///
/// With no subcommand it runs continuously, easing brightness over the day.
#[derive(Parser)]
#[command(
    name = "sundial",
    version,
    about,
    after_help = "Windowless by default (no console flash at logon). Pass --console with \
                  any command to force a console window, e.g. `sundial --console` to \
                  watch the loop. Config lives in sundial.toml next to the exe."
)]
struct Cli {
    /// Force a console window (e.g. to watch the loop when launched windowless).
    #[arg(long, global = true)]
    console: bool,

    /// Start hidden in the background (used by the logon entry).
    #[arg(long)]
    minimized: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Compute and apply the target once, then exit.
    Once,
    /// Print sun position and each display's current level.
    Status,
    /// Print the target brightness curve over the next 24 hours.
    Curve,
    /// Set every HDR display to slider value N (0..100) now.
    Set {
        /// Slider value, 0..100.
        value: f64,
    },
    /// Register to run at logon (shows in Task Manager's Startup apps).
    Startup,
    /// Remove the logon entry.
    RemoveStartup,
}

fn main() -> Result<()> {
    // Attach a console before clap parses so --help/--version are visible when
    // run from a terminal; setup_console stays silent at logon (no console).
    let force_console = std::env::args().any(|a| a == "--console");
    setup_console(force_console);

    let cli = Cli::parse();

    // Startup registration doesn't need (and shouldn't create) a config.
    match cli.command {
        Some(Command::Startup) => return startup::add(),
        Some(Command::RemoveStartup) => return startup::remove(),
        _ => {}
    }

    let cfg = load_config()?;
    if cfg.latitude_deg == 0.0 && cfg.longitude_deg == 0.0 {
        eprintln!(
            "warning: latitude/longitude are unset (0, 0) in {}",
            config_path()?.display()
        );
        eprintln!("         brightness will track the wrong location until you set them.");
    }

    match cli.command {
        Some(Command::Status) => run_status(&cfg),
        Some(Command::Curve) => run_curve(&cfg),
        Some(Command::Once) => run_once(&cfg),
        Some(Command::Set { value }) => run_set(&cfg, value),
        Some(Command::Startup | Command::RemoveStartup) => unreachable!("handled above"),
        None => gui::run(cfg, cli.minimized),
    }
}

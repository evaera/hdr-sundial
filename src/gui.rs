//! GUI mode (no subcommand): the "Instrument" dashboard window, a background
//! brightness thread, an optional tray icon, and single-instance handling.
//! Closing the window only hides it; the process runs until Quit.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use slint::ComponentHandle;
use toml_edit::value;

use crate::sundial_math::{self, Model};
use crate::{config_path, current_position, target_brightness, Config};

slint::include_modules!();

/// Native window handle (HWND) backing the Slint window, for the custom title
/// bar's drag + min/max/close.
fn window_hwnd(ui: &AppWindow) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match ui.window().window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(windows::Win32::Foundation::HWND(
            h.hwnd.get() as *mut core::ffi::c_void,
        )),
        _ => None,
    }
}

/// Ask DWM for Windows 11 rounded corners (and the matching drop shadow) on our
/// frameless window. No-op on older Windows where the attribute is unsupported.
fn round_window_corners(ui: &AppWindow) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    if let Some(hwnd) = window_hwnd(ui) {
        let pref = DWMWCP_ROUND.0;
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const i32 as *const core::ffi::c_void,
                core::mem::size_of::<i32>() as u32,
            );
        }
    }
}

struct AppState {
    cfg: Config,
    doc: toml_edit::DocumentMut,
    path: PathBuf,
    shared: Arc<Mutex<Config>>,
    wake: Sender<()>,
    scrub: Option<f64>,    // solar hour 0..24 when scrubbing; None = live
    season: Option<u16>,   // overridden day-of-year (0..364); None = today
    // Pending Windows-location lookup; the worker thread sends its result here
    // and the tick timer drains it on the UI thread.
    loc_rx: Option<mpsc::Receiver<Result<(f64, f64), String>>>,
}

fn model_of(cfg: &Config) -> Model {
    Model {
        lat: cfg.latitude_deg,
        lon: cfg.longitude_deg,
        night: cfg.night_brightness_percent,
        day: cfg.day_brightness_percent,
        cal0: cfg.calibration_nits_at_0,
        cal100: if cfg.calibration_nits_at_100 > 0.0 {
            cfg.calibration_nits_at_100
        } else {
            500.0
        },
        ramp_start: cfg.elev_low_deg,
        full_daylight: cfg.elev_high_deg,
        doy: 0.0, // set per-frame in recompute (today or the season override)
        heading_aware: cfg.heading_aware,
        heading_deg: cfg.heading_deg,
        ambient_fraction: cfg.ambient_fraction,
        direct_ramp_deg: cfg.direct_ramp_deg,
    }
}

/// Recompute all geometry from state and push it into the Slint backend.
fn recompute(ui: &AppWindow, st: &AppState) {
    let b = ui.global::<Backend>();
    let mut m = model_of(&st.cfg);
    // Season: today's day-of-year unless the scrubber overrides it.
    let today = sundial_math::today_doy();
    let eff_doy = st.season.map(|s| s as i32).unwrap_or(today);
    m.doy = eff_doy as f64;
    let cur_t = st.scrub.unwrap_or_else(|| sundial_math::now_solar_hour(m.lon));
    let c = sundial_math::compute(&m, cur_t);

    // "Live" means both time and season follow real time.
    b.set_is_live(st.scrub.is_none() && st.season.is_none());
    b.set_today_doy(today);
    b.set_season_doy(eff_doy);
    b.set_season_date(sundial_math::season_date_label(eff_doy as i64).into());
    b.set_cur_nits(c.cur_nits);
    b.set_cur_slider(c.cur_slider);
    b.set_cur_elev(c.cur_elev);
    b.set_phase(c.phase.to_uppercase().into());
    b.set_clock(c.clock.into());
    b.set_peak_nits(c.peak_nits);

    b.set_dial_band_area(c.dial_band_area.into());
    b.set_dial_band_line(c.dial_band_line.into());
    b.set_dial_ticks_major(c.dial_ticks_major.into());
    b.set_dial_ticks_minor(c.dial_ticks_minor.into());
    b.set_dial_riseset(c.dial_riseset.into());
    b.set_dial_marker_x(c.dial_marker_x);
    b.set_dial_marker_y(c.dial_marker_y);

    b.set_curve_area(c.curve_area.into());
    b.set_curve_line(c.curve_line.into());
    b.set_curve_grid(c.curve_grid.into());
    b.set_curve_riseset(c.curve_riseset.into());
    b.set_curve_night_l(c.curve_night_l.into());
    b.set_curve_night_r(c.curve_night_r.into());
    b.set_curve_now_x(c.curve_now_x);
    b.set_curve_now_y(c.curve_now_y);
    b.set_curve_rise_x(c.curve_rise_x);
    b.set_curve_set_x(c.curve_set_x);
    b.set_curve_rise_label(c.curve_rise_label.into());
    b.set_curve_set_label(c.curve_set_label.into());
    b.set_hl0(c.hl0.into());
    b.set_hl6(c.hl6.into());
    b.set_hl12(c.hl12.into());
    b.set_hl18(c.hl18.into());
    b.set_hl24(c.hl24.into());

    b.set_globe_land(c.globe_land.into());
    b.set_globe_grid(c.globe_grid.into());
    b.set_globe_grid_hi(c.globe_grid_hi.into());
    b.set_globe_night(c.globe_night.into());

    let ag = sundial_math::arc(st.cfg.elev_low_deg, st.cfg.elev_high_deg, c.cur_elev as f64);
    b.set_arc_base(ag.base.into());
    b.set_arc_night(ag.night.into());
    b.set_arc_ramp(ag.ramp.into());
    b.set_arc_day(ag.day.into());
    b.set_arc_edge(ag.edge.into());
    b.set_arc_notch_minor(ag.notch_minor.into());
    b.set_arc_notch_major(ag.notch_major.into());
    b.set_arc_horizon(ag.horizon.into());
    b.set_arc_sun_x(ag.sun_x);
    b.set_arc_sun_y(ag.sun_y);
    b.set_arc_rs_x(ag.rs_x);
    b.set_arc_rs_y(ag.rs_y);
    b.set_arc_rs_ix(ag.rs_ix);
    b.set_arc_rs_iy(ag.rs_iy);
    b.set_arc_fd_x(ag.fd_x);
    b.set_arc_fd_y(ag.fd_y);
    b.set_arc_fd_ix(ag.fd_ix);
    b.set_arc_fd_iy(ag.fd_iy);
    b.set_arc_rs_pill(ag.rs_pill.into());
    b.set_arc_fd_pill(ag.fd_pill.into());

    let cg = sundial_math::compass(st.cfg.heading_deg);
    b.set_comp_major(cg.ticks_major.into());
    b.set_comp_minor(cg.ticks_minor.into());
    b.set_comp_nx1(cg.needle_x1);
    b.set_comp_ny1(cg.needle_y1);
    b.set_comp_nx2(cg.needle_x2);
    b.set_comp_ny2(cg.needle_y2);
    b.set_comp_heading(st.cfg.heading_deg as f32);
    b.set_comp_cardinal(cg.cardinal.into());
}

/// Write config to disk, mirror into the shared loop config, and wake the loop.
fn persist(st: &mut AppState) {
    let c = st.cfg.clone();
    let d = &mut st.doc;
    d["latitude_deg"] = value(c.latitude_deg);
    d["longitude_deg"] = value(c.longitude_deg);
    d["night_brightness_percent"] = value(c.night_brightness_percent);
    d["day_brightness_percent"] = value(c.day_brightness_percent);
    d["elev_low_deg"] = value(c.elev_low_deg);
    d["elev_high_deg"] = value(c.elev_high_deg);
    d["heading_aware"] = value(c.heading_aware);
    d["heading_deg"] = value(c.heading_deg);
    d["ambient_fraction"] = value(c.ambient_fraction);
    d.remove("heading_mix_percent"); // drop the obsolete key
    d["show_tray_icon"] = value(c.show_tray_icon);
    let _ = std::fs::write(&st.path, d.to_string());
    *st.shared.lock().unwrap() = c;
    let _ = st.wake.send(());
}

/// Format a coordinate for display (≤4 decimals, no trailing zeros).
fn coord_str(v: f64) -> String {
    format!("{}", (v * 10000.0).round() / 10000.0)
}

/// Parse a coordinate field. A comma (e.g. pasting "37.78, -122.39") sets both
/// lat and lon; otherwise just the one field is updated. Returns true when the
/// text split into both coordinates (so the UI should rewrite both boxes).
fn apply_coord_text(st: &mut AppState, text: &str, is_lat: bool) -> bool {
    if let Some((a, b)) = text.split_once(',') {
        if let (Ok(la), Ok(lo)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
            st.cfg.latitude_deg = la;
            st.cfg.longitude_deg = lo;
            return true;
        }
    }
    if let Ok(v) = text.trim().parse::<f64>() {
        if is_lat {
            st.cfg.latitude_deg = v;
        } else {
            st.cfg.longitude_deg = v;
        }
    }
    false
}

/// Apply a finished Windows-location lookup. On success: round both coords to
/// 2 decimals, write them into config + both boxes, return to "idle". On
/// failure: switch the link to "error" (its label becomes a retry prompt).
fn apply_location(ui: &AppWindow, st: &mut AppState, res: Result<(f64, f64), String>) {
    let bk = ui.global::<Backend>();
    match res {
        Ok((lat, lon)) => {
            let lat = (lat * 100.0).round() / 100.0;
            let lon = (lon * 100.0).round() / 100.0;
            st.cfg.latitude_deg = lat;
            st.cfg.longitude_deg = lon;
            bk.set_lat(lat as f32);
            bk.set_lon(lon as f32);
            bk.set_lat_text(coord_str(lat).into());
            bk.set_lon_text(coord_str(lon).into());
            bk.set_loc_state("idle".into());
            persist(st);
            recompute(ui, st);
        }
        Err(_) => bk.set_loc_state("error".into()),
    }
}

pub fn run(cfg: Config, minimized: bool) -> Result<()> {
    match singleton::acquire() {
        singleton::Acquire::AlreadyRunning => {
            singleton::signal_show();
            return Ok(());
        }
        singleton::Acquire::Owner(_handle) => {}
    }

    let mut cfg = cfg;
    let path = config_path()?;
    let mut doc = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_default();

    // Auto-calibrate the panel max if unset (0), writing it back.
    if cfg.calibration_nits_at_100 <= 0.0 {
        if let Some(target) = crate::display::enumerate_hdr_targets()
            .ok()
            .and_then(|t| t.into_iter().next())
        {
            let max = target.probe_max_nits();
            cfg.calibration_nits_at_100 = max;
            doc["calibration_nits_at_100"] = value(max);
            let _ = std::fs::write(&path, doc.to_string());
        } else {
            cfg.calibration_nits_at_100 = 500.0;
        }
    }

    let shared = Arc::new(Mutex::new(cfg.clone()));
    let (wake_tx, wake_rx) = mpsc::channel::<()>();
    {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || brightness_loop(shared, wake_rx));
    }

    let ui = AppWindow::new()?;
    let b = ui.global::<Backend>();
    b.set_lat(cfg.latitude_deg as f32);
    b.set_lon(cfg.longitude_deg as f32);
    b.set_lat_text(coord_str(cfg.latitude_deg).into());
    b.set_lon_text(coord_str(cfg.longitude_deg).into());
    b.set_night_brightness(cfg.night_brightness_percent as f32);
    b.set_day_brightness(cfg.day_brightness_percent as f32);
    b.set_heading_aware(cfg.heading_aware);
    b.set_heading_mix(((1.0 - cfg.ambient_fraction) * 100.0) as f32);
    b.set_version(env!("CARGO_PKG_VERSION").into());
    b.set_run_at_startup(crate::startup::is_registered());
    b.set_show_tray_icon(cfg.show_tray_icon);

    let state = Rc::new(RefCell::new(AppState {
        cfg,
        doc,
        path,
        shared,
        wake: wake_tx,
        scrub: None,
        season: None,
        loc_rx: None,
    }));
    let tray: Rc<RefCell<Option<tray_icon::TrayIcon>>> = Rc::new(RefCell::new(None));

    recompute(&ui, &state.borrow());

    // changed: pull two-way-bound inputs back into config, apply side effects.
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        let tray = Rc::clone(&tray);
        ui.global::<Backend>().on_changed(move || {
            let ui = weak.unwrap();
            let bk = ui.global::<Backend>();
            let mut st = state.borrow_mut();
            st.cfg.latitude_deg = bk.get_lat() as f64;
            st.cfg.longitude_deg = bk.get_lon() as f64;
            st.cfg.night_brightness_percent = bk.get_night_brightness() as f64;
            st.cfg.day_brightness_percent = bk.get_day_brightness() as f64;
            st.cfg.heading_aware = bk.get_heading_aware();
            // CONTRIBUTION slider (0..100 = direct %) drives ambient_fraction.
            st.cfg.ambient_fraction = (1.0 - bk.get_heading_mix() as f64 / 100.0).clamp(0.0, 1.0);
            st.cfg.show_tray_icon = bk.get_show_tray_icon();
            // run at startup (registry side effect)
            let want_startup = bk.get_run_at_startup();
            if want_startup != crate::startup::is_registered() {
                let _ = if want_startup {
                    crate::startup::add()
                } else {
                    crate::startup::remove()
                };
            }
            // tray side effect
            let show_tray = st.cfg.show_tray_icon;
            let has_tray = tray.borrow().is_some();
            if show_tray && !has_tray {
                *tray.borrow_mut() = create_tray().ok();
            } else if !show_tray && has_tray {
                *tray.borrow_mut() = None;
            }
            persist(&mut st);
            recompute(&ui, &st);
        });
    }

    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_scrub(move |f| {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            st.scrub = Some((f as f64).clamp(0.0, 1.0) * 24.0);
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_go_live(move || {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            st.scrub = None;
            st.season = None;
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_set_season(move |doy| {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            st.season = Some((doy.clamp(0, 364)) as u16);
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_nudge_location(move |dlat, dlon| {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            st.cfg.latitude_deg = (st.cfg.latitude_deg + dlat as f64).clamp(-85.0, 85.0);
            let mut lon = st.cfg.longitude_deg + dlon as f64;
            while lon > 180.0 {
                lon -= 360.0;
            }
            while lon < -180.0 {
                lon += 360.0;
            }
            st.cfg.longitude_deg = lon;
            let bk = ui.global::<Backend>();
            bk.set_lat(st.cfg.latitude_deg as f32);
            bk.set_lon(st.cfg.longitude_deg as f32);
            bk.set_lat_text(coord_str(st.cfg.latitude_deg).into());
            bk.set_lon_text(coord_str(st.cfg.longitude_deg).into());
            persist(&mut st);
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_set_ramp(move |angle| {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            let a = (angle as f64).round();
            st.cfg.elev_low_deg = a.clamp(-18.0, 6.0_f64.min(st.cfg.elev_high_deg.round() - 1.0));
            persist(&mut st);
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_set_daylight(move |angle| {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            let a = (angle as f64).round();
            st.cfg.elev_high_deg = a.clamp(0.0_f64.max(st.cfg.elev_low_deg.round() + 1.0), 24.0);
            persist(&mut st);
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_set_heading(move |deg| {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            st.cfg.heading_deg = deg as f64;
            persist(&mut st);
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_lat_edited(move |text| {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            let split = apply_coord_text(&mut st, text.as_str(), true);
            let bk = ui.global::<Backend>();
            bk.set_lat(st.cfg.latitude_deg as f32);
            bk.set_lon(st.cfg.longitude_deg as f32);
            // On a paste that split into both coords, rewrite both boxes; on
            // plain typing leave the field alone so decimals aren't clobbered.
            if split {
                bk.set_lat_text(coord_str(st.cfg.latitude_deg).into());
                bk.set_lon_text(coord_str(st.cfg.longitude_deg).into());
            }
            persist(&mut st);
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_lon_edited(move |text| {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            let split = apply_coord_text(&mut st, text.as_str(), false);
            let bk = ui.global::<Backend>();
            bk.set_lat(st.cfg.latitude_deg as f32);
            bk.set_lon(st.cfg.longitude_deg as f32);
            if split {
                bk.set_lat_text(coord_str(st.cfg.latitude_deg).into());
                bk.set_lon_text(coord_str(st.cfg.longitude_deg).into());
            }
            persist(&mut st);
            recompute(&ui, &st);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_use_location(move || {
            let ui = weak.unwrap();
            let bk = ui.global::<Backend>();
            let mut st = state.borrow_mut();
            if st.loc_rx.is_some() {
                return; // a lookup is already in flight (clicks ignored)
            }
            let (tx, rx) = mpsc::channel::<Result<(f64, f64), String>>();
            st.loc_rx = Some(rx);
            bk.set_loc_state("locating".into());
            // WinRT's GetGeoposition blocks for a fix, so do it off the UI
            // thread; the tick timer applies the result.
            std::thread::spawn(move || {
                let res = crate::location::current_latlon().map_err(|e| e.to_string());
                let _ = tx.send(res);
            });
        });
    }
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        ui.global::<Backend>().on_apply(move || {
            let ui = weak.unwrap();
            let mut st = state.borrow_mut();
            persist(&mut st);
            recompute(&ui, &st);
        });
    }
    ui.global::<Backend>().on_quit(|| {
        let _ = slint::quit_event_loop();
    });

    {
        let weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.hide();
            }
            slint::CloseRequestResponse::HideWindow
        });
    }

    // ---- custom title bar: drag by repositioning the window from deltas ----
    {
        let weak = ui.as_weak();
        ui.global::<Backend>().on_win_drag_move(move |dx, dy| {
            if let Some(ui) = weak.upgrade() {
                let w = ui.window();
                let scale = w.scale_factor();
                let pos = w.position();
                w.set_position(slint::PhysicalPosition::new(
                    pos.x + (dx * scale).round() as i32,
                    pos.y + (dy * scale).round() as i32,
                ));
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.global::<Backend>().on_win_minimize(move || {
            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MINIMIZE};
            if let Some(ui) = weak.upgrade() {
                if let Some(hwnd) = window_hwnd(&ui) {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    }
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.global::<Backend>().on_win_maximize(move || {
            use windows::Win32::UI::WindowsAndMessaging::{
                IsZoomed, ShowWindow, SW_MAXIMIZE, SW_RESTORE,
            };
            if let Some(ui) = weak.upgrade() {
                if let Some(hwnd) = window_hwnd(&ui) {
                    unsafe {
                        let zoomed = IsZoomed(hwnd).as_bool();
                        let _ = ShowWindow(hwnd, if zoomed { SW_RESTORE } else { SW_MAXIMIZE });
                        ui.global::<Backend>().set_win_maximized(!zoomed);
                    }
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.global::<Backend>().on_win_close(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.hide();
            }
        });
    }

    if cfg_show_tray(&state) {
        *tray.borrow_mut() = create_tray().ok();
    }

    let _tray_poll = start_tray_polling(ui.as_weak());
    singleton::spawn_show_listener(ui.as_weak());

    // Live tick: advance the readout every second while not scrubbing. Also
    // applies the rounded-corner style once the native window exists (covers
    // both immediate and minimized-then-shown launches).
    let tick_timer = slint::Timer::default();
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&state);
        let styled = std::cell::Cell::new(false);
        tick_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                if let Some(ui) = weak.upgrade() {
                    if !styled.get() && window_hwnd(&ui).is_some() {
                        round_window_corners(&ui);
                        styled.set(true);
                    }
                    // Apply a Windows-location result if the worker finished.
                    {
                        let mut st = state.borrow_mut();
                        if st.loc_rx.is_some() {
                            match st.loc_rx.as_ref().unwrap().try_recv() {
                                Ok(res) => {
                                    st.loc_rx = None;
                                    apply_location(&ui, &mut st, res);
                                }
                                Err(mpsc::TryRecvError::Empty) => {}
                                Err(mpsc::TryRecvError::Disconnected) => {
                                    st.loc_rx = None;
                                    ui.global::<Backend>().set_loc_state("error".into());
                                }
                            }
                        }
                    }
                    if state.borrow().scrub.is_none() {
                        recompute(&ui, &state.borrow());
                    }
                }
            },
        );
    }

    if !minimized {
        ui.show()?;
        round_window_corners(&ui);
    }
    slint::run_event_loop_until_quit()?;
    Ok(())
}

fn cfg_show_tray(state: &Rc<RefCell<AppState>>) -> bool {
    state.borrow().cfg.show_tray_icon
}

/// Background brightness loop: snap each HDR display to the sun-derived target.
fn brightness_loop(shared: Arc<Mutex<Config>>, wake: mpsc::Receiver<()>) {
    let mut targets = crate::display::enumerate_hdr_targets().unwrap_or_default();
    let mut ticks = 0u32;
    loop {
        let cfg = shared.lock().unwrap().clone();
        ticks += 1;
        if ticks >= 30 {
            ticks = 0;
            if let Ok(fresh) = crate::display::enumerate_hdr_targets() {
                targets = fresh;
            }
        }
        let goal = target_brightness(&cfg, current_position(&cfg));
        let goal_nits = cfg.brightness_to_nits(goal);
        let threshold = cfg.update_threshold_percent.max(0.0);
        for t in &targets {
            if let Ok(nits) = t.get_white_level_nits() {
                if (goal - cfg.nits_to_brightness(nits)).abs() >= threshold {
                    let _ = t.set_white_level_nits(goal_nits);
                }
            }
        }
        let tick = Duration::from_secs_f64(cfg.tick_seconds.max(0.1));
        match wake.recv_timeout(tick) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn tray_icon_image() -> Result<tray_icon::Icon> {
    let bytes = include_bytes!("../assets/sundial.ico");
    let dir = ico::IconDir::read(std::io::Cursor::new(&bytes[..]))?;
    let entry = dir
        .entries()
        .iter()
        .max_by_key(|e| e.width())
        .context("icon has no entries")?;
    let image = entry.decode()?;
    let (w, h) = (image.width(), image.height());
    Ok(tray_icon::Icon::from_rgba(image.rgba_data().to_vec(), w, h)?)
}

fn create_tray() -> Result<tray_icon::TrayIcon> {
    use tray_icon::menu::{Menu, MenuItem};
    let menu = Menu::new();
    menu.append(&MenuItem::with_id("open", "Open Settings", true, None))?;
    menu.append(&MenuItem::with_id("quit", "Quit", true, None))?;
    Ok(tray_icon::TrayIconBuilder::new()
        .with_tooltip("HDR Sundial")
        .with_icon(tray_icon_image()?)
        .with_menu(Box::new(menu))
        .build()?)
}

fn show_window(weak: &slint::Weak<AppWindow>) {
    if let Some(ui) = weak.upgrade() {
        let _ = ui.show();
        ui.window().set_minimized(false);
    }
}

fn start_tray_polling(weak: slint::Weak<AppWindow>) -> slint::Timer {
    use tray_icon::menu::MenuEvent;
    use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(150),
        move || {
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                match ev.id.0.as_str() {
                    "open" => show_window(&weak),
                    "quit" => {
                        let _ = slint::quit_event_loop();
                    }
                    _ => {}
                }
            }
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = ev
                {
                    show_window(&weak);
                }
            }
        },
    );
    timer
}

mod singleton {
    use windows::core::w;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE,
    };

    pub enum Acquire {
        Owner(HANDLE),
        AlreadyRunning,
    }

    pub fn acquire() -> Acquire {
        unsafe {
            match CreateMutexW(None, true, w!("Local\\HDRSundial_Singleton")) {
                Ok(handle) => {
                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        let _ = CloseHandle(handle);
                        Acquire::AlreadyRunning
                    } else {
                        Acquire::Owner(handle)
                    }
                }
                Err(_) => Acquire::Owner(HANDLE::default()),
            }
        }
    }

    pub fn signal_show() {
        unsafe {
            if let Ok(h) = CreateEventW(None, false, false, w!("Local\\HDRSundial_ShowWindow")) {
                let _ = SetEvent(h);
                let _ = CloseHandle(h);
            }
        }
    }

    pub fn spawn_show_listener(weak: slint::Weak<super::AppWindow>) {
        std::thread::spawn(move || unsafe {
            let event = match CreateEventW(None, false, false, w!("Local\\HDRSundial_ShowWindow")) {
                Ok(h) => h,
                Err(_) => return,
            };
            while WaitForSingleObject(event, INFINITE) == WAIT_OBJECT_0 {
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || super::show_window(&weak));
            }
        });
    }
}

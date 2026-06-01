use chrono::{Datelike, Local, NaiveDate, Offset, Timelike, Utc};

const D2R: f64 = std::f64::consts::PI / 180.0;

fn clamp(v: f64, a: f64, b: f64) -> f64 {
    v.max(a).min(b)
}
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn smoothstep(t: f64) -> f64 {
    let t = clamp(t, 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The subset of config the instrument geometry needs.
#[derive(Clone, Copy)]
pub struct Model {
    pub lat: f64,
    pub lon: f64,
    pub night: f64,
    pub day: f64,
    pub cal0: f64,
    pub cal100: f64,
    pub ramp_start: f64,
    pub full_daylight: f64,
    /// Effective day-of-year (0..364) driving the sun's declination — overridden
    /// by the season scrubber, else today.
    pub doy: f64,
    // heading-aware compensation (mirrors the brightness loop's daylight01)
    pub heading_aware: bool,
    pub heading_deg: f64,
    pub ambient_fraction: f64,
    pub direct_ramp_deg: f64,
}

/// Today's day-of-year, 0-based (0..364/365).
pub fn today_doy() -> i32 {
    Utc::now().ordinal0() as i32
}

/// Localized "May 31" for a 0-based day-of-year in the current year.
pub fn season_date_label(doy: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let year = Utc::now().year();
    let ord = (doy.clamp(0, 365) + 1) as u32;
    let d = NaiveDate::from_yo_opt(year, ord)
        .or_else(|| NaiveDate::from_yo_opt(year, 1))
        .unwrap();
    format!("{} {}", MONTHS[d.month0() as usize], d.day())
}

/// Solar declination for a 0-based day-of-year (n = doy + 1 in the classic form).
fn declination_from_doy(doy: f64) -> f64 {
    -23.44 * (D2R * (360.0 / 365.0) * (doy + 11.0)).cos()
}

fn elevation(lat_deg: f64, t: f64, dec: f64) -> f64 {
    let h = 15.0 * (t - 12.0) * D2R;
    let lat = lat_deg * D2R;
    let d = dec * D2R;
    let s = lat.sin() * d.sin() + lat.cos() * d.cos() * h.cos();
    clamp(s, -1.0, 1.0).asin() / D2R
}

/// Solar azimuth, degrees clockwise from north (0=N, 90=E, 180=S, 270=W),
/// from the same simplified hour-angle/declination as `elevation`.
fn azimuth(lat_deg: f64, t: f64, dec: f64, elev: f64) -> f64 {
    let lat = lat_deg * D2R;
    let d = dec * D2R;
    let el = elev * D2R;
    let denom = el.cos() * lat.cos();
    if denom.abs() < 1e-9 {
        return 180.0;
    }
    let cos_az = ((d.sin() - el.sin() * lat.sin()) / denom).clamp(-1.0, 1.0);
    let az0 = cos_az.acos() / D2R; // 0..180
    let hour_angle = 15.0 * (t - 12.0);
    if hour_angle > 0.0 {
        360.0 - az0 // afternoon → west half
    } else {
        az0 // morning → east half
    }
}

/// Daylight factor 0..1, mirroring the brightness loop's `daylight01`:
/// location-only smoothstep, blended with a direct-beam term by
/// `ambient_fraction` when heading-aware.
fn daylight01(elev: f64, az: f64, m: &Model) -> f64 {
    let location_only = smoothstep((elev - m.ramp_start) / (m.full_daylight - m.ramp_start));
    if !m.heading_aware {
        return location_only;
    }
    let d_az = ((az - m.heading_deg) * D2R).cos();
    let direct = (elev * D2R).cos() * d_az;
    let direct = direct.max(0.0) * smoothstep(elev / m.direct_ramp_deg);
    clamp(location_only * m.ambient_fraction + direct * (1.0 - m.ambient_fraction), 0.0, 1.0)
}

/// (sunrise, sunset) as local solar hours, or None for polar day/night.
fn rise_set(lat_deg: f64, dec: f64) -> (Option<f64>, Option<f64>) {
    let lat = lat_deg * D2R;
    let d = dec * D2R;
    let cos_h = -lat.tan() * d.tan();
    if cos_h <= -1.0 {
        (Some(0.0), Some(24.0)) // polar day
    } else if cos_h >= 1.0 {
        (None, None) // polar night
    } else {
        let h0 = clamp(cos_h, -1.0, 1.0).acos() / D2R;
        (Some(12.0 - h0 / 15.0), Some(12.0 + h0 / 15.0))
    }
}

fn slider_at(elev: f64, az: f64, m: &Model) -> f64 {
    lerp(m.night, m.day, daylight01(elev, az, m))
}
fn nits_from_slider(slider: f64, m: &Model) -> f64 {
    lerp(m.cal0, m.cal100, slider / 100.0)
}

/// Current local solar hour from device UTC + longitude.
pub fn now_solar_hour(lon: f64) -> f64 {
    let now = Utc::now();
    let utc_h = now.hour() as f64 + now.minute() as f64 / 60.0 + now.second() as f64 / 3600.0;
    let t = utc_h + lon / 15.0;
    ((t % 24.0) + 24.0) % 24.0
}

pub fn phase(elev: f64, ramp_start: f64) -> &'static str {
    if elev < ramp_start - 2.0 {
        "Night"
    } else if elev < 0.0 {
        "Twilight"
    } else if elev < 8.0 {
        "Golden hour"
    } else {
        "Daylight"
    }
}

/// Whether the system prefers 24-hour time (Windows: HKCU iTime = "1").
fn prefers_24h() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Control Panel\International")
        .and_then(|k| k.get_value::<String, _>("iTime"))
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

/// Hours the local timezone is ahead of UTC (handles DST via chrono::Local).
fn local_offset_hours() -> f64 {
    Local::now().offset().fix().local_minus_utc() as f64 / 3600.0
}

/// Convert a local *solar* hour to the user's wall-clock hour.
fn solar_to_local(t_solar: f64, lon: f64, offset_h: f64) -> f64 {
    let h = t_solar - lon / 15.0 + offset_h;
    ((h % 24.0) + 24.0) % 24.0
}

/// Full clock string, "5:32 PM" or "17:32" per the system preference.
fn fmt_clock(local_h: f64, h24: bool) -> String {
    let total = ((local_h * 60.0).round() as i64).rem_euclid(24 * 60);
    let (h, m) = (total / 60, total % 60);
    if h24 {
        format!("{h:02}:{m:02}")
    } else {
        let ampm = if h < 12 { "AM" } else { "PM" };
        let h12 = if h % 12 == 0 { 12 } else { h % 12 };
        format!("{h12}:{m:02} {ampm}")
    }
}

/// Hour-of-day axis tick label: "06" / "24" in 24h, "6a" / "12a" in 12h.
fn hour_label(h: i64, h24: bool) -> String {
    if h24 {
        format!("{h:02}")
    } else {
        let hh = h % 24;
        let (h12, ap) = if hh == 0 {
            (12, "a")
        } else if hh < 12 {
            (hh, "a")
        } else if hh == 12 {
            (12, "p")
        } else {
            (hh - 12, "p")
        };
        format!("{h12}{ap}")
    }
}

/// Compact clock for small labels, "4:47a" / "7:13p" or "04:47" / "19:13".
fn fmt_short(local_h: f64, h24: bool) -> String {
    let total = ((local_h * 60.0).round() as i64).rem_euclid(24 * 60);
    let (h, m) = (total / 60, total % 60);
    if h24 {
        format!("{h:02}:{m:02}")
    } else {
        let ap = if h < 12 { "a" } else { "p" };
        let h12 = if h % 12 == 0 { 12 } else { h % 12 };
        format!("{h12}:{m:02}{ap}")
    }
}

struct Sample {
    t: f64,
    slider: f64,
    nits: f64,
}

fn samples(m: &Model, dec: f64) -> Vec<Sample> {
    (0..145)
        .map(|i| {
            let t = (i as f64 / 144.0) * 24.0;
            let elev = elevation(m.lat, t, dec);
            let slider = slider_at(elev, azimuth(m.lat, t, dec, elev), m);
            Sample {
                t,
                slider,
                nits: nits_from_slider(slider, m),
            }
        })
        .collect()
}

/// Everything the UI needs for one frame, ready to push into the Slint backend.
#[derive(Default)]
pub struct Computed {
    // live readouts
    pub cur_nits: f32,
    pub cur_slider: f32,
    pub cur_elev: f32,
    pub phase: String,
    pub clock: String,
    pub peak_nits: f32,

    // sun-dial (size 284)
    pub dial_band_area: String,
    pub dial_band_line: String,
    pub dial_ticks_major: String,
    pub dial_ticks_minor: String,
    pub dial_riseset: String,
    pub dial_marker_x: f32,
    pub dial_marker_y: f32,

    // brightness curve
    pub curve_area: String,
    pub curve_line: String,
    pub curve_grid: String,
    pub curve_riseset: String,
    pub curve_night_l: String, // left night rect (path)
    pub curve_night_r: String,
    pub curve_now_x: f32,
    pub curve_now_y: f32,
    pub curve_rise_x: f32,
    pub curve_set_x: f32,
    pub curve_rise_label: String,
    pub curve_set_label: String,
    pub hl0: String,
    pub hl6: String,
    pub hl12: String,
    pub hl18: String,
    pub hl24: String,

    // globe (size 132)
    pub globe_grid: String,
    pub globe_grid_hi: String,
}

fn line(seg: &mut String, x1: f64, y1: f64, x2: f64, y2: f64) {
    seg.push_str(&format!("M {x1:.2} {y1:.2} L {x2:.2} {y2:.2} "));
}

pub fn compute(m: &Model, cur_t: f64) -> Computed {
    let dec = declination_from_doy(m.doy);
    let s = samples(m, dec);
    let (rise, set) = rise_set(m.lat, dec);
    let rise = rise.unwrap_or(0.0);
    let set = set.unwrap_or(24.0);

    let mut c = Computed {
        peak_nits: nits_from_slider(m.day, m) as f32,
        ..Default::default()
    };

    let cur_elev = elevation(m.lat, cur_t, dec);
    let cur_slider = slider_at(cur_elev, azimuth(m.lat, cur_t, dec, cur_elev), m);
    c.cur_elev = cur_elev as f32;
    c.cur_slider = cur_slider as f32;
    c.cur_nits = nits_from_slider(cur_slider, m) as f32;
    c.phase = phase(cur_elev, m.ramp_start).to_string();
    let h24 = prefers_24h();
    let off = local_offset_hours();
    c.clock = fmt_clock(solar_to_local(cur_t, m.lon, off), h24);
    c.hl0 = hour_label(0, h24);
    c.hl6 = hour_label(6, h24);
    c.hl12 = hour_label(12, h24);
    c.hl18 = hour_label(18, h24);
    c.hl24 = hour_label(24, h24);

    // ---- Sun-dial geometry (S = 284) ----
    let sz = 284.0;
    let cx = sz / 2.0;
    let cy = sz / 2.0;
    let r_out = sz / 2.0 - 8.0;
    let r_in = r_out - sz * 0.16;
    let a_deg = |t: f64| ((t / 24.0) * 360.0 - 90.0) * D2R;

    // band: filled area + stroked polyline
    let mut area = format!("M {cx:.2} {cy:.2} ");
    let mut bline = String::new();
    for (i, smp) in s.iter().enumerate() {
        let r = r_in + (smp.slid
            er / 100.0) * (r_out
            - r_in);
        
        let a = a_deg(smp.t);
        let x = cx + a.cos() * r;
        let y = cy + a.sin() * r
            ;


        area.push_str(&format!("L {x:.2} {y:.2} "));
        bline.push_str(&format!("{} {x:.2} {y:.2} ", if i == 0 { "M" } else { "L" }));
    }



    area.push('Z');
    c.dial_band_area = area;
    c.dial_band_line = bline;




    // ticks
    let mut major = String::new();
    let mut minor = String::new();
    for i in 0..24 {
        let a = a_deg(i as f64);
        let len = if i % 6 == 0 { 10.0 } else { 5.0 };
        let x1 = cx + a.cos() * r_out;
        let y1 = cy + a.sin() * r_out;
        let x2 = cx + a.cos() * (r_out - len);
        let y2 = cy + a.sin() * (r_out - len);
        if i % 6 == 0 {
            line(&mut major, x1, y1, x2, y2);
        } else {
            line(&mut minor, x1, y1, x2, y2);
        }
    }
    c.dial_ticks_major = major;
    c.dial_ticks_minor = minor;

    // sunrise/sunset radial ticks (dashed)
    let mut rs = String::new();
    for t in [rise, set] {
        let a = a_deg(t);
        let mut r = r_in - 2.0;
        while r < r_out {
            let re = (r + 5.0).min(r_out);
            line(
                &mut rs,
                cx + a.cos() * r,
                cy + a.sin() * r,
                cx + a.cos() * re,
                cy + a.sin() * re,
            );
            r += 9.0;
        }
    }
    c.dial_riseset = rs;

    // sun marker
    let a = a_deg(cur_t);
    let mr = r_in + (cur_slider / 100.0) * (r_out - r_in);
    c.dial_marker_x = (cx + a.cos
            () * mr) as f32;
           
        
    c.dial_marker_y = (cy + a.sin() * mr) as f32;

    // ---- Brightness curve (W = 540, H = 150) ----
    let w = 540.0;
    let h = 150.0;
    let (pl, pr, pt, pb) = (8.0,
            8.0, 14.0, 22.0);


    let xt = |t: f64| pl + (t / 24.0) * (w - pl - pr);
    let yv = |v: f64| (h - pb) - ((v - m.cal0) / (m.cal100 - m.cal0)) * (h - pt - pb);

    let mut line_p = String::new();
    let mut area_p = format!("M {:.2} {:.2} ", xt(0.0), yv(m.cal0));
    for (i, smp) in s.iter().enum
            erate() {


        let (x, y) = (xt(smp.t), yv(smp.nits));
        line_p.push_str(&format!("{} {x:.2} {y:.2} ", if i == 0 { "M" } else { "L" }));
        area_p.push_str(&format!("L {x:.2} {y:.2} "));
    }
    area_p.push_str(&format!("L {:.2} {:.2} Z", xt(24.0), yv(m.cal0)));
    c.curve_line = line_p;



    c.curve_area = area_p;

    let mut grid = String::new();
    for t in [0.0, 6.0, 12.0, 18.0, 24.0] {
        line(&mut grid, xt(t), pt, xt(t), h - pb);
    }
    c.curve_grid = grid;

    c.curve_night_l = format!(
        "M {:.2} {pt:.2} L {:.2} {pt:.2} L {:.2} {:.2} L {:.2} {:.2} Z",
        xt(0.0),
        xt(rise),
        xt(rise),
        h - pb,
        xt(0.0),
        h - pb
    );
    c.curve_night_r = format!(
        "M {:.2} {pt:.2} L {:.2} {pt:.2} L {:.2} {:.2} L {:.2} {:.2} Z",
        xt(set),
        xt(24.0),
        xt(24.0),
        h - pb,
        xt(set),
        h - pb
    );

    let mut crs = String::new();
    for t in [rise, set] {
        let x = xt(t);
        let mut y = pt;
        while y < h - pb {
            let ye = (y + 5.0).min(h - pb);
            crs.push_str(&format!("M {x:.2} {y:.2} L {x:.2} {ye:.2} "));
            y += 9.0;
        }
    }
    c.curve_riseset = crs;
    c.curve_rise_x = xt(rise) as f32;
    c.curve_set_x = xt(set) as f32;
    c.curve_rise_label = fmt_short(solar_to_local(rise, m.lon, off), h24);
    c.curve_set_label = fmt_short(solar_to_local(set, m.lon, off), h24);
    c.curve_now_x = xt(cur_t) as f32;
    c.curve_now_y = yv(c.cur_nits as f64) as f32;

    // ---- Globe graticule (G = 132) ----
    let g = 132.0;
    let gcx = g / 2.0;
    let gcy = g / 2.0;
    let gr = g / 2.0 - 6.0;
    let lat0 = m.lat * D2R;
    let lon0 = m.lon * D2R;
    let project = |la: f64, lo: f64| -> Option<(f64, f64)> {
        let cosc = lat0.sin() * la.sin() + lat0.cos() * la.cos() * (lo - lon0).cos();
        if cosc < 0.0 {
            return None;
        }
        let x = la.cos() * (lo - lon0).sin();
        let y = lat0.cos() * la.sin() - lat0.sin() * la.cos() * (lo - lon0).cos();
        Some((gcx + x * gr, gcy - y * gr))
    };
    let mut grid = String::new();
    let mut grid_hi = String::new();
    // meridians every 30°
    let mut lon_deg = -180;
    while lon_deg <= 180 {
        let target = if (lon_deg % 30 + 360) % 30 == 0 {
            if lon_deg == 0 {
                &mut grid_hi
            } else {
                &mut grid
            }
        } else {
            lon_deg += 30;
            continue;
        };
        let lo = lon_deg as f64 * D2R;
        let mut prev: Option<(f64, f64)> = None;
        let mut la_deg = -90;
        while la_deg <= 90 {
            let p = project(la_deg as f64 * D2R, lo);
            if let (Some(a), Some(b)) = (prev, p) {
                target.push_str(&format!("M {:.2} {:.2} L {:.2} {:.2} ", a.0, a.1, b.0, b.1));
            }
            prev = p;
            la_deg += 4;
        }
        lon_deg += 30;
    }
    // parallels every 30°
    let mut la_deg = -60;
    while la_deg <= 60 {
        let target = if la_deg == 0 { &mut grid_hi } else { &mut grid };
        let la = la_deg as f64 * D2R;
        let mut prev: Option<(f64, f64)> = None;
        let mut lo_deg = -180;
        while lo_deg <= 180 {
            let p = project(la, lo_deg as f64 * D2R);
            if let (Some(a), Some(b)) = (prev, p) {
                target.push_str(&format!("M {:.2} {:.2} L {:.2} {:.2} ", a.0, a.1, b.0, b.1));
            }
            prev = p;
            lo_deg += 4;
        }
        la_deg += 30;
    }
    c.globe_grid = grid;
    c.globe_grid_hi = grid_hi;

    c
}

#[derive(Default)]
pub struct CompassGeom {
    pub ticks_major: String,
    pub ticks_minor: String,
    pub needle_x1: f32,
    pub needle_y1: f32,
    pub needle_x2: f32,
    pub needle_y2: f32,
    pub cardinal: String,
}

/// Compass (size 112): fixed ticks + a needle pointing at `value`.
pub fn compass(value: f64) -> CompassGeom {
    let cx = 56.0;
    let cy = 56.0;
    let r = 42.0;
    let mut g = CompassGeom::default();
    for i in 0..24 {
        let a = (i as f64 * 15.0 - 90.0) * D2R;
        let len = if i % 6 == 0 { 9.0 } else { 5.0 };
        let x1 = cx + a.cos() * r;
        let y1 = cy + a.sin() * r;
        let x2 = cx + a.cos() * (r - len);
        let y2 = cy + a.sin() * (r - len);
        if i % 6 == 0 {
            line(&mut g.ticks_major, x1, y1, x2, y2);
        } else {
            line(&mut g.ticks_minor, x1, y1, x2, y2);
        }
    }
    let a = (value - 90.0) * D2R;
    g.needle_x1 = (cx - a.cos() * r * 0.4) as f32;
    g.needle_y1 = (cy - a.sin() * r * 0.4) as f32;
    g.needle_x2 = (cx + a.cos() * (r - 6.0)) as f32;
    g.needle_y2 = (cy + a.sin() * (r - 6.0)) as f32;
    let dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    g.cardinal = dirs[((value / 45.0).round() as usize) % 8].to_string();
    g
}

// ---- ElevationArc (side-o
            n protractor; size 
           276 x 176) ----
        
const AMIN: f64 = -20.0;
const AMAX: f64 = 34.0;
const AOX: f64 = 26.0;
const AOY: f64 = 94.0;
const A_R: f64 = 148.0;
const A_RI: f64 = 66.0;

fn arc_po(a: f64) -> (f64, f64) {
    let r = a * D2R;



    (AOX + A_R * r.cos(), AOY - A_R * r.sin())
}
fn arc_pi(a: f64) -> (f64, f64) {
    let r = a * D2R;
    (AOX + A_RI * r.cos(), AOY - A_RI * r.sin())
}
/// Filled annular band between two elevations.
fn arc_band(a1: f64, a2: f64) -> String {
    let (lo, hi) = (a1.min(a
            2), a1.max(a2));


    let mut d = String::new();
    let mut a = lo;
    while a <= hi + 1e-6 {
        let (x, y) = arc_po(a);
        d.push_str(&format!("{}{x:.1} {y:.1} ", if d.is_empty() { "M" } else { "L" }));
        a += 1.5;
    }
    let mut a = hi;
    while a >= lo - 1e-6 {



        let (x, y) = arc_pi(a);
        d.push_str(&format!("L{x:.1} {y:.1} "));
        a -= 1.5;
    }
    d.push('Z');
    d
}

#[derive(Default)]
pub struct ArcGeom {
    pub base: String,
    pub night: String,
    pub ramp: String,
    pub day: String,
            
           
        
    pub edge: String,
    pub notch_minor: String,
    pub notch_major: String,
    pub horizon: String,
    pub sun_x: f32,
    pub sun_y: f32,
    pub rs_x: f32,
    pub rs_y: f32,
    pub rs_ix: f32,
    pub rs_iy: f32,
    pub fd_x: f32,
    pub fd_y: f32,



    pub fd_ix: f32,
    pub fd_iy: f32,
    pub rs_pill: String,
    pub fd_pill: String,
}

fn pill_text(v: f64) -> String {
    format!(
        "{}{}\u{b0}",
        if v > 0.0 { "+" } else { "" },
        v.round() as i64
    )



}

pub fn arc(ramp_start: f64, full_daylight: f64, cur_elev: f64) -> ArcGeom {
    let mut edge = String::new();
    let mut a = AMIN;
    while a <= AMAX + 1e-6 {
        let (x, y) = arc_po(a);
        edge.push_str(&format!("{}{x:.1} {y:.1} ", if edge.is_empty() { "M" } else { "L" }));
        a += 1.5;
    }

    let mut nmin = String::new(
            );


    let mut nmaj = String::new();
    let mut a = AMIN;
    while a <= AMAX + 1e-6 {
        let maj = (a.round() as i64) % 10 == 0;
        let (ox, oy) = arc_po(a);
        let ext = if maj { 6.0 } else { 3.0 };
        let r = a * D2R;
        let (ix, iy) = (AOX + (A_R + ext) * r.cos(), AOY - (A_R + ext) * r.sin());
        let t = if maj { &mut nmaj } else { &mut nmin };
        t.push_str(&format!("M{ox:.1} {oy:.1} L{ix:.1} {iy:.1} "));
        a += 5.0;
    }

    let (h0ox, h0oy) = arc_po(0.0);
    let (h0ix, h0iy) = arc_pi(0.0);
    let (sx, sy) = arc_po(clamp(cur_elev, AMIN, AMAX));
    let (rsx, rsy) = arc_po(ramp_start);
    let (rsix, rsiy) = arc_pi(ramp_start);
    let (fdx, fdy) = arc_po(full_daylight);
    let (fdix, fdiy) = arc_pi(full_daylight);

    ArcGeom {
        base: arc_band(AMIN, AMAX),
        night: arc_band(AMIN, ramp_start),
        ramp: arc_band(ramp_start, full_daylight),
        day: arc_band(full_daylight, AMAX),
        edge,
        notch_minor: nmin,
        notch_major: nmaj,
        horizon: format!("M{h0ix:.1} {h0iy:.1} L{:.1} {h0oy:.1}", h0ox + 5.0),
        sun_x: sx as f32,
        sun_y: sy as f32,
        rs_x: rsx as f32,
        rs_y: rsy as f32,
        rs_ix: rsix as f32,
        rs_iy: rsiy as f32,
        fd_x: fdx as f32,
        fd_y: fdy as f32,
        fd_ix: fdix as f32,
        fd_iy: fdiy as f32,
        rs_pill: pill_text(ramp_start),
        fd_pill: pill_text(full_daylight),
    }
}

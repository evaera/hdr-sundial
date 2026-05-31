//! Solar position math (NOAA algorithm).
//!
//! Given a latitude/longitude and a UTC instant, returns the sun's elevation
//! above the horizon and azimuth (clockwise from true north), in degrees.

use chrono::{DateTime, Utc};

const DEG2RAD: f64 = std::f64::consts::PI / 180.0;
const RAD2DEG: f64 = 180.0 / std::f64::consts::PI;

/// Sun position for a location and time.
#[derive(Debug, Clone, Copy)]
pub struct SunPosition {
    /// Degrees above the horizon (negative = below).
    pub elevation: f64,
    /// Degrees clockwise from true north (0 = N, 90 = E, 180 = S, 270 = W).
    pub azimuth: f64,
}

/// Julian Day number for a UTC instant.
fn julian_day(t: DateTime<Utc>) -> f64 {
    // Unix epoch (1970-01-01T00:00:00Z) is Julian Day 2440587.5.
    2440587.5 + (t.timestamp() as f64 + f64::from(t.timestamp_subsec_millis()) / 1000.0) / 86400.0
}

/// Sun elevation and azimuth for the given location and time.
///
/// `latitude` and `longitude` are in degrees; longitude is positive east.
pub fn position(latitude: f64, longitude: f64, t: DateTime<Utc>) -> SunPosition {
    let jd = julian_day(t);
    let jc = (jd - 2451545.0) / 36525.0; // Julian century since J2000.0

    let geom_mean_long = (280.46646 + jc * (36000.76983 + jc * 0.0003032)).rem_euclid(360.0);
    let geom_mean_anom = 357.52911 + jc * (35999.05029 - 0.0001537 * jc);
    let eccentricity = 0.016708634 - jc * (0.000042037 + 0.0000001267 * jc);

    let m = geom_mean_anom * DEG2RAD;
    let sun_eq_ctr = (m).sin() * (1.914602 - jc * (0.004817 + 0.000014 * jc))
        + (2.0 * m).sin() * (0.019993 - 0.000101 * jc)
        + (3.0 * m).sin() * 0.000289;

    let true_long = geom_mean_long + sun_eq_ctr;
    let app_long = true_long - 0.00569 - 0.00478 * ((125.04 - 1934.136 * jc) * DEG2RAD).sin();

    let mean_obliq =
        23.0 + (26.0 + (21.448 - jc * (46.815 + jc * (0.00059 - jc * 0.001813))) / 60.0) / 60.0;
    let obliq_corr = mean_obliq + 0.00256 * ((125.04 - 1934.136 * jc) * DEG2RAD).cos();

    let declination =
        ((obliq_corr * DEG2RAD).sin() * (app_long * DEG2RAD).sin()).asin() * RAD2DEG;

    // Equation of time (minutes).
    let var_y = (obliq_corr / 2.0 * DEG2RAD).tan().powi(2);
    let l = geom_mean_long * DEG2RAD;
    let eq_time = 4.0
        * RAD2DEG
        * (var_y * (2.0 * l).sin() - 2.0 * eccentricity * m.sin()
            + 4.0 * eccentricity * var_y * m.sin() * (2.0 * l).cos()
            - 0.5 * var_y * var_y * (4.0 * l).sin()
            - 1.25 * eccentricity * eccentricity * (2.0 * m).sin());

    // Minutes since UTC midnight.
    let day_secs = t.timestamp().rem_euclid(86400) as f64
        + f64::from(t.timestamp_subsec_millis()) / 1000.0;
    let utc_minutes = day_secs / 60.0;

    // True solar time (minutes), longitude east positive.
    let true_solar_time = (utc_minutes + eq_time + 4.0 * longitude).rem_euclid(1440.0);

    let hour_angle = if true_solar_time / 4.0 < 0.0 {
        true_solar_time / 4.0 + 180.0
    } else {
        true_solar_time / 4.0 - 180.0
    };

    let lat = latitude * DEG2RAD;
    let decl = declination * DEG2RAD;
    let ha = hour_angle * DEG2RAD;

    let cos_zenith = lat.sin() * decl.sin() + lat.cos() * decl.cos() * ha.cos();
    let zenith_rad = cos_zenith.clamp(-1.0, 1.0).acos();
    let elevation = 90.0 - zenith_rad * RAD2DEG;

    // Azimuth, clockwise from true north (NOAA convention).
    let denom = lat.cos() * zenith_rad.sin();
    let azimuth = if denom.abs() < 1e-9 {
        // Sun at zenith or observer at a pole: azimuth is undefined.
        180.0
    } else {
        let az = (((lat.sin() * zenith_rad.cos()) - decl.sin()) / denom)
            .clamp(-1.0, 1.0)
            .acos()
            * RAD2DEG;
        if hour_angle > 0.0 {
            (az + 180.0).rem_euclid(360.0)
        } else {
            (540.0 - az).rem_euclid(360.0)
        }
    };

    SunPosition { elevation, azimuth }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn solar_noon_is_high_in_summer() {
        // New York, ~solar noon on 2024-06-21 (~16:00 UTC). Elevation should be
        // well above 60 degrees near the summer solstice.
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 16, 0, 0).unwrap();
        let elev = position(40.71, -74.01, t).elevation;
        assert!(elev > 60.0, "expected high midday sun, got {elev}");
    }

    #[test]
    fn midnight_is_below_horizon() {
        // New York at ~05:00 UTC (~midnight local) — sun is below the horizon.
        let t = Utc.with_ymd_and_hms(2024, 6, 21, 5, 0, 0).unwrap();
        let elev = position(40.71, -74.01, t).elevation;
        assert!(elev < 0.0, "expected sun below horizon, got {elev}");
    }

    #[test]
    fn azimuth_is_east_in_morning_west_in_afternoon() {
        // New York, 2024-06-21. 12:00 UTC is 08:00 EDT (sun in the east);
        // 23:00 UTC is 19:00 EDT (sun in the west).
        let morning = position(40.7128, -74.0060, Utc.with_ymd_and_hms(2024, 6, 21, 12, 0, 0).unwrap());
        assert!(
            (45.0..135.0).contains(&morning.azimuth),
            "expected easterly morning sun, got {}",
            morning.azimuth
        );
        let evening = position(40.7128, -74.0060, Utc.with_ymd_and_hms(2024, 6, 21, 23, 0, 0).unwrap());
        assert!(
            (225.0..315.0).contains(&evening.azimuth),
            "expected westerly evening sun, got {}",
            evening.azimuth
        );
    }
}

# HDR Sundial

Drives the Windows **SDR content brightness** slider (the one under
*Settings → Display → HDR*) from the sun's position, making your monitor brighter
when the sun is brightest, without the need for an ambient light sensor.

It computes the sun's position offline based on time, location, and heading, and continuously sets your
HDR display to the correct brightness.

## Installation

**With cargo**:

```
cargo install hdr-sundial
```

This puts the `sundial` binary on your PATH.

**Prebuilt binaries:** download the Windows x64 or ARM64 zip from the
[Releases](https://github.com/evaera/hdr-sundial/releases) page and extract
`sundial.exe` anywhere.

**From source:**

```
git clone https://github.com/evaera/hdr-sundial
cd hdr-sundial
cargo build --release   # -> target/release/sundial.exe
```

## Usage

```
sundial                  run continuously, easing brightness over the day
sundial once             apply the target once and exit
sundial status           print sun position and each display's current level
sundial curve            print the next 24h of target brightness
sundial set N            set every HDR display to slider value N (0..100)
sundial startup          register to run at logon (see below)
sundial remove-startup   unregister
sundial --help           full help; each subcommand also has --help
```

`sundial.toml` is created next to the exe on first run.

## Config

Brightness values are on the Windows slider scale (0..100), the same numbers
Settings shows.

```toml
latitude_deg = 40.7128           # New York City
longitude_deg = -74.0060         # negative = west

night_brightness_percent = 40.0  # floor when dark
day_brightness_percent = 95.0    # ceiling at full daylight

# Slider→nits calibration. Windows is ~linear: 0 ≈ 80 nits, 100 ≈ 500.
# Drag the Settings slider to each end and read it back with `sundial status`.
calibration_nits_at_0 = 80.0
calibration_nits_at_100 = 500.0

elev_low_deg = -6.0              # sun elevation where the ramp starts
elev_high_deg = 10.0             # ...and where diffuse daylight is "full"

heading_aware = true             # see below
heading_deg = 270.0              # 0=N, 90=E, 180=S, 270=W (this one faces due west)
ambient_fraction = 0.55          # share of the range from skylight alone
direct_ramp_deg = 5.0            # sun must clear this many degrees for full direct sun

tick_seconds = 2.0               # how often to check the target and slider value
update_threshold_percent = 0.5   # leave the slider alone until it drifts this far
```

`status` prints both slider value and nits, so to calibrate your
panel: drag the Settings slider to each end, read the nits, and put them in
`calibration_nits_at_0` / `calibration_nits_at_100`.

### Heading-aware brightness

With `heading_aware = true` the target is a blend:

- **Diffuse skylight** — present whenever the sun is up, reaching
  `ambient_fraction` of the night→day range.
- **Direct sun** — fills the rest, but only when the sun is in front of the
  window. Modeled as light on a vertical surface,
  `cos(elevation)·cos(azimuth − heading)`, gated to above-horizon sun.

So a west-facing window sits on the diffuse plateau all morning (sun's in the
east) and ramps up in the afternoon as the sun swings into it. `curve` shows
the shape. Set `heading_aware = false` to track elevation alone.

## Run at login

```
sundial startup          register to run at logon
sundial remove-startup   unregister
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option.

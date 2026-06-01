import { useState } from 'react'

const REPO = 'https://github.com/evaera/hdr-sundial'
const RELEASES = `${REPO}/releases`
const BASE = import.meta.env.BASE_URL // './' — resolves under any Pages subpath

function Label({ children }) {
  return (
    <span className="label">
      <span className="label-dot" />
      {children}
    </span>
  )
}

function SunMark() {
  // Echoes the app's tray glyph: a half-sun over a horizon.
  return (
    <img className="brand-mark" src={`${BASE}sundial.svg`} alt="" width="28" height="28" />
  )
}

function CopyBlock({ lines }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(lines.join('\n'))
      setCopied(true)
      setTimeout(() => setCopied(false), 1400)
    } catch {
      /* clipboard blocked — no-op */
    }
  }
  return (
    <div className="cmd">
      <pre>
        {lines.map((l) => (
          <code key={l}>
            <span className="cmd-prompt">$</span> {l}
          </code>
        ))}
      </pre>
      <button className="cmd-copy" onClick={copy} aria-label="Copy commands">
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  )
}

const FEATURES = [
  {
    label: 'Follows the sun',
    title: 'HDR brightness that tracks daylight',
    body: 'A NOAA solar model computes the sun’s real elevation for your location and eases the Windows SDR content-brightness slider from night to day and back — no schedules to set.',
  },
  {
    label: 'Season-aware',
    title: 'It knows it’s winter',
    body: 'The sun rides low and days run short in winter, high and long in summer. Sundial follows the seasons automatically, so the curve is right for today.',
  },
  {
    label: 'Heading-aware',
    title: 'Big windows? No problem',
    body: 'Tell it which way your window faces and it blends ambient skylight with direct sun, matching the light that actually reaches you.',
  },
  {
    label: 'Out of the way',
    title: 'Set it and forget it',
    body: 'Lives in the system tray, starts at logon, and only nudges the slider when it drifts.',
  },
]

export default function App() {
  return (
    <>
      <header className="site-header">
        <div className="container header-inner">
          <a className="brand" href="#top">
            <SunMark />
            <span className="brand-name">HDR Sundial</span>
          </a>
          <nav className="nav">
            <a href="#features">Features</a>
            <a href="#developers">Developers</a>
            <a className="nav-gh" href={REPO} target="_blank" rel="noreferrer">
              GitHub ↗
            </a>
          </nav>
        </div>
      </header>

      <main id="top">
        <section className="hero container">
          <Label>Windows 11 · HDR displays</Label>
          <h1 className="hero-title">
            Your display, tuned to <span className="grad">the sun</span>.
          </h1>
          <p className="hero-sub">
            HDR Sundial pins your brightness to the sun — no light sensor, just your location, the
            time of day, your window’s direction, and a little astronomy.
          </p>
          <div className="cta-row">
            <a className="btn btn-primary" href={RELEASES} target="_blank" rel="noreferrer">
              Download for Windows
            </a>
            <a className="btn btn-ghost" href={REPO} target="_blank" rel="noreferrer">
              View on GitHub
            </a>
          </div>
          <div className="hero-foot">
            <p className="hero-req">
              <span className="label-dot" /> Requires an HDR display with HDR turned on
            </p>
            <p className="hero-meta">Free &amp; open source · MIT / Apache-2.0</p>
          </div>
        </section>

        <section className="shot container">
          <div className="shot-frame">
            <img
              src={`${BASE}screenshot.png`}
              alt="The HDR Sundial dashboard: a sun dial, a 24-hour brightness curve, a globe showing day and night, and brightness controls."
              width="1155"
              height="958"
              loading="lazy"
            />
          </div>
        </section>

        <section id="features" className="features container">
          <div className="section-head">
            <Label>What it does</Label>
            <h2>Brightness that thinks for itself</h2>
          </div>
          <div className="feature-grid">
            {FEATURES.map((f) => (
              <article className="card" key={f.label}>
                <Label>{f.label}</Label>
                <h3>{f.title}</h3>
                <p>{f.body}</p>
              </article>
            ))}
          </div>
        </section>

        <section id="developers" className="install container">
          <div className="section-head">
            <Label>Developers</Label>
            <h2>Install with Cargo</h2>
            <p className="section-lead">
              Have the Rust toolchain? Install the latest release straight from crates.io:
            </p>
          </div>
          <CopyBlock lines={['cargo install hdr-sundial']} />
          <p className="install-note">
            That puts the <code>sundial</code> binary on your PATH. Run <code>sundial</code> for the
            dashboard, or <code>sundial once</code> / <code>sundial status</code> from a terminal;{' '}
            <code>sundial startup</code> launches it at logon. Prefer a prebuilt exe? Grab one from{' '}
            <a href={RELEASES} target="_blank" rel="noreferrer">
              Releases
            </a>
            .
          </p>
        </section>

        <section className="cta-band">
          <div className="container cta-band-inner">
            <h2>
              Tune your screen to <span className="grad">the sun</span>.
            </h2>
            <div className="cta-row">
              <a className="btn btn-primary" href={RELEASES} target="_blank" rel="noreferrer">
                Download for Windows
              </a>
              <a className="btn btn-ghost" href={REPO} target="_blank" rel="noreferrer">
                View on GitHub
              </a>
            </div>
          </div>
        </section>
      </main>

      <footer className="site-footer">
        <div className="container footer-inner">
          <div className="footer-brand">
            <SunMark />
            <div>
              <div className="brand-name">HDR Sundial</div>
              <div className="footer-fine">© 2026 evaera · MIT / Apache-2.0</div>
              <div className="footer-fine">Made with good vibes</div>
            </div>
          </div>
          <nav className="footer-links">
            <a href={REPO} target="_blank" rel="noreferrer">
              GitHub
            </a>
            <a href={RELEASES} target="_blank" rel="noreferrer">
              Releases
            </a>
            <a href={`${REPO}#readme`} target="_blank" rel="noreferrer">
              Docs
            </a>
          </nav>
        </div>
      </footer>
    </>
  )
}

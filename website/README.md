# HDR Sundial landing page

A static one-page site (Vite + React) promoting the app. It builds to plain
HTML/CSS/JS in `dist/`, so it can be hosted anywhere.

## Develop

```
cd website
npm install
npm run dev
```

`npm run dev` starts Vite with hot-module reload — edits to `src/App.jsx` or
`src/styles.css` show up instantly without a refresh. Open the printed
`http://localhost:5173`.

## Build / preview

```
npm run build      # -> website/dist (static, self-contained)
npm run preview     # serve the production build locally
```

`vite.config.js` sets `base: './'` so the build uses relative URLs and works at
any path — a Pages root *or* a project subpath like `/hdr-sundial/`.

## Publish to GitHub Pages

Deployment is automatic via [`.github/workflows/deploy-pages.yml`](../.github/workflows/deploy-pages.yml):
every push to `main` that touches `website/**` builds the site and deploys it.

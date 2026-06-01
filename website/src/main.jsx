import { createRoot, hydrateRoot } from 'react-dom/client'
import App from './App.jsx'
import './styles.css'

const root = document.getElementById('root')

// Prod: hydrate the HTML that prerender.js baked into index.html (so the page
// is complete with JavaScript disabled). Dev: plain client render — Vite serves
// an empty shell and HMR stays simple.
if (import.meta.env.PROD) {
  hydrateRoot(root, <App />)
} else {
  createRoot(root).render(<App />)
}

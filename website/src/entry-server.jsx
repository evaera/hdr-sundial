import { renderToString } from 'react-dom/server'
import App from './App.jsx'

// Built as an SSR bundle and invoked by prerender.js at build time to produce
// the static HTML for #root.
export function render() {
  return renderToString(<App />)
}

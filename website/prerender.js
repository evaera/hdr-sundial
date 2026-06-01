// Static-site generation: inject the server-rendered app into the built
// index.html so the page is meaningful without JavaScript. Run after both Vite
// builds (client -> dist, SSR -> dist-server). The client bundle then hydrates.
import { readFileSync, writeFileSync, rmSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const indexPath = resolve(here, 'dist/index.html')

const template = readFileSync(indexPath, 'utf-8')
const { render } = await import('./dist-server/entry-server.js')

writeFileSync(indexPath, template.replace('<!--app-html-->', render()))
rmSync(resolve(here, 'dist-server'), { recursive: true, force: true })
console.log('prerendered dist/index.html')

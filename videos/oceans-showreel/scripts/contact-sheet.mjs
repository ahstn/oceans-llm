// Tiles PNGs into one labelled contact sheet (ffmpeg-free; uses the admin UI's Playwright).
// Usage: node scripts/contact-sheet.mjs <out.png> <columns> <thumbWidth> <image...>
import fs from 'node:fs/promises'
import path from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath, pathToFileURL } from 'node:url'

const projectDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const requireFromAdminUi = createRequire(path.join(projectDir, '../../crates/admin-ui/web/package.json'))
const { chromium } = requireFromAdminUi('playwright')

const [out, columnsArg, thumbArg, ...images] = process.argv.slice(2)
const columns = Number(columnsArg)
const thumbWidth = Number(thumbArg)
if (!out || !columns || !thumbWidth || images.length === 0) {
  console.error('usage: node scripts/contact-sheet.mjs <out.png> <columns> <thumbWidth> <image...>')
  process.exit(1)
}

const cells = images
  .map((file) => {
    const src = pathToFileURL(path.resolve(file)).href
    return `<figure><img src="${src}"><figcaption>${path.basename(file, '.png')}</figcaption></figure>`
  })
  .join('')
const html = `<!doctype html><style>
  body { margin: 0; padding: 12px; background: #111; font: 600 18px/1.2 system-ui; color: #ddd; }
  main { display: grid; grid-template-columns: repeat(${columns}, ${thumbWidth}px); gap: 12px; }
  figure { margin: 0; } img { width: 100%; display: block; border: 1px solid #333; }
  figcaption { padding: 4px 2px 0; }
</style><main>${cells}</main>`

// about:blank can't load file:// images, so serve the sheet from a file beside the output.
const htmlFile = `${path.resolve(out)}.html`
await fs.writeFile(htmlFile, html)
const browser = await chromium.launch({ headless: true })
try {
  const page = await browser.newPage({ viewport: { width: columns * (thumbWidth + 12) + 12, height: 400 } })
  await page.goto(pathToFileURL(htmlFile).href, { waitUntil: 'load' })
  await page.screenshot({ path: path.resolve(out), fullPage: true })
  console.log(`wrote ${out}`)
} finally {
  await browser.close()
  await fs.rm(htmlFile, { force: true })
}

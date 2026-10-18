// Samples a coarse color grid from the logo PNG so the push-in target and veil color match.
import path from 'node:path'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
const req = createRequire('/Users/ahstn/git/oceans-llm/crates/admin-ui/web/package.json')
const { chromium } = req('playwright')
const file = process.argv[2]
const browser = await chromium.launch()
const page = await browser.newPage()
const out = await page.evaluate(async (src) => {
  const img = new Image(); img.src = src; await img.decode()
  const c = document.createElement('canvas'); c.width = img.width; c.height = img.height
  const g = c.getContext('2d'); g.drawImage(img, 0, 0)
  const N = 16, rows = []
  for (let j = 0; j < N; j++) {
    const row = []
    for (let i = 0; i < N; i++) {
      const x = Math.floor((i + 0.5) * img.width / N), y = Math.floor((j + 0.5) * img.height / N)
      const d = g.getImageData(x, y, 1, 1).data
      row.push('#' + [d[0], d[1], d[2]].map(v => v.toString(16).padStart(2, '0')).join(''))
    }
    rows.push(row.join(' '))
  }
  return rows.join('\n')
}, 'data:image/png;base64,' + (await import('node:fs')).readFileSync(file).toString('base64'))
console.log(out)
await browser.close()

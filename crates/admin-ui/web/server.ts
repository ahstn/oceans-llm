import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const PORT = Number(process.env.PORT ?? 3000)

const rootDirectory = path.dirname(fileURLToPath(import.meta.url))
const clientDirectory = path.join(rootDirectory, 'dist', 'client')
const serverEntryPoint = path.join(rootDirectory, 'dist', 'server', 'server.js')

const staticAssetExtensions: Record<string, true> = {
  '.avif': true,
  '.css': true,
  '.gif': true,
  '.ico': true,
  '.jpeg': true,
  '.jpg': true,
  '.js': true,
  '.json': true,
  '.map': true,
  '.mjs': true,
  '.png': true,
  '.svg': true,
  '.txt': true,
  '.webmanifest': true,
  '.webp': true,
  '.woff': true,
  '.woff2': true,
}

interface StaticAssetRequest {
  path: string
  terminal: boolean
}

const serverEntryModule = (await import(pathToFileURL(serverEntryPoint).href)) as {
  default: {
    fetch: (request: Request) => Response | Promise<Response>
  }
}

const appHandler = serverEntryModule.default

function resolveStaticAssetRequest(pathname: string): StaticAssetRequest | null {
  if (!pathname.startsWith('/admin/')) {
    return null
  }

  const relativePath = pathname.slice('/admin/'.length)
  if (!relativePath || relativePath.endsWith('/')) {
    return null
  }

  const normalizedPath = path.posix.normalize(relativePath)
  if (normalizedPath.startsWith('../') || normalizedPath.includes('/../')) {
    return null
  }

  const terminal =
    normalizedPath.startsWith('assets/') ||
    staticAssetExtensions[path.extname(normalizedPath)] === true

  return {
    path: path.join(clientDirectory, normalizedPath),
    terminal,
  }
}

const server = Bun.serve({
  port: PORT,
  async fetch(request) {
    const url = new URL(request.url)
    const candidate = resolveStaticAssetRequest(url.pathname)

    if (candidate) {
      const file = Bun.file(candidate.path)
      if (await file.exists()) {
        return new Response(file, {
          headers: {
            'Cache-Control': url.pathname.includes('/assets/')
              ? 'public, max-age=31536000, immutable'
              : 'public, max-age=300',
          },
        })
      }

      if (candidate.terminal) {
        return new Response('Static asset not found', {
          status: 404,
          headers: {
            'Cache-Control': 'no-store',
            'Content-Type': 'text/plain; charset=utf-8',
          },
        })
      }
    }

    return appHandler.fetch(request)
  },
})

console.log(`Started production server: http://localhost:${server.port}`)

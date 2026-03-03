import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const PORT = Number(process.env.PORT ?? 3000)

const rootDirectory = path.dirname(fileURLToPath(import.meta.url))
const clientDirectory = path.join(rootDirectory, 'dist', 'client')
const serverEntryPoint = path.join(rootDirectory, 'dist', 'server', 'server.js')

const serverEntryModule = (await import(pathToFileURL(serverEntryPoint).href)) as {
  default: {
    fetch: (request: Request) => Response | Promise<Response>
  }
}

const appHandler = serverEntryModule.default

function resolveStaticAssetPath(pathname: string): string | null {
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

  return path.join(clientDirectory, normalizedPath)
}

const server = Bun.serve({
  port: PORT,
  async fetch(request) {
    const url = new URL(request.url)
    const candidatePath = resolveStaticAssetPath(url.pathname)

    if (candidatePath) {
      const file = Bun.file(candidatePath)
      if (await file.exists()) {
        return new Response(file, {
          headers: {
            'Cache-Control': url.pathname.includes('/assets/')
              ? 'public, max-age=31536000, immutable'
              : 'public, max-age=300',
          },
        })
      }
    }

    return appHandler.fetch(request)
  },
})

console.log(`Started production server: http://localhost:${server.port}`)

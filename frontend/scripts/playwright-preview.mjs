import { spawn } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const viteBin = join(root, 'node_modules', 'vite', 'bin', 'vite.js')

// e2e runs against API mocks; a nonexistent base keeps accidental real calls visible.
process.env.VITE_API_BASE_URL ??= 'http://127.0.0.1:3457/api/v1'
process.env.VITE_PLATFORM_BRANDING_URL ??= 'http://127.0.0.1:3457/api/v1/runtime/branding'
process.env.VITE_PLATFORM_SERVICES_URL ??= 'http://127.0.0.1:3457/api/v1/runtime/services'
const port = process.env.PLAYWRIGHT_PREVIEW_PORT ?? '4177'

const child = spawn(process.execPath, [viteBin, 'preview', '--port', port, '--strictPort'], {
  cwd: root,
  env: process.env,
  stdio: 'inherit',
})

function stop(signal) {
  if (!child.killed) child.kill(signal)
}

process.on('SIGINT', () => stop('SIGINT'))
process.on('SIGTERM', () => stop('SIGTERM'))
child.on('exit', (code) => process.exit(code ?? 0))

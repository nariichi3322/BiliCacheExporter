// Includes dependency notices conservatively: this is not a list of linked EXE modules.
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const desktop = path.dirname(path.dirname(fileURLToPath(import.meta.url)))

export function noticeFiles(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const file = path.join(dir, entry.name)
    if (entry.isDirectory() && !['node_modules', 'target', '.git'].includes(entry.name)) return noticeFiles(file)
    return entry.isFile() && /^(licen[cs]e|copying|notice|copyright)([._-]|$)/i.test(entry.name) ? [file] : []
  }).sort()
}

export function notice({ name, version, license, source, dir }, extra = []) {
  if (!license) throw new Error(`Missing license declaration: ${name} ${version}`)
  const files = [...noticeFiles(dir), ...extra]
  if (!extra.length && !files.some(file => /^(licen[cs]e|copying)([._-]|$)/i.test(path.basename(file)))) {
    throw new Error(`Missing license text: ${name} ${version}`)
  }
  return [
    `## ${name} ${version} — ${license}`, `Source: ${source}`,
    ...files.flatMap(file => {
      const text = fs.readFileSync(file, 'utf8').trim()
      if (!text) throw new Error(`Empty notice: ${name} ${version}`)
      const relative = path.relative(dir, file)
      return [`--- ${relative.startsWith('..') ? path.basename(file) : relative.replaceAll('\\', '/')} ---`, text]
    }), '',
  ].join('\r\n')
}

export function generate(out) {
  const meta = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--offline', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc'], {
    cwd: path.join(desktop, 'src-tauri'), maxBuffer: 1 << 28,
  }).toString())
  const supplements = JSON.parse(fs.readFileSync(path.join(desktop, 'licenses', 'upstream.json'), 'utf8'))
  const used = new Set(meta.resolve.nodes.map(n => n.id))
  const crates = meta.packages.filter(p => used.has(p.id) && p.name !== 'bili-cache-exporter').sort((a, b) => a.name.localeCompare(b.name)).map(p => {
    const extra = supplements[`${p.name}@${p.version}`]
    return notice({ ...p, source: `https://crates.io/api/v1/crates/${p.name}/${p.version}/download`, dir: path.dirname(p.manifest_path) },
      extra ? [extra.file, ...(extra.additionalFiles ?? [])].map(file => path.join(desktop, 'licenses', 'upstream', file)) : [])
      + (extra ? `Supplement source: ${extra.source}\r\n${extra.note ?? ''}\r\n` : '')
  })
  const lock = JSON.parse(fs.readFileSync(path.join(desktop, 'package-lock.json'), 'utf8'))
  const npm = Object.entries(lock.packages).filter(([key, p]) => key && !p.dev).sort(([a], [b]) => a.localeCompare(b)).map(([key, p]) => {
    const dir = path.join(desktop, key)
    const pkg = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
    if (pkg.version !== p.version) throw new Error(`Installed version differs from lockfile: ${key}`)
    return notice({ name: pkg.name, version: pkg.version, license: pkg.license, source: p.resolved, dir })
  })
  fs.writeFileSync(out, [
    'BiliCacheExporter 第三方依賴授權與聲明。各元件依其原授權提供，不因本專案採 MIT 而改變。',
    '前端清單涵蓋 package-lock 的正式依賴與間接依賴；可能包含被打包器移除的程式。',
    'Rust 清單涵蓋 Windows 目標的解析依賴（含建置與程序巨集）；不代表全部連結進 EXE。',
    'Source 連結提供此版本的原始碼；MPL 元件原始碼仍依 MPL-2.0 提供，未在本專案修改。',
    'ADB／DLL／libwinpthread 與 FFmpeg 不隨包提供。下載後的 tools/adb/NOTICE.txt 與 tools/ffmpeg/LICENSE 列出其條款。',
    '', '# npm production dependencies (including transitive dependencies)', ...npm,
    '', '# Rust Windows dependency graph (including build dependencies)', ...crates, '',
  ].join('\r\n').replace(/\r?\n/g, '\r\n'))
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2]) throw new Error('Usage: node licenses.mjs <output-file>')
  generate(process.argv[2])
}

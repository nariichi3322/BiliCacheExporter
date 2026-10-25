// Writes the third-party notice list for what is compiled into the EXE (Rust crates and bundled npm packages).
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const out = process.argv[2]
const desktop = path.dirname(path.dirname(fileURLToPath(import.meta.url)))
const meta = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc'], { cwd: path.join(desktop, 'src-tauri'), maxBuffer: 1 << 28 }).toString())
const used = new Set(meta.resolve.nodes.map(n => n.id))
const crates = meta.packages.filter(p => used.has(p.id) && p.name !== 'bili-cache-exporter').map(p => `${p.name} ${p.version} — ${p.license ?? 'see ' + (p.repository ?? 'crate')}`).sort()
const npm = ['vue', '@tauri-apps/api', '@tauri-apps/plugin-dialog'].map(name => {
  const pkg = JSON.parse(fs.readFileSync(path.join(desktop, 'node_modules', name, 'package.json'), 'utf8'))
  return `${name} ${pkg.version} — ${pkg.license}`
})
fs.writeFileSync(out, [
  'BiliCacheExporter 內含的第三方元件（編譯進 EXE）。各元件依其授權條款散布；完整條款見各專案原始碼庫。',
  'ADB 與 FFmpeg 不隨包提供，由使用者按需自官方來源下載，授權檔隨工具放在 tools 資料夾。',
  '', '## 前端（npm）', ...npm, '', '## Rust crates', ...crates, '',
].join('\r\n'))

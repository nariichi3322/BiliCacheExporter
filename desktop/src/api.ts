// Typed wrappers for the Rust application commands; the UI never builds shell commands itself.
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

// PascalCase mirrors settings.json; SettingsInput is the camelCase command argument.
export interface Settings {
  Adb: string
  Ffmpeg: string
  Ffprobe: string
  Output: string
  Temp: string
  Theme: 'dark' | 'light'
  Accent: string
  CheckUpdates: boolean
  SkipVersion: string
  [extra: string]: unknown
}

export interface Device { serial: string; state: string; detail: string }
export interface Variant { video: string; audio: string; videoSize: number; audioSize: number; quality: string; issue: string }
export interface FileInfo { path: string; size: number | null; error: string }
export interface CacheItem {
  id: number
  title: string
  remote: string
  variants: Variant[]
  autoVariant: number | null
  needsChoice: boolean
  issue: string
  quality: string
  cover: string | null
  hasCover: boolean
  inventory: FileInfo[]
}
export interface Package { name: string; title: string; version: string; publisher: string; url: string; sha256: string; size: number; license: string }
export interface Snapshot {
  version: string
  settings: Settings
  settingsError: string | null
  storageError: string | null
  logPath: string | null
  devices: Device[]
  serial: string | null
  items: CacheItem[]
  generation: number
  recoverableJobs: number
  packages: Package[]
}
export interface ConnectResult { devices: Device[]; serial: string | null; items: CacheItem[]; generation: number }
export interface ToolStatus { kind: string; path: string; ok: boolean; message: string }
export type Outcome = 'success' | 'partial' | 'failed' | 'cancelled' | 'notStarted'
export interface ItemResult { id: number; outcome: Outcome; message: string; detail: string; path: string | null }
export interface BatchProgress { id: number; index: number; total: number; completed: number; fraction: number | null; stage: string }
export interface Summary { total: number; successful: number; partial: number; failed: number; cancelled: number; notStarted: number }
export interface DownloadProgress { name: string; stage: string; bytes: number; total: number }
export interface CmdError { message: string; cancelled: boolean; missing: string[] }
export interface SettingsInput { adb: string; ffmpeg: string; ffprobe: string; output: string; temp: string; theme: string; accent: string; checkUpdates: boolean }
export interface Release { version: string; notes: string; newer: boolean }

export type AppEvent =
  | { type: 'log'; line: string }
  | { type: 'stage'; text: string }
  | { type: 'item'; item: CacheItem }
  | { type: 'progress'; progress: BatchProgress }
  | { type: 'result'; result: ItemResult }
  | { type: 'download'; progress: DownloadProgress }

export const api = {
  snapshot: () => invoke<Snapshot>('snapshot'),
  ready: () => invoke<void>('ready'),
  saveSettings: (input: SettingsInput) => invoke<Settings>('save_settings', { input }),
  resetSettings: () => invoke<Settings>('reset_settings'),
  checkTools: () => invoke<ToolStatus[]>('check_tools'),
  connect: (serial: string | null) => invoke<ConnectResult>('connect', { serial }),
  scan: () => invoke<ConnectResult>('scan'),
  export: (generation: number, selection: { id: number; variant: number | null }[]) => invoke<Summary>('export', { generation, selection }),
  cancel: () => invoke<void>('cancel'),
  recoverJobs: () => invoke<[number, number]>('recover_jobs'),
  downloadTools: (names: string[]) => invoke<Settings>('download_tools', { names }),
  openFolder: (kind: 'log' | 'output') => invoke<void>('open_folder', { kind }),
  checkUpdate: () => invoke<Release>('check_update'),
  installUpdate: (version: string) => invoke<void>('install_update', { version }),
  skipVersion: (version: string) => invoke<Settings>('skip_version', { version }),
  openUrl: (kind: 'project' | 'releases') => invoke<void>('open_url', { kind }),
  onEvent: (handler: (e: AppEvent) => void) => listen<AppEvent>('app-event', e => handler(e.payload)),
}

export function errorOf(e: unknown): CmdError {
  if (e && typeof e === 'object' && 'message' in e) {
    const c = e as Partial<CmdError>
    return { message: String(c.message), cancelled: !!c.cancelled, missing: c.missing ?? [] }
  }
  return { message: String(e), cancelled: false, missing: [] }
}

export const mb = (bytes: number) => `${(bytes / 1048576).toLocaleString(undefined, { maximumFractionDigits: 1 })} MB`

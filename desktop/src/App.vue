<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorOf, mb, type AppEvent, type BatchProgress, type CacheItem, type Device, type DownloadProgress, type ItemResult, type Package, type Settings, type Summary, type ToolStatus } from './api'

type Phase = 'idle' | 'connect' | 'scan' | 'export' | 'download' | 'recover'

const s = reactive({
  ready: false,
  page: 'main' as 'main' | 'settings',
  phase: 'idle' as Phase,
  version: '',
  settings: null as Settings | null,
  settingsError: null as string | null,
  storageError: null as string | null,
  logPath: null as string | null,
  devices: [] as Device[],
  chosenSerial: '',
  serial: null as string | null,
  items: [] as CacheItem[],
  generation: 0,
  selected: new Set<number>(),
  choices: {} as Record<number, number>,
  results: {} as Record<number, ItemResult>,
  progress: null as BatchProgress | null,
  summary: null as Summary | null,
  stage: '',
  notice: '',
  error: '',
  logs: [] as string[],
  showLogs: false,
  packages: [] as Package[],
  toolDialog: null as string[] | null,
  download: null as DownloadProgress | null,
  downloadError: '',
  recoverable: 0,
  tools: [] as ToolStatus[],
  updateResult: '',
  checking: false,
})
const draft = reactive({ adb: '', ffmpeg: '', ffprobe: '', output: '', temp: '' })
const busy = computed(() => s.phase !== 'idle')
const scanning = computed(() => s.phase === 'connect' || s.phase === 'scan')
const blocked = computed(() => !!s.storageError || !!s.settingsError)
const theme = computed(() => s.settings?.Theme ?? 'dark')
watch(theme, t => { document.documentElement.dataset.theme = t }, { immediate: true })

function variantOf(item: CacheItem): number | null {
  return s.choices[item.id] ?? item.autoVariant ?? null
}
function exportable(item: CacheItem) {
  const v = variantOf(item)
  return !item.issue && v !== null && !item.variants[v]?.issue
}
function reason(item: CacheItem) {
  if (item.issue) return item.issue
  if (variantOf(item) === null) return '請先選擇畫質版本'
  return ''
}
function sizeOf(item: CacheItem) {
  const v = variantOf(item)
  return v === null ? null : item.variants[v].videoSize + item.variants[v].audioSize
}
const exportableItems = computed(() => s.items.filter(exportable))
const selectedItems = computed(() => s.items.filter(i => s.selected.has(i.id) && exportable(i)))
const selectedBytes = computed(() => selectedItems.value.reduce((n, i) => n + (sizeOf(i) ?? 0), 0))
const allSelected = computed(() => exportableItems.value.length > 0 && exportableItems.value.every(i => s.selected.has(i.id)))
const retryable = computed(() => s.items.filter(i => exportable(i) && ['failed', 'partial', 'cancelled', 'notStarted'].includes(s.results[i.id]?.outcome)))
const canExport = computed(() => !busy.value && !blocked.value && !!s.serial && selectedItems.value.length > 0)

function toggleAll(on: boolean) {
  s.selected = new Set(on ? exportableItems.value.map(i => i.id) : [])
}
function toggle(item: CacheItem, on: boolean) {
  const next = new Set(s.selected)
  if (on && exportable(item)) next.add(item.id)
  else next.delete(item.id)
  s.selected = next
}
function choose(item: CacheItem, value: string) {
  if (value === '') delete s.choices[item.id]
  else s.choices[item.id] = Number(value)
  if (!exportable(item)) toggle(item, false)
}

function apply(snapshot: { devices: Device[]; serial: string | null; items: CacheItem[]; generation: number }) {
  if (snapshot.generation !== s.generation) {
    // A new list never carries selections or results from the previous device or scan.
    s.selected = new Set()
    s.choices = {}
    s.results = {}
    s.summary = null
  }
  s.devices = snapshot.devices
  s.serial = snapshot.serial
  s.items = snapshot.items
  s.generation = snapshot.generation
  if (!s.devices.some(d => d.serial === s.chosenSerial)) s.chosenSerial = s.serial ?? ''
}
async function refresh() {
  const snap = await api.snapshot()
  s.version = snap.version
  s.settings = snap.settings
  s.settingsError = snap.settingsError
  s.storageError = snap.storageError
  s.logPath = snap.logPath
  s.packages = snap.packages
  s.recoverable = snap.recoverableJobs
  apply(snap)
  loadDraft()
}
function loadDraft() {
  if (!s.settings) return
  Object.assign(draft, { adb: s.settings.Adb, ffmpeg: s.settings.Ffmpeg, ffprobe: s.settings.Ffprobe, output: s.settings.Output, temp: s.settings.Temp })
}

function fail(e: unknown) {
  const err = errorOf(e)
  s.error = err.cancelled ? '' : err.message
  if (err.cancelled) s.notice = '已取消。'
  if (err.missing.length) openTools(err.missing)
}

async function run(phase: Phase, task: () => Promise<void>) {
  if (busy.value) return
  s.phase = phase
  s.error = ''
  s.notice = ''
  s.stage = ''
  try {
    await task()
  } catch (e) {
    fail(e)
    if (phase === 'connect' || phase === 'scan' || phase === 'export') await refresh().catch(() => {})
  } finally {
    s.phase = 'idle'
    s.stage = ''
    s.progress = null
  }
}

const connect = () => run('connect', async () => {
  s.items = []
  const result = await api.connect(s.chosenSerial || null)
  apply(result)
  if (!result.serial) s.notice = `找到 ${result.devices.length} 部裝置，請選擇要讀取的手機後再按連線。`
  else s.notice = scanNotice(result.items)
})
const rescan = () => run('scan', async () => {
  s.items = []
  const result = await api.scan()
  apply(result)
  s.notice = scanNotice(result.items)
})
function scanNotice(items: CacheItem[]) {
  const bad = items.filter(i => i.issue).length
  return `掃描完成，找到 ${items.length} 個項目${bad ? `，其中 ${bad} 個無法匯出（列中顯示原因）` : ''}。`
}

async function startExport(items: CacheItem[]) {
  await run('export', async () => {
    s.summary = null
    for (const i of items) delete s.results[i.id]
    const summary = await api.export(s.generation, items.map(i => ({ id: i.id, variant: variantOf(i) })))
    s.summary = summary
    s.notice = `已處理 ${summary.successful + summary.partial + summary.failed}/${summary.total}：${summary.successful} 完成，${summary.partial} 部分完成，${summary.failed} 失敗，${summary.cancelled} 取消，${summary.notStarted} 未開始。`
  })
}
const exportSelected = () => startExport(selectedItems.value)
const retry = () => startExport(retryable.value)

function onEvent(e: AppEvent) {
  switch (e.type) {
    case 'log':
      s.logs.push(e.line)
      if (s.logs.length > 2000) s.logs.splice(0, s.logs.length - 2000)
      break
    case 'stage':
      s.stage = e.text
      break
    case 'item':
      if (scanning.value) s.items.push(e.item)
      break
    case 'progress':
      s.progress = e.progress
      break
    case 'result':
      s.results[e.result.id] = e.result
      break
    case 'download':
      s.download = e.progress
      break
  }
}

async function setTheme(light: boolean) {
  if (!s.settings) return
  try {
    s.settings = await api.saveSettings({ ...currentInput(), theme: light ? 'light' : 'dark' })
  } catch (e) {
    fail(e)
  }
}
function currentInput() {
  const t = s.settings!
  return { adb: t.Adb, ffmpeg: t.Ffmpeg, ffprobe: t.Ffprobe, output: t.Output, temp: t.Temp, theme: t.Theme, checkUpdates: t.CheckUpdates }
}
async function setCheckUpdates(on: boolean) {
  if (!s.settings) return
  try {
    s.settings = await api.saveSettings({ ...currentInput(), checkUpdates: on })
  } catch (e) {
    fail(e)
  }
}
async function checkUpdate() {
  s.checking = true
  try {
    const r = await api.checkUpdate()
    s.updateResult = r.newer ? `發現新版本 ${r.version}` : '已是最新版本'
  } catch (e) {
    s.updateResult = errorOf(e).message
  } finally {
    s.checking = false
  }
}
async function openUrl(kind: 'project' | 'releases') {
  try {
    await api.openUrl(kind)
  } catch (e) {
    fail(e)
  }
}
async function saveDraft() {
  s.error = ''
  try {
    s.settings = await api.saveSettings({ ...currentInput(), ...draft })
    loadDraft()
    s.notice = '設定已儲存。'
    await checkTools()
  } catch (e) {
    fail(e)
  }
}
async function saveOutput() {
  if (!s.settings || draft.output === s.settings.Output) return
  try {
    s.settings = await api.saveSettings({ ...currentInput(), output: draft.output })
  } catch (e) {
    fail(e)
  }
}
let toolCheck = 0
async function checkTools() {
  // An older check finishing late must not overwrite the result for newer settings.
  const current = ++toolCheck
  try {
    const tools = await api.checkTools()
    if (current === toolCheck) s.tools = tools
  } catch (e) {
    fail(e)
  }
}
async function resetSettings() {
  try {
    s.settings = await api.resetSettings()
    s.settingsError = null
    loadDraft()
    s.notice = '已備份損壞的設定檔並改用預設值。'
  } catch (e) {
    fail(e)
  }
}
async function browse(field: keyof typeof draft, folder: boolean, saveNow = false) {
  const picked = await open(folder ? { directory: true } : { filters: [{ name: '執行檔', extensions: ['exe'] }] })
  if (typeof picked === 'string') {
    draft[field] = picked
    if (field === 'ffmpeg' && !draft.ffprobe) draft.ffprobe = picked.replace(/ffmpeg\.exe$/i, 'ffprobe.exe')
    if (saveNow) await saveOutput()
  }
}

function openTools(missing: string[]) {
  s.toolDialog = missing.length ? missing : s.packages.map(p => p.name)
  s.downloadError = ''
  s.download = null
}
const dialogPackages = computed(() => s.packages.filter(p => s.toolDialog?.includes(p.name)))
async function download() {
  const names = s.toolDialog ?? []
  s.downloadError = ''
  await run('download', async () => {
    try {
      s.settings = await api.downloadTools(names)
      loadDraft()
      s.toolDialog = null
      s.notice = '工具已下載、驗證並設定完成。'
      await checkTools()
    } catch (e) {
      const err = errorOf(e)
      s.downloadError = err.cancelled ? '已取消下載，現有工具未變更。' : err.message
    }
  })
}
function closeDialog() {
  if (s.phase === 'download') api.cancel()
  else s.toolDialog = null
}
function manualTools() {
  s.toolDialog = null
  s.page = 'settings'
}

const recover = () => run('recover', async () => {
  const [cleaned, kept] = await api.recoverJobs()
  s.recoverable = 0
  s.notice = `已清理 ${cleaned} 個殘留暫存${kept ? `；${kept} 個無法確認歸屬，已保留（詳見記錄）` : ''}。`
})

async function openFolder(kind: 'log' | 'output') {
  try {
    await api.openFolder(kind)
  } catch (e) {
    fail(e)
  }
}

const overall = computed(() => {
  const p = s.progress
  return p ? `正在匯出 · 第 ${p.index} / ${p.total} 部 · 已處理 ${p.completed} 部` : ''
})
function resultDetail(r: ItemResult) {
  return [r.path?.split(/[\\/]/).pop(), r.detail].filter(Boolean).map(t => ' · ' + t).join('')
}
const outcomeLabel: Record<string, string> = { success: '完成', partial: '部分完成', failed: '失敗', cancelled: '已取消', notStarted: '未開始' }

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && s.toolDialog) closeDialog()
}

onMounted(async () => {
  await api.onEvent(onEvent)
  try {
    await refresh()
  } catch (e) {
    fail(e)
  }
  s.ready = true
  await nextTick()
  await api.ready()
})
</script>

<template>
  <div class="app" @keydown="onKey">
    <header class="chrome">
      <span class="logo" aria-hidden="true">B</span>
      <span class="brand">BiliCacheExporter</span>
      <span class="spacer"></span>
      <button class="quiet icon" :aria-label="theme === 'dark' ? '切換淺色模式' : '切換深色模式'" :disabled="!s.settings || blocked" @click="setTheme(theme === 'dark')">
        <svg v-if="theme === 'dark'" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></svg>
        <svg v-else viewBox="0 0 24 24" aria-hidden="true"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" /></svg>
      </button>
      <button class="quiet icon" :aria-pressed="s.page === 'settings'" @click="s.page = s.page === 'settings' ? 'main' : 'settings'; checkTools()">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></svg>
        {{ s.page === 'settings' ? '返回影片' : '設定' }}
      </button>
    </header>

    <main class="content">
      <div v-if="s.storageError" class="notice error" role="alert">{{ s.storageError }}。請將整個程式資料夾移到可寫的位置後再開啟。</div>
      <div v-if="s.settingsError" class="notice error" role="alert">
        {{ s.settingsError }}
        <button @click="resetSettings">備份並重設</button>
      </div>
      <div v-if="s.recoverable && !busy" class="notice" role="status">
        找到 {{ s.recoverable }} 個先前未完成作業的暫存資料。只會清理可確認屬於本程式且未使用中的暫存，不會刪除已匯出影片。
        <button @click="recover">清理</button> <button class="quiet" @click="s.recoverable = 0">稍後</button>
      </div>
      <div v-if="s.error" class="notice error" role="alert">{{ s.error }}</div>
      <div v-else-if="s.notice" class="notice" role="status">{{ s.notice }}</div>

      <section v-if="s.page === 'settings'" aria-labelledby="settings-title">
        <div class="heading">
          <div><h2 id="settings-title">設定</h2><p class="muted">外觀、工具與匯出位置</p></div>
        </div>
        <div class="settings-grid">
          <div class="panel">
            <h3>外觀</h3>
            <label class="setting"><span>淺色模式</span><input type="checkbox" :checked="theme === 'light'" :disabled="blocked" @change="setTheme(($event.target as HTMLInputElement).checked)" /></label>
            <div class="setting"><span>主色</span><span class="accent">● 藍色</span></div>
          </div>
          <div class="panel">
            <h3>連線方式</h3>
            <p class="muted">按「連線」才檢查手機，成功後自動掃描。重新掃描與匯出前會再次確認裝置。</p>
            <small>待機時不輪巡裝置。</small>
          </div>
          <form class="panel full" @submit.prevent="saveDraft">
            <h3>必要工具與位置</h3>
            <label v-for="f in ([['adb', 'ADB（adb.exe）', false], ['ffmpeg', 'FFmpeg（ffmpeg.exe）', false], ['ffprobe', 'FFprobe（ffprobe.exe）', false], ['output', '輸出資料夾', true], ['temp', '暫存資料夾（留空：輸出資料夾下的 .bili-temp）', true]] as const)" :key="f[0]" class="path">
              <small>{{ f[1] }}</small>
              <span class="path-row">
                <input v-model="draft[f[0]]" :aria-label="f[1]" :disabled="busy || blocked" spellcheck="false" />
                <button type="button" :disabled="busy || blocked" @click="browse(f[0], f[2])">瀏覽</button>
              </span>
              <small v-if="s.tools.find(t => t.kind.toLowerCase() === f[0])" :class="s.tools.find(t => t.kind.toLowerCase() === f[0])!.ok ? 'good' : 'warning'">
                {{ s.tools.find(t => t.kind.toLowerCase() === f[0])!.message }}
              </small>
            </label>
            <div class="setting">
              <small>工具只在你按下載時取得，不會在啟動時自動更新。相對路徑以程式資料夾為準，不使用 PATH。</small>
              <span class="actions">
                <button type="button" :disabled="busy || blocked" @click="openTools([])">下載並設定工具</button>
                <button type="button" :disabled="busy || blocked" @click="checkTools">檢查工具</button>
                <button type="submit" class="primary" :disabled="busy || blocked">儲存設定</button>
              </span>
            </div>
          </form>
          <div class="panel full">
            <h3>日誌</h3>
            <div class="setting">
              <span class="muted">記錄存放在程式旁的 log 資料夾，不會自動上傳；裝置序號、網址參數與個人路徑會遮蔽。</span>
              <button :disabled="!s.logPath" @click="openFolder('log')">開啟日誌資料夾</button>
            </div>
          </div>
          <div class="panel full">
            <h3>更新</h3>
            <label class="setting"><span>啟動時檢查更新</span><input type="checkbox" :checked="s.settings?.CheckUpdates ?? true" :disabled="blocked" @change="setCheckUpdates(($event.target as HTMLInputElement).checked)" /></label>
            <div class="setting">
              <span>目前版本 {{ s.version.split('+')[0] }}<span v-if="s.updateResult" class="muted" role="status"> · {{ s.updateResult }}</span></span>
              <span class="actions">
                <button @click="openUrl('project')">GitHub 專案頁面</button>
                <button :disabled="s.checking" @click="checkUpdate">{{ s.checking ? '檢查中…' : '立即檢查' }}</button>
              </span>
            </div>
          </div>
        </div>
      </section>

      <section v-else aria-label="影片匯出">
        <div class="device">
          <svg class="device-icon" viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="2" width="12" height="20" rx="2" /><path d="M11 18h2" /></svg>
          <div>
            <div class="title">{{ s.serial ? (s.devices.find(d => d.serial === s.serial)?.detail.match(/model:(\S+)/)?.[1] ?? 'Android 裝置') : '連接你的 Android 裝置' }}</div>
            <div :class="s.serial ? 'good small' : 'meta'">{{ s.serial ? '已連線 · USB' : '啟用 USB 偵錯並解鎖手機後，按下連線' }}</div>
          </div>
          <span class="spacer"></span>
          <select v-if="s.devices.length > 1" v-model="s.chosenSerial" aria-label="選擇裝置" :disabled="busy">
            <option value="" disabled>選擇裝置…</option>
            <option v-for="d in s.devices" :key="d.serial" :value="d.serial">{{ (d.detail.match(/model:(\S+)/)?.[1] ?? d.serial) + ' · ' + ({ device: '可用', unauthorized: '未授權', offline: '離線' }[d.state] ?? d.state) }}</option>
          </select>
          <button class="primary-outline" :disabled="busy || blocked || (s.devices.length > 1 && !s.chosenSerial)" @click="connect">{{ s.phase === 'connect' ? '連線中…' : s.serial ? '重新連線' : '連線' }}</button>
        </div>

        <template v-if="s.items.length">
          <div class="toolbar">
            <label><input type="checkbox" :checked="allSelected" :disabled="busy || !exportableItems.length" @change="toggleAll(($event.target as HTMLInputElement).checked)" />全選可匯出項目</label>
            <span class="muted">{{ s.items.length }} 個項目 · 已選 {{ selectedItems.length }} 部<template v-if="selectedItems.length"> · {{ mb(selectedBytes) }}</template></span>
            <span class="spacer"></span>
            <button class="quiet" :disabled="busy || !s.serial" @click="rescan">重新掃描</button>
          </div>
          <ul class="rows" aria-label="快取影片">
            <li v-for="item in s.items" :key="item.id" class="row" :class="{ selected: s.selected.has(item.id), invalid: !!item.issue }">
              <input type="checkbox" :aria-label="'選取 ' + item.title" :checked="s.selected.has(item.id)" :disabled="busy || !exportable(item)" @change="toggle(item, ($event.target as HTMLInputElement).checked)" />
              <img v-if="item.cover" class="thumb" :src="item.cover" alt="" />
              <span v-else class="thumb" aria-hidden="true">無封面</span>
              <div class="info">
                <div class="title">{{ item.title }}</div>
                <div class="meta">
                  <select v-if="item.variants.length > 1 && !item.issue" :aria-label="'畫質：' + item.title" :value="s.choices[item.id] ?? item.autoVariant ?? ''" :disabled="busy" @change="choose(item, ($event.target as HTMLSelectElement).value)">
                    <option value="" disabled>選擇畫質（{{ item.variants.length }} 個版本）</option>
                    <option v-for="(v, i) in item.variants" :key="i" :value="i" :disabled="!!v.issue">{{ v.quality }} · {{ mb(v.videoSize + v.audioSize) }}{{ v.issue ? ' · 不可用：' + v.issue : '' }}</option>
                  </select>
                  <span v-else>{{ item.quality || '畫質未知' }}</span>
                  <span v-if="reason(item)" class="warning"> · {{ reason(item) }}</span>
                  <span v-if="!item.hasCover && !item.issue" class="muted"> · 快取沒有封面</span>
                </div>
                <div v-if="s.results[item.id]" class="meta" :class="s.results[item.id].outcome === 'success' ? 'good' : 'warning'">{{ outcomeLabel[s.results[item.id].outcome] }}{{ resultDetail(s.results[item.id]) }}</div>
                <div v-else-if="s.progress && s.progress.id === item.id" class="meta accent">{{ s.progress.stage }}</div>
                <details v-if="item.inventory.length || item.variants.length > 1">
                  <summary>快取詳情</summary>
                  <div class="inventory">
                    <div v-for="(v, i) in item.variants" :key="'v' + i">{{ v.quality }} · {{ mb(v.videoSize + v.audioSize) }}{{ v.issue ? ' · 不可用：' + v.issue : '' }}</div>
                    <div v-for="f in item.inventory" :key="f.path">{{ f.path }} · {{ f.size === null ? '大小未知' : f.size.toLocaleString() + ' bytes' }}{{ f.error ? ' · ' + f.error : '' }}</div>
                  </div>
                </details>
              </div>
              <span class="size">{{ item.issue ? '無法匯出' : sizeOf(item) === null ? '待選畫質' : mb(sizeOf(item)!) }}</span>
            </li>
          </ul>
        </template>
        <div v-else class="rows empty" aria-live="polite">
          <h3>{{ scanning ? (s.stage || '正在處理…') : s.serial ? '這部手機沒有國際版快取影片' : '準備好，把影片帶回電腦' }}</h3>
          <p class="muted">{{ scanning ? '正在檢查影片資訊與可匯出狀態' : s.serial ? '可在手機下載影片後按「重新掃描」。' : '連接手機並按下「連線」，成功後會自動掃描影片。' }}</p>
          <button v-if="scanning" @click="api.cancel()">取消</button>
          <button v-else-if="s.serial" :disabled="busy || blocked" @click="rescan">重新掃描</button>
        </div>
        <p v-if="s.items.length && scanning" class="muted small" aria-live="polite">{{ s.stage }} 已找到 {{ s.items.length }} 個項目… <button class="quiet" @click="api.cancel()">取消</button></p>

        <div class="output">
          <label class="path">
            <small>輸出資料夾</small>
            <input v-model="draft.output" aria-label="輸出資料夾" :disabled="busy || blocked" spellcheck="false" @change="saveOutput" />
          </label>
          <button :disabled="busy || blocked" @click="browse('output', true, true)">瀏覽</button>
          <button class="primary" :disabled="!canExport" @click="exportSelected">匯出所選{{ selectedItems.length ? `（${selectedItems.length}）` : '' }}</button>
        </div>

        <div v-if="s.phase === 'export'" class="progress-box" aria-live="polite">
          <div class="progress-caption">
            <span>{{ overall || s.stage || '準備匯出…' }}</span>
            <button class="quiet" @click="api.cancel()">取消</button>
          </div>
          <progress aria-label="整體進度（以項目數計）" :max="s.progress?.total ?? 1" :value="s.progress?.completed ?? 0"></progress>
          <small>{{ s.progress?.stage }}<template v-if="s.progress?.fraction != null"> · {{ Math.round(s.progress.fraction * 100) }}%</template></small>
          <progress v-if="s.progress?.fraction != null" aria-label="目前階段進度" max="1" :value="s.progress.fraction"></progress>
          <progress v-else aria-label="目前階段進行中（無法量測百分比）"></progress>
        </div>
        <div v-else-if="s.summary" class="progress-box" aria-live="polite">
          <div :class="s.summary.successful === s.summary.total ? 'good' : 'warning'">
            {{ s.summary.successful === s.summary.total ? `匯出完成 · ${s.summary.total} 部影片` : `匯出結束 · ${s.summary.successful} 完成、${s.summary.partial} 部分完成、${s.summary.failed} 失敗、${s.summary.cancelled + s.summary.notStarted} 取消／未開始` }}
          </div>
          <span class="actions">
            <button class="quiet" @click="openFolder('output')">開啟輸出資料夾</button>
            <button v-if="retryable.length" :disabled="busy || blocked || !s.serial" @click="retry">重試未完成項目（{{ retryable.length }}）</button>
          </span>
        </div>
      </section>

      <div class="footer">
        <span>{{ s.serial ? '裝置已連線' : '尚未連線' }} · {{ { idle: '待命', connect: '連線中', scan: '掃描中', export: '匯出中', download: '下載工具中', recover: '清理暫存中' }[s.phase] }}</span>
        <span class="spacer"></span>
        <button class="quiet" :aria-expanded="s.showLogs" @click="s.showLogs = !s.showLogs">{{ s.showLogs ? '收合' : '查看' }}詳細記錄</button>
      </div>
      <pre v-if="s.showLogs" class="logs" tabindex="0" aria-label="詳細記錄">{{ s.logs.join('\n') || '尚無記錄' }}</pre>

      <div v-if="s.toolDialog" class="scrim">
        <section class="dialog" role="dialog" aria-modal="true" aria-labelledby="tools-title">
          <h3 id="tools-title">{{ s.phase === 'download' ? '正在準備必要工具' : '下載並設定必要工具' }}</h3>
          <p class="muted">工具會下載到程式旁的 tools 資料夾，核對 SHA-256 與可執行性後才啟用。也可以改為指定已有的工具。</p>
          <div v-for="p in dialogPackages" :key="p.name" class="tools-row">
            <div>
              <div>{{ p.title }} {{ p.version }}</div>
              <div class="meta">來源：{{ p.publisher }} · 下載 {{ mb(p.size) }}</div>
              <div class="meta">{{ p.license }}</div>
            </div>
          </div>
          <template v-if="s.download && s.phase === 'download'">
            <progress aria-label="工具下載進度" :max="s.download.total" :value="s.download.bytes"></progress>
            <small>{{ s.download.name }} · {{ s.download.stage }} · {{ mb(s.download.bytes) }} / {{ mb(s.download.total) }}</small>
          </template>
          <p v-if="s.downloadError" class="warning" role="alert">{{ s.downloadError }}</p>
          <div class="actions end">
            <button @click="closeDialog">{{ s.phase === 'download' ? '取消下載' : '稍後' }}</button>
            <button v-if="s.phase !== 'download'" @click="manualTools">指定已有工具</button>
            <button v-if="s.phase !== 'download'" class="primary" @click="download">{{ s.downloadError ? '重試下載' : '下載並設定' }}</button>
          </div>
        </section>
      </div>
    </main>
  </div>
</template>

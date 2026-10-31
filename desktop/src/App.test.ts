import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { CacheItem, Snapshot } from './api'

const calls: { cmd: string; args: unknown }[] = []
let handlers: Record<string, (args: any) => unknown> = {}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args?: unknown) => {
    calls.push({ cmd, args })
    const h = handlers[cmd]
    if (!h) return undefined
    return h(args)
  }),
}))
let emit: (payload: unknown) => void = () => {}
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (_: string, cb: (e: { payload: unknown }) => void) => { emit = payload => cb({ payload }); return () => {} }) }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => null) }))

import App from './App.vue'

const variant = (q: string, issue = '', label = q) => ({ video: 'v', audio: 'a', videoSize: 1048576, audioSize: 1048576, quality: q, label, issue })
const item = (id: number, extra: Partial<CacheItem> = {}): CacheItem => ({
  id, title: `影片 ${id}`, remote: `/r/${id}`, variants: [variant('80（快取目錄）')], autoVariant: 0, needsChoice: false, issue: '', quality: '80（快取目錄）', cover: null, hasCover: true, danmaku: 0, inventory: [], source: '國際版（com.bilibili.app.in）', exported: false, ...extra,
})
const items = [
  item(0),
  item(1, { title: '多畫質', variants: [variant('80'), variant('116')], autoVariant: null, needsChoice: true, quality: '2 個版本（需選擇畫質）' }),
  item(2, { title: '未完成', issue: '下載尚未完成', autoVariant: null }),
]

function snapshot(extra: Partial<Snapshot> = {}): Snapshot {
  return {
    version: '2.0.0+test', settings: { Adb: 'tools\\adb\\adb.exe', Ffmpeg: 'f', Ffprobe: 'p', Output: 'D:\\Out', Temp: '', Theme: 'dark', Accent: 'blue', CheckUpdates: true, FullDecode: false, SkipVersion: '' },
    settingsError: null, storageError: null, logPath: 'log', devices: [], serial: null, items: [], generation: 0, recoverableJobs: 0,
    packages: [{ name: 'adb', title: 'Android SDK Platform-Tools（ADB）', version: '37.0.1', publisher: 'Google', url: 'https://x', sha256: 'h', size: 8044989, license: 'L' }],
    ...extra,
  }
}

async function start(extra: Partial<Snapshot> = {}) {
  handlers.snapshot = () => snapshot(extra)
  const wrapper = mount(App, { attachTo: document.body })
  await flushPromises()
  return wrapper
}

const connected = { devices: [{ serial: 'S1', state: 'device', detail: 'model:SM_S9260' }], serial: 'S1', items, generation: 1 }

beforeEach(() => {
  calls.length = 0
  handlers = { connect: () => connected, save_settings: ({ input }: any) => ({ Adb: input.adb, Ffmpeg: input.ffmpeg, Ffprobe: input.ffprobe, Output: input.output, Temp: input.temp, Theme: input.theme, Accent: input.accent, CheckUpdates: input.checkUpdates, FullDecode: input.fullDecode }) }
  document.body.innerHTML = ''
})

describe('compact list', () => {
  it('starts without device queries or sample data', async () => {
    const w = await start()
    expect(calls.map(c => c.cmd)).toEqual(['snapshot', 'ready', 'check_update'])
    expect(w.text()).toContain('準備好，把影片帶回電腦')
    expect(w.findAll('li.row')).toHaveLength(0)
    expect(w.get('button.primary').attributes('disabled')).toBeDefined()
  })

  it('connects, lists rows unselected and only exports valid choices', async () => {
    const w = await start()
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    const rows = w.findAll('li.row')
    expect(rows).toHaveLength(3)
    const boxes = rows.map(r => r.get('input[type=checkbox]'))
    expect(boxes.every(b => !(b.element as HTMLInputElement).checked)).toBe(true)
    expect(boxes[1].attributes('disabled')).toBeDefined()
    expect(boxes[2].attributes('disabled')).toBeDefined()
    expect(rows[2].text()).toContain('下載尚未完成')
    expect(rows[1].get('details').text()).toContain('來源：國際版（com.bilibili.app.in）')
    expect(rows[1].get('.title').text()).not.toContain('國際版')
    const all = w.get('.toolbar input[type=checkbox]')
    await all.setValue(true)
    expect(w.text()).toContain('已選 1 部')
    await rows[1].get('select').setValue('1')
    expect(boxes[1].attributes('disabled')).toBeUndefined()
    await boxes[1].setValue(true)
    handlers.export = () => ({ total: 2, successful: 2, partial: 0, failed: 0, cancelled: 0, notStarted: 0 })
    const exportButton = w.get('button.primary')
    expect(exportButton.text()).toContain('（2）')
    await exportButton.trigger('click')
    await flushPromises()
    expect(calls.find(c => c.cmd === 'export')!.args).toEqual({ generation: 1, selection: [{ id: 0, variant: 0, danmaku: false }, { id: 1, variant: 1, danmaku: false }] })
    expect(w.text()).toContain('匯出完成')
  })

  it('marks rows whose MP4 is already in the output folder', async () => {
    const w = await start({ ...connected, items: [item(0, { exported: true }), item(1)] })
    const [done, fresh] = w.findAll('li.row')
    expect(done.get('.meta.accent.exported').text()).toBe('輸出資料夾已有同名影片，可能匯出過')
    expect(fresh.text()).not.toContain('同名影片')
  })

  it('only shows the danmaku count and unchecked export box for rows with danmaku', async () => {
    const w = await start({ ...connected, items: [item(0, { danmaku: 60, hasCover: false }), item(1)] })
    const [withDanmaku, withoutDanmaku] = w.findAll('li.row')
    expect(withDanmaku.get('.meta').text()).toContain('· 快取沒有封面')
    expect(withDanmaku.get('.danmaku-option').text()).toBe('匯出彈幕 · 60 則')
    expect((withDanmaku.get('[aria-label="匯出彈幕：影片 0"]').element as HTMLInputElement).checked).toBe(false)
    expect(withoutDanmaku.text()).not.toContain('彈幕')
    expect(withoutDanmaku.findAll('input[type=checkbox]')).toHaveLength(1)
    await withDanmaku.get('[aria-label="選取 影片 0"]').setValue(true)
    expect((withDanmaku.get('[aria-label="匯出彈幕：影片 0"]').element as HTMLInputElement).checked).toBe(false)
    handlers.export = () => ({ total: 1, successful: 1, partial: 0, failed: 0, cancelled: 0, notStarted: 0 })
    await w.get('button.primary').trigger('click')
    await flushPromises()
    expect(calls.find(c => c.cmd === 'export')!.args).toEqual({ generation: 1, selection: [{ id: 0, variant: 0, danmaku: false }] })
  })

  it('keeps danmaku independent of video selection and sends the checked choice on export', async () => {
    const w = await start({ ...connected, items: [item(0, { danmaku: 60 }), item(1, { danmaku: 10 })] })
    const box = w.get('[aria-label="匯出彈幕：影片 0"]')
    const video = w.get('[aria-label="選取 影片 0"]')
    await box.setValue(true)
    expect((video.element as HTMLInputElement).checked).toBe(false)
    expect(w.get('button.primary').attributes('disabled')).toBeDefined()
    await video.setValue(true)
    await video.setValue(false)
    expect((box.element as HTMLInputElement).checked).toBe(true)
    await w.get('.toolbar input[type=checkbox]').setValue(true)
    handlers.export = () => ({ total: 2, successful: 2, partial: 0, failed: 0, cancelled: 0, notStarted: 0 })
    await w.get('button.primary').trigger('click')
    await flushPromises()
    expect(calls.find(c => c.cmd === 'export')!.args).toEqual({ generation: 1, selection: [{ id: 0, variant: 0, danmaku: true }, { id: 1, variant: 0, danmaku: false }] })
    await box.setValue(false)
    expect((video.element as HTMLInputElement).checked).toBe(true)
  })

  it('clears danmaku choices when a new scan replaces the list', async () => {
    const scanned = [item(0, { danmaku: 60 })]
    const w = await start({ ...connected, items: scanned })
    await w.get('[aria-label="匯出彈幕：影片 0"]').setValue(true)
    handlers.scan = () => ({ ...connected, items: scanned, generation: 2 })
    await w.findAll('button').find(b => b.text() === '重新掃描')!.trigger('click')
    await flushPromises()
    expect((w.get('[aria-label="匯出彈幕：影片 0"]').element as HTMLInputElement).checked).toBe(false)
    const all = w.get<HTMLInputElement>('[aria-label="全選彈幕"]')
    expect(all.element.checked).toBe(false)
    expect(all.element.indeterminate).toBe(false)
  })

  it('selects all available danmaku independently and reflects individual changes', async () => {
    const w = await start({ ...connected, items: [item(0, { danmaku: 60 }), item(1, { danmaku: 1280 }), item(2)] })
    const all = w.get<HTMLInputElement>('[aria-label="全選彈幕"]')
    const first = w.get<HTMLInputElement>('[aria-label="匯出彈幕：影片 0"]')
    const second = w.get<HTMLInputElement>('[aria-label="匯出彈幕：影片 1"]')
    expect(all.element.checked).toBe(false)
    expect(all.element.indeterminate).toBe(false)
    await first.setValue(true)
    expect(all.element.checked).toBe(false)
    expect(all.element.indeterminate).toBe(true)
    await all.setValue(true)
    expect(first.element.checked && second.element.checked).toBe(true)
    expect(all.element.indeterminate).toBe(false)
    expect(w.get('button.primary').attributes('disabled')).toBeDefined()
    expect(w.findAll<HTMLInputElement>('.row > input').every(b => !b.element.checked)).toBe(true)
    await w.get('.toolbar input[type=checkbox]').setValue(true)
    await all.setValue(false)
    expect(first.element.checked || second.element.checked).toBe(false)
    expect(all.element.indeterminate).toBe(false)
    expect(w.findAll<HTMLInputElement>('.row > input').every(b => b.element.checked)).toBe(true)
    await all.setValue(true)
    handlers.export = () => ({ total: 3, successful: 3, partial: 0, failed: 0, cancelled: 0, notStarted: 0 })
    await w.get('button.primary').trigger('click')
    await flushPromises()
    expect(calls.find(c => c.cmd === 'export')!.args).toEqual({ generation: 1, selection: [
      { id: 0, variant: 0, danmaku: true }, { id: 1, variant: 0, danmaku: true }, { id: 2, variant: 0, danmaku: false },
    ] })
  })

  it('disables bulk danmaku selection when none is available or while exporting', async () => {
    const empty = await start({ ...connected, items: [item(0)] })
    expect(empty.get('[aria-label="全選彈幕"]').attributes('disabled')).toBeDefined()
    empty.unmount()
    const w = await start({ ...connected, items: [item(0, { danmaku: 60 })] })
    let finish!: (value: unknown) => void
    handlers.export = () => new Promise(resolve => { finish = resolve })
    await w.get('[aria-label="選取 影片 0"]').setValue(true)
    await w.get('button.primary').trigger('click')
    expect(w.get('[aria-label="全選彈幕"]').attributes('disabled')).toBeDefined()
    expect(w.get('[aria-label="匯出彈幕：影片 0"]').attributes('disabled')).toBeDefined()
    finish({ total: 1, successful: 1, partial: 0, failed: 0, cancelled: 0, notStarted: 0 })
    await flushPromises()
    expect(w.get('[aria-label="全選彈幕"]').attributes('disabled')).toBeUndefined()
  })

  it('counts selected exportable videos with danmaku enabled in the toolbar', async () => {
    const w = await start({ ...connected, items: [item(0, { danmaku: 60 }), item(1, { danmaku: 1280 }), item(2), item(3, { danmaku: 500, issue: '下載尚未完成', autoVariant: null })] })
    const summary = w.get('.toolbar .muted')
    expect(summary.text()).toContain('已選 0 部影片的彈幕')
    await w.get('[aria-label="全選彈幕"]').setValue(true)
    expect(summary.text()).toContain('已選 0 部影片的彈幕')
    await w.get('[aria-label="選取 影片 0"]').setValue(true)
    expect(summary.text()).toContain('已選 1 部影片的彈幕')
    await w.get('.toolbar input[type=checkbox]').setValue(true)
    expect(summary.text()).toContain('已選 2 部影片的彈幕')
    await w.get('[aria-label="匯出彈幕：影片 0"]').setValue(false)
    expect(summary.text()).toContain('已選 1 部影片的彈幕')
    await w.get('[aria-label="選取 影片 1"]').setValue(false)
    expect(summary.text()).toContain('已選 0 部影片的彈幕')
    await w.get('[aria-label="選取 影片 1"]').setValue(true)
    await w.get('[aria-label="全選彈幕"]').setValue(true)
    await w.get('[aria-label="全選彈幕"]').setValue(false)
    expect(summary.text()).toContain('已選 0 部影片的彈幕')
  })

  it('keeps the danmaku label separate from cache details', async () => {
    const w = await start({ ...connected, items: [item(0, { danmaku: 1280, inventory: [{ path: '80/video.m4s', size: 1024, error: '' }] })] })
    const row = w.get('li.row')
    expect(row.get('.danmaku-option').text()).toContain((1280).toLocaleString() + ' 則')
    const details = row.get<HTMLDetailsElement>('details')
    expect(details.find('input').exists()).toBe(false)
    expect(row.get('.danmaku-option').find('summary').exists()).toBe(false)
    expect(row.get('.danmaku-option').element.nextElementSibling).toBe(details.element)
    await row.get('[aria-label="匯出彈幕：影片 0"]').setValue(true)
    expect(details.element.open).toBe(false)
    details.element.open = true
    expect(details.text()).toContain('80/video.m4s')
    expect(row.get<HTMLInputElement>('[aria-label="選取 影片 0"]').element.checked).toBe(false)
  })

  it('needs an explicit device when several are attached', async () => {
    handlers.connect = ({ serial }: any) => serial ? connected : { devices: [connected.devices[0], { serial: 'S2', state: 'unauthorized', detail: '' }], serial: null, items: [], generation: 1 }
    const w = await start()
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('請選擇要讀取的手機')
    const connect = w.findAll('button').find(b => b.text() === '連線')!
    expect(connect.attributes('disabled')).toBeDefined()
    await w.get('select[aria-label=選擇裝置]').setValue('S1')
    await connect.trigger('click')
    await flushPromises()
    expect(calls.filter(c => c.cmd === 'connect').map(c => c.args)).toEqual([{ serial: null }, { serial: 'S1' }])
    expect(w.findAll('li.row')).toHaveLength(3)
  })

  it('toggles and saves the light theme', async () => {
    const w = await start()
    expect(document.documentElement.dataset.theme).toBe('dark')
    await w.get('button[aria-label=切換淺色模式]').trigger('click')
    await flushPromises()
    expect((calls.find(c => c.cmd === 'save_settings')!.args as any).input.theme).toBe('light')
    expect(document.documentElement.dataset.theme).toBe('light')
  })

  it('opens settings, returns, and expands logs', async () => {
    handlers.check_tools = () => [{ kind: 'ADB', path: 'x', ok: false, message: '找不到 ADB' }]
    const w = await start()
    await w.findAll('button').find(b => b.text().includes('設定'))!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('必要工具與位置')
    expect(w.text()).toContain('找不到 ADB')
    await w.findAll('button').find(b => b.text() === '返回影片')!.trigger('click')
    expect(w.text()).toContain('輸出資料夾')
    await w.findAll('button').find(b => b.text().includes('詳細記錄'))!.trigger('click')
    expect(w.find('pre.logs').exists()).toBe(true)
  })

  it('startup check prompts only for a newer, not skipped release', async () => {
    const release = { version: '2.1.0', notes: '- 新功能\n- 修正', newer: true }
    handlers.check_update = () => release
    let w = await start()
    const dialog = w.get('[role=dialog]')
    expect(dialog.text()).toContain('有新版本 2.1.0')
    expect(dialog.get('pre.notes').text()).toBe(release.notes)
    await dialog.findAll('button').find(b => b.text() === '稍後')!.trigger('click')
    expect(w.find('[role=dialog]').exists()).toBe(false)
    w.unmount()

    handlers.skip_version = ({ version }: any) => ({ ...snapshot().settings, SkipVersion: version })
    w = await start()
    await w.get('[role=dialog]').findAll('button').find(b => b.text() === '略過此版本')!.trigger('click')
    await flushPromises()
    expect(calls.find(c => c.cmd === 'skip_version')!.args).toEqual({ version: '2.1.0' })
    expect(w.find('[role=dialog]').exists()).toBe(false)
    w.unmount()

    w = await start({ settings: { ...snapshot().settings, SkipVersion: '2.1.0' } })
    expect(w.find('[role=dialog]').exists()).toBe(false)
    handlers.check_tools = () => []
    await w.findAll('button').find(b => b.text().includes('設定'))!.trigger('click')
    await w.findAll('button').find(b => b.text() === '立即檢查')!.trigger('click')
    await flushPromises()
    expect(w.get('[role=dialog]').text()).toContain('2.1.0')
    w.unmount()

    handlers.check_update = () => ({ ...release, version: '2.2.0' })
    w = await start({ settings: { ...snapshot().settings, SkipVersion: '2.1.0' } })
    expect(w.get('[role=dialog]').text()).toContain('2.2.0')
  })

  it('installs an update with progress, and offers the release page on failure', async () => {
    handlers.check_update = () => ({ version: '2.1.0', notes: '', newer: true })
    let finish!: (v: unknown) => void
    handlers.install_update = () => new Promise(r => { finish = r })
    const w = await start()
    await w.get('[role=dialog]').findAll('button').find(b => b.text() === '更新')!.trigger('click')
    await flushPromises()
    expect(calls.find(c => c.cmd === 'install_update')!.args).toEqual({ version: '2.1.0' })
    expect(w.find('[role=dialog] progress').exists()).toBe(true)
    expect(w.get('[role=dialog]').findAll('button')).toHaveLength(0)
    expect(w.text()).toContain('更新中')
    finish({ ignored: true })
    handlers.install_update = () => { throw { message: '下載檔案的 SHA-256 與發行記錄不符', cancelled: false, missing: [] } }
    await flushPromises()
    await w.get('[role=dialog]').findAll('button').find(b => b.text() === '更新')!.trigger('click')
    await flushPromises()
    const dialog = w.get('[role=dialog]')
    expect(dialog.text()).toContain('SHA-256 與發行記錄不符')
    await dialog.findAll('button').find(b => b.text() === '開啟 Release 頁面')!.trigger('click')
    expect(calls.find(c => c.cmd === 'open_url')!.args).toEqual({ kind: 'releases' })
    expect(dialog.text()).toContain('重試更新')
  })

  it('update waits while an export runs', async () => {
    handlers.check_update = () => ({ version: '2.1.0', notes: '', newer: true })
    handlers.export = () => new Promise(() => {})
    const w = await start()
    await w.get('[role=dialog]').findAll('button').find(b => b.text() === '稍後')!.trigger('click')
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    await w.get('.toolbar input[type=checkbox]').setValue(true)
    await w.get('button.primary').trigger('click')
    await flushPromises()
    handlers.check_tools = () => []
    await w.findAll('button').find(b => b.text().includes('設定'))!.trigger('click')
    await w.findAll('button').find(b => b.text() === '立即檢查')!.trigger('click')
    await flushPromises()
    const dialog = w.get('[role=dialog]')
    expect(dialog.text()).toContain('匯出完成後才能更新')
    expect(dialog.findAll('button').find(b => b.text() === '更新')!.attributes('disabled')).toBeDefined()
  })

  it('startup check is silent when off, up to date or offline', async () => {
    calls.length = 0
    await start({ settings: { ...snapshot().settings, CheckUpdates: false } })
    expect(calls.some(c => c.cmd === 'check_update')).toBe(false)
    handlers.check_update = () => ({ version: '2.0.0', notes: '', newer: false })
    expect((await start()).find('[role=dialog]').exists()).toBe(false)
    handlers.check_update = () => { throw { message: '檢查更新失敗：offline', cancelled: false, missing: [] } }
    const w = await start()
    expect(w.find('[role=dialog]').exists()).toBe(false)
    expect(w.find('[role=alert]').exists()).toBe(false)
  })

  it('applies and saves a preset accent', async () => {
    handlers.check_tools = () => []
    const w = await start()
    expect(document.documentElement.dataset.accent).toBe('blue')
    await w.findAll('button').find(b => b.text().includes('設定'))!.trigger('click')
    await flushPromises()
    const swatches = w.findAll('[role=radiogroup] [role=radio]')
    expect(swatches.map(b => b.attributes('aria-label'))).toEqual(['藍色', 'B 站粉', '綠色', '紫色', '橘色', '青色'])
    expect(swatches[0].attributes('aria-checked')).toBe('true')
    await swatches[1].trigger('click')
    await flushPromises()
    expect((calls.find(c => c.cmd === 'save_settings')!.args as any).input).toMatchObject({ accent: 'pink', theme: 'dark' })
    expect(document.documentElement.dataset.accent).toBe('pink')
    expect(w.get('[aria-label="B 站粉"]').attributes('aria-checked')).toBe('true')
  })

  it('full verification is off by default, explained by a hint and saved when turned on', async () => {
    handlers.check_tools = () => []
    const w = await start()
    await w.findAll('button').find(b => b.text().includes('設定'))!.trigger('click')
    await flushPromises()
    const label = w.findAll('label.setting').find(l => l.text().includes('匯出後完整驗證影音'))!
    const toggle = label.get('input')
    expect((toggle.element as HTMLInputElement).checked).toBe(false)
    expect(toggle.attributes('aria-describedby')).toBe('full-decode-tip')
    expect(label.get('#full-decode-tip').text()).toContain('變慢')
    await label.get('.hint').trigger('click')
    expect(calls.some(c => c.cmd === 'save_settings')).toBe(false)
    await toggle.setValue(true)
    await flushPromises()
    expect((calls.filter(c => c.cmd === 'save_settings').pop()!.args as any).input.fullDecode).toBe(true)
  })

  it('settings has one way back and an update card', async () => {
    handlers.check_tools = () => []
    handlers.check_update = () => ({ version: '2.1.0', notes: '', newer: true })
    const w = await start()
    await w.findAll('button').find(b => b.text().includes('設定'))!.trigger('click')
    await flushPromises()
    expect(w.findAll('button').filter(b => b.text().includes('返回影片'))).toHaveLength(1)
    expect(w.text()).toContain('目前版本 2.0.0')
    expect(w.text()).not.toContain('2.0.0+test')
    await w.findAll('button').find(b => b.text() === '立即檢查')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('發現新版本 2.1.0')
    handlers.check_update = () => ({ version: '2.0.0', notes: '', newer: false })
    await w.findAll('button').find(b => b.text() === '立即檢查')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('已是最新版本')
    handlers.check_update = () => { throw { message: '檢查更新失敗：offline', cancelled: false, missing: [] } }
    await w.findAll('button').find(b => b.text() === '立即檢查')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('檢查更新失敗：offline')
    const toggle = w.findAll('label.setting').find(l => l.text().includes('啟動時檢查更新'))!.get('input')
    await toggle.setValue(false)
    await flushPromises()
    expect((calls.filter(c => c.cmd === 'save_settings').pop()!.args as any).input.checkUpdates).toBe(false)
    await w.findAll('button').find(b => b.text() === 'GitHub 專案頁面')!.trigger('click')
    expect(calls.find(c => c.cmd === 'open_url')!.args).toEqual({ kind: 'project' })
  })

  it('empty phone list offers rescan; scanning offers cancel', async () => {
    let finish!: (v: unknown) => void
    handlers.connect = () => new Promise(r => { finish = r })
    const w = await start()
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    await w.findAll('button').find(b => b.text() === '取消')!.trigger('click')
    expect(calls.some(c => c.cmd === 'cancel')).toBe(true)
    finish({ ...connected, items: [] })
    await flushPromises()
    expect(w.text()).toContain('這部手機沒有 Bilibili 快取影片')
    handlers.scan = () => ({ ...connected, items: [], generation: 2 })
    await w.findAll('button').find(b => b.text() === '重新掃描')!.trigger('click')
    await flushPromises()
    expect(calls.some(c => c.cmd === 'scan')).toBe(true)
  })

  it('a re-sent row during scanning replaces the earlier one', async () => {
    let finish!: (v: unknown) => void
    handlers.connect = () => new Promise(r => { finish = r })
    const w = await start()
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    emit({ type: 'item', item: item(0) })
    emit({ type: 'item', item: item(0, { title: '合併後' }) })
    await flushPromises()
    expect(w.findAll('li.row').map(r => r.get('.title').text())).toEqual(['合併後'])
    finish({ ...connected, items: [] })
  })

  it('quality menu shows the automatically chosen version', async () => {
    handlers.connect = () => ({ ...connected, items: [item(0, { variants: [variant('80'), variant('64', '缺少 video.m4s 或 audio.m4s')], autoVariant: 0 })] })
    const w = await start()
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    expect((w.get('li.row select').element as HTMLSelectElement).value).toBe('0')
    expect(w.get('li.row input[type=checkbox]').attributes('disabled')).toBeUndefined()
  })

  it('rows and the quality menu show the short label; cache details keep the raw metadata', async () => {
    const raw = '80（快取目錄） · bandwidth=102599, height=0, width=0, height=1080, width=1920'
    handlers.connect = () => ({ ...connected, items: [
      item(0, { variants: [variant(raw, '', '1920×1080')], quality: '1920×1080', inventory: [{ path: '80/index.json', size: 1, error: '' }] }),
      item(1, { variants: [variant(raw, '', '1920×1080'), variant('64（快取目錄）', '', '解析度未知')], autoVariant: null, needsChoice: true, quality: '2 個版本（需選擇畫質）' }),
    ] })
    const w = await start()
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    const [single, multi] = w.findAll('li.row')
    expect(single.get('.meta').text()).toContain('1920×1080')
    expect(single.get('.meta').text()).not.toContain('80 ·')
    expect(single.get('.meta').text()).not.toContain('bandwidth=')
    expect(single.get('details').text()).toContain('bandwidth=102599')
    const options = multi.findAll('option').slice(1).map(o => o.text())
    expect(options[0]).toContain('80 · 1920×1080')
    expect(options[1]).toContain('64 · 解析度未知')
    expect(options.join()).not.toContain('bandwidth=')
  })

  it('the update prompt waits while the tool download dialog is open', async () => {
    handlers.check_update = () => ({ version: '2.1.0', notes: '', newer: true })
    handlers.connect = () => { throw { message: '缺少必要工具：ADB', cancelled: false, missing: ['adb'] } }
    const w = await start()
    expect(w.get('[role=dialog]').text()).toContain('有新版本 2.1.0')
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    expect(w.findAll('[role=dialog]')).toHaveLength(1)
    expect(w.get('[role=dialog]').text()).toContain('37.0.1')
    await w.get('.app').trigger('keydown', { key: 'Escape' })
    expect(w.findAll('[role=dialog]')).toHaveLength(1)
    expect(w.get('[role=dialog]').text()).toContain('有新版本 2.1.0')
  })

  it('a late, older tool check does not overwrite a newer result', async () => {
    let releaseFirst!: (v: unknown) => void
    let n = 0
    handlers.check_tools = () => (++n === 1 ? new Promise(r => { releaseFirst = r }).then(() => [{ kind: 'ADB', path: 'x', ok: false, message: '找不到 ADB' }]) : [{ kind: 'ADB', path: 'x', ok: true, message: 'Android Debug Bridge version 1.0.41' }])
    const w = await start()
    await w.findAll('button').find(b => b.text().includes('設定'))!.trigger('click')
    await w.findAll('button').find(b => b.text() === '儲存設定')!.trigger('click')
    await flushPromises()
    releaseFirst(null)
    await flushPromises()
    expect(w.text()).toContain('Android Debug Bridge')
    expect(w.text()).not.toContain('找不到 ADB')
  })

  it('missing tools open the download guide with real package facts and retry', async () => {
    handlers.connect = () => { throw { message: '缺少必要工具：ADB', cancelled: false, missing: ['adb'] } }
    handlers.download_tools = () => { throw { message: '下載失敗：offline', cancelled: false, missing: [] } }
    const w = await start()
    await w.findAll('button').find(b => b.text() === '連線')!.trigger('click')
    await flushPromises()
    const dialog = w.get('[role=dialog]')
    expect(dialog.text()).toContain('37.0.1')
    expect(dialog.text()).toContain('7.7 MB')
    await dialog.findAll('button').find(b => b.text() === '下載並設定')!.trigger('click')
    await flushPromises()
    expect(w.get('[role=dialog]').text()).toContain('下載失敗：offline')
    expect(w.get('[role=dialog]').text()).toContain('重試下載')
    expect(w.get('[role=dialog]').text()).toContain('指定已有工具')
  })

  it('shows reset for corrupt settings and blocks actions', async () => {
    const w = await start({ settingsError: '設定檔損壞，無法讀取' })
    expect(w.text()).toContain('備份並重設')
    expect(w.findAll('button').find(b => b.text() === '連線')!.attributes('disabled')).toBeDefined()
  })
})

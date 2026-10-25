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
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => null) }))

import App from './App.vue'

const variant = (q: string, issue = '') => ({ video: 'v', audio: 'a', videoSize: 1048576, audioSize: 1048576, quality: q, issue })
const item = (id: number, extra: Partial<CacheItem> = {}): CacheItem => ({
  id, title: `影片 ${id}`, remote: `/r/${id}`, variants: [variant('80（快取目錄）')], autoVariant: 0, needsChoice: false, issue: '', quality: '80（快取目錄）', cover: null, hasCover: true, inventory: [], ...extra,
})
const items = [
  item(0),
  item(1, { title: '多畫質', variants: [variant('80'), variant('116')], autoVariant: null, needsChoice: true, quality: '2 個版本（需選擇畫質）' }),
  item(2, { title: '未完成', issue: '下載尚未完成', autoVariant: null }),
]

function snapshot(extra: Partial<Snapshot> = {}): Snapshot {
  return {
    version: '2.0.0+test', settings: { Adb: 'tools\\adb\\adb.exe', Ffmpeg: 'f', Ffprobe: 'p', Output: 'D:\\Out', Temp: '', Theme: 'dark' },
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
  handlers = { connect: () => connected, save_settings: ({ input }: any) => ({ Adb: input.adb, Ffmpeg: input.ffmpeg, Ffprobe: input.ffprobe, Output: input.output, Temp: input.temp, Theme: input.theme }) }
  document.body.innerHTML = ''
})

describe('compact list', () => {
  it('starts without device queries or sample data', async () => {
    const w = await start()
    expect(calls.map(c => c.cmd)).toEqual(['snapshot', 'ready'])
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
    expect(calls.find(c => c.cmd === 'export')!.args).toEqual({ generation: 1, selection: [{ id: 0, variant: 0 }, { id: 1, variant: 1 }] })
    expect(w.text()).toContain('匯出完成')
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

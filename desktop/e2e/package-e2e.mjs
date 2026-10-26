// End-to-end check of the packaged app: fake phone (test-only fake adb) + real FFmpeg/FFprobe.
// Drives the real WebView2 window over the DevTools protocol. Usage:
//   node e2e/package-e2e.mjs <BiliCacheExporter.exe> <fake_adb.exe> <ffmpeg.exe> <work dir> [--with-download]
import { spawn, execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const [exe, fakeAdb, ffmpeg, work] = process.argv.slice(2)
const withDownload = process.argv.includes('--with-download')
const ffprobe = path.join(path.dirname(ffmpeg), 'ffprobe.exe')
const evidence = path.join(work, 'evidence')
fs.mkdirSync(evidence, { recursive: true })
const results = []
const check = (ok, name, detail = '') => {
  results.push({ ok: !!ok, name, detail })
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ' — ' + detail : ''}`)
}
const sleep = ms => new Promise(r => setTimeout(r, ms))
const ROOT = 'sdcard/Android/data/com.bilibili.app.in/download'
const run = (file, args) => execFileSync(file, args, { stdio: ['ignore', 'pipe', 'pipe'] }).toString()

// ---------- fixtures ----------
function makePhone(dir) {
  const media = path.join(work, 'media')
  fs.mkdirSync(media, { recursive: true })
  const ff = args => run(ffmpeg, ['-hide_banner', '-nostdin', '-v', 'error', '-y', ...args])
  ff(['-f', 'lavfi', '-i', 'testsrc2=size=320x180:rate=10', '-t', '2', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-movflags', '+frag_keyframe+empty_moov', '-f', 'mp4', path.join(media, 'video.m4s')])
  ff(['-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000', '-t', '2', '-c:a', 'aac', '-movflags', '+frag_keyframe+empty_moov', '-f', 'mp4', path.join(media, 'audio.m4s')])
  ff(['-f', 'lavfi', '-i', 'testsrc=size=640x360', '-frames:v', '1', path.join(media, 'cover.jpg')])
  const add = (id, entry, qualities, cover) => {
    const remote = path.join(dir, ROOT, id)
    fs.mkdirSync(remote, { recursive: true })
    fs.writeFileSync(path.join(remote, 'entry.json'), JSON.stringify(entry))
    for (const q of qualities) {
      fs.mkdirSync(path.join(remote, q), { recursive: true })
      for (const f of ['video.m4s', 'audio.m4s']) fs.copyFileSync(path.join(media, f), path.join(remote, q, f))
      fs.writeFileSync(path.join(remote, q, 'index.json'), JSON.stringify({ video: [{ width: 320, height: 180, codecs: 'avc1.64000d' }] }))
    }
    if (cover) fs.copyFileSync(path.join(media, 'cover.jpg'), path.join(remote, 'cover.jpg'))
  }
  add('100/c_1', { title: '非常長的中文標題：京都散步｜雨天的鴨川與老街，從清晨一路走到夜晚的完整紀錄（含 Unicode 🎬 與 Windows 非法字元 ?*）', is_completed: true, page_data: { part: '第一集 開場' } }, ['80'], true)
  add('200/c_2', { title: '多畫質影片', is_completed: true, page_data: { part: 'P2' } }, ['80', '116'], true)
  add('300/c_3', { title: '尚未下載完成', is_completed: false }, ['80'], true)
  add('400/c_4', { title: '沒有封面的影片', is_completed: true }, ['64'], false)
  fs.writeFileSync(path.join(dir, 'devices.txt'), 'List of devices attached\nFAKE-SERIAL-0001\tdevice product:x model:SM_S9260 device:y\n')
}

function makeFolder(name, { tools }) {
  const dir = path.join(work, name)
  fs.rmSync(dir, { recursive: true, force: true })
  fs.mkdirSync(dir, { recursive: true })
  fs.copyFileSync(exe, path.join(dir, 'BiliCacheExporter.exe'))
  const settings = { Output: path.join(dir, 'out'), Temp: '', Theme: 'dark', Ffmpeg: ffmpeg, Ffprobe: ffprobe, Adb: '' }
  if (tools) {
    const adbDir = path.join(dir, 'tools', 'adb')
    fs.mkdirSync(path.join(adbDir, 'fake-phone'), { recursive: true })
    fs.copyFileSync(fakeAdb, path.join(adbDir, 'adb.exe'))
    makePhone(path.join(adbDir, 'fake-phone'))
    settings.Adb = 'tools\\adb\\adb.exe'
  }
  // v1-style file (no SettingsVersion) exercises the upgrade path in the packaged app.
  fs.writeFileSync(path.join(dir, 'settings.json'), JSON.stringify(settings))
  return dir
}

// ---------- app driver ----------
let port = 9330
const requests = []
async function launch(dir, env = {}) {
  port += 1
  const myPort = port
  const child = spawn(path.join(dir, 'BiliCacheExporter.exe'), [], {
    cwd: dir,
    env: { ...process.env, ...env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` },
    stdio: 'ignore',
  })
  let target
  for (let i = 0; i < 100 && !target; i++) {
    await sleep(200)
    try {
      target = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find(t => t.type === 'page')
    } catch {}
  }
  if (!target) throw new Error('DevTools target not found')
  const ws = new WebSocket(target.webSocketDebuggerUrl)
  await new Promise(r => ws.addEventListener('open', r, { once: true }))
  let id = 0
  const pending = new Map()
  ws.addEventListener('message', m => {
    const msg = JSON.parse(m.data)
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg)
      pending.delete(msg.id)
    }
    if (msg.method === 'Network.requestWillBeSent') requests.push(msg.params.request.url)
  })
  const send = (method, params = {}) => new Promise(r => { id += 1; pending.set(id, r); ws.send(JSON.stringify({ id, method, params })) })
  await send('Network.enable')
  // Reloading while still on about:blank would cancel the app's first navigation, so wait for it to finish first.
  for (let i = 0; i < 100; i++) {
    const r = await send('Runtime.evaluate', { expression: "location.href.startsWith('http://tauri.localhost') && document.readyState === 'complete'", returnByValue: true })
    if (r.result?.result?.value) break
    await sleep(100)
  }
  // Reload once so the page's own resource loads are recorded too.
  await send('Runtime.evaluate', { expression: 'window.__beforeReload = true' })
  await send('Page.reload')
  const app = {
    child,
    send,
    async eval(expression) {
      const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
      if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails))
      return r.result?.result?.value
    },
    async waitFor(expression, ms = 20000) {
      const end = Date.now() + ms
      while (Date.now() < end) {
        if (await app.eval(expression).catch(() => false)) return true
        await sleep(150)
      }
      return false
    },
    click: text => app.eval(`(() => { const b = [...document.querySelectorAll('button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)}) && !b.disabled); if (b) b.click(); return !!b })()`),
    text: () => app.eval('document.body.innerText'),
    async shot(name) {
      const r = await send('Page.captureScreenshot', { format: 'png' })
      fs.writeFileSync(path.join(evidence, name + '.png'), Buffer.from(r.result.data, 'base64'))
    },
    async key(key, code, keyCode, text) {
      await send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: keyCode, ...(text ? { text } : {}) })
      await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: keyCode })
    },
    async close() {
      ws.close()
      child.kill()
      await new Promise(r => child.once('exit', r))
      // WebView2 refuses a new instance on the same profile while the old browser (other debug port) still runs.
      for (let i = 0; i < 100; i++) {
        try { await fetch(`http://127.0.0.1:${myPort}/json/version`) } catch { break }
        await sleep(100)
      }
      await sleep(300)
    },
  }
  // Settings arrive after first render (the real window stays hidden until then); wait like a user would.
  await app.waitFor("!window.__beforeReload && !!document.querySelector('input[aria-label=輸出資料夾]')?.value")
  return app
}

const calls = dir => (fs.existsSync(path.join(dir, 'tools/adb/fake-phone/calls.log')) ? fs.readFileSync(path.join(dir, 'tools/adb/fake-phone/calls.log'), 'utf8').trim().split('\n').filter(Boolean).map(l => JSON.parse(l)) : [])
const outputs = dir => (fs.existsSync(path.join(dir, 'out')) ? fs.readdirSync(path.join(dir, 'out')).filter(n => n.endsWith('.mp4') && !n.startsWith('.')).sort() : [])
const probe = file => JSON.parse(run(ffprobe, ['-v', 'error', '-show_streams', '-of', 'json', file])).streams

// ---------- A: missing tool guidance (and optional real in-app download) ----------
{
  const dir = makeFolder('missing-tools', { tools: false })
  const app = await launch(dir)
  await app.click('連線')
  check(await app.waitFor("document.querySelector('[role=dialog]')?.innerText.includes('37.0.1')"), 'Missing ADB opens download guide with pinned version/source/size', '')
  const dialog = await app.eval("document.querySelector('[role=dialog]').innerText")
  check(dialog.includes('Google') && dialog.includes('7.7 MB') && !dialog.includes('FFmpeg 與 FFprobe'), 'Guide lists only the missing package with real size')
  await app.shot('01-missing-tool-dialog-dark')
  if (withDownload) {
    await app.click('下載並設定')
    check(await app.waitFor("document.body.innerText.includes('工具已下載、驗證並設定完成')", 180000), 'Real in-app ADB download, hash check and activation from the package')
    const saved = JSON.parse(fs.readFileSync(path.join(dir, 'settings.json'), 'utf8'))
    check(saved.Adb === 'tools\\adb\\adb.exe' && fs.existsSync(path.join(dir, 'tools/adb/AdbWinApi.dll')) && !fs.existsSync(path.join(dir, 'AdbWinApi.dll')), 'Downloaded ADB stored portably under tools\\adb with its DLLs', saved.Adb)
    const statuses = await app.eval("(async () => (await window.__TAURI_INTERNALS__.invoke('check_tools')).map(t => t.kind + ':' + t.ok))()")
    check(statuses?.[0] === 'ADB:true', 'Downloaded ADB passes the shared tool validation', JSON.stringify(statuses))
  }
  await app.close()
}

// ---------- B: main flows on the fake phone ----------
const dir = makeFolder('portable', { tools: true })
let app = await launch(dir)
await sleep(3000)
check(calls(dir).length === 0, 'Startup and 3 s idle make no ADB calls', `${calls(dir).length} calls`)
check(await app.eval("document.documentElement.dataset.theme") === 'dark', 'Default theme is dark')
check(!(await app.text()).match(/Pixel 7|旅途日記|原型|模擬/), 'No prototype sample data or scenario controls')
await app.shot('02-empty-dark')

// Keyboard: Tab to 連線 and press Enter.
await app.eval('document.activeElement?.blur(); document.body.focus()')
let reached = false
for (let i = 0; i < 12 && !reached; i++) {
  await app.key('Tab', 'Tab', 9)
  reached = await app.eval("document.activeElement?.textContent?.trim() === '連線'")
}
check(reached, 'Keyboard Tab reaches the 連線 button')
await app.key('Enter', 'Enter', 13, String.fromCharCode(13))
check(await app.waitFor("document.querySelectorAll('li.row').length === 4"), 'Connect by keyboard scans automatically and lists 4 rows')
const deviceCalls = calls(dir).filter(c => c[0] === 'devices').length
check(deviceCalls === 1, 'Exactly one device listing for the connect action', String(deviceCalls))
check(await app.eval("[...document.querySelectorAll('li.row input[type=checkbox]')].every(b => !b.checked)"), 'All rows start unselected')
check(await app.eval("document.querySelectorAll('li.row img.thumb').length === 3"), 'Real covers shown for rows that have them')
const rowsText = await app.eval("[...document.querySelectorAll('li.row')].map(r => r.innerText)")
check(rowsText.some(t => t.includes('下載尚未完成')) && rowsText.some(t => t.includes('請先選擇畫質版本')), 'Incomplete and needs-quality rows show reasons')
check(await app.eval("document.documentElement.scrollWidth <= window.innerWidth"), 'Long title does not cause horizontal overflow')
await app.shot('03-list-dark')

// Keyboard quality choice on the multi-quality row (native select, arrow key).
await app.eval(`document.querySelector('select[aria-label^="畫質"]').focus()`)
await app.key('ArrowDown', 'ArrowDown', 40)
check(await app.eval(`document.querySelector('select[aria-label^="畫質"]').value !== ''`), 'Quality can be chosen with the keyboard')
// Quality choice + select all + export.
await app.eval(`(() => { const s = document.querySelector('select[aria-label^="畫質"]'); const o = [...s.options].find(o => o.text.startsWith('116')); s.value = o.value; s.dispatchEvent(new Event('change')); })()`)
await app.eval("document.querySelector('.toolbar input[type=checkbox]').click()")
check(await app.eval("document.querySelectorAll('li.row input[type=checkbox]:checked').length === 3"), 'Select all covers only the 3 exportable rows')
await app.click('匯出所選')
check(await app.waitFor("document.querySelector('.progress-box')?.innerText.includes('匯出')", 5000), 'Export shows real stages')
await app.shot('04-exporting-dark')
check(await app.waitFor("document.querySelector('.progress-box')?.innerText.includes('匯出結束') || document.querySelector('.progress-box')?.innerText.includes('匯出完成')", 120000), 'Batch export finishes')
await app.shot('05-export-result-dark')
const files = outputs(dir)
check(files.length === 3, 'Three MP4 files written', files.join(' | '))
const long = files.find(f => f.startsWith('非常長'))
const longStreams = probe(path.join(dir, 'out', long))
check(longStreams.some(s => s.codec_name === 'h264' && !s.disposition.attached_pic) && longStreams.some(s => s.codec_name === 'aac') && longStreams.some(s => s.disposition.attached_pic === 1 && s.width === 640), 'Long Unicode title exported with H.264/AAC and embedded 640x360 cover', long)
check(!long.includes('?') && !long.includes('*'), 'Illegal characters replaced in file name')
const multi = files.find(f => f.startsWith('多畫質'))
const pulls = calls(dir).filter(c => c[2] === 'pull').map(c => c[3])
check(pulls.some(p => p.includes('200/c_2/116/video.m4s')) && !pulls.some(p => p.includes('200/c_2/80/video.m4s')) && !pulls.some(p => p.includes('300/c_3/80/')), 'Only selected quality and items transferred', multi)
check(calls(dir).filter(c => c[2] === 'shell').every(c => /^(pm list|find |stat -c|cat )/.test(c[3])), 'Phone only received read-only commands')
const resultText = await app.text()
check(resultText.includes('部分完成') && resultText.includes('手機快取中沒有封面圖片'), 'Missing cover reported as partial with reason')
const extracted = path.join(work, 'extracted.jpg')
run(ffmpeg, ['-hide_banner', '-nostdin', '-v', 'error', '-y', '-i', path.join(dir, 'out', long), '-map', '0:v:1', '-c', 'copy', '-f', 'image2', extracted])
check(Buffer.compare(fs.readFileSync(extracted), fs.readFileSync(path.join(work, 'media', 'cover.jpg'))) === 0, 'Embedded cover equals the original bytes')

// Cancel during transfer, then manual retry.
fs.writeFileSync(path.join(dir, 'tools/adb/fake-phone/pull-delay-ms'), '1500')
await app.eval("document.querySelector('.toolbar input[type=checkbox]').click()") // clear all
await app.eval("document.querySelector('li.row input[type=checkbox]').click()")
await app.click('匯出所選')
await app.waitFor("document.querySelector('.progress-box')?.innerText.includes('傳輸')", 10000)
await app.click('取消')
check(await app.waitFor("document.body.innerText.includes('已取消')", 20000), 'Cancel stops the running export')
check(outputs(dir).length === 3 && fs.readdirSync(path.join(dir, 'out', '.bili-temp')).length === 0, 'Cancel keeps earlier outputs and cleans its temp')
await app.shot('06-cancelled-dark')
fs.writeFileSync(path.join(dir, 'tools/adb/fake-phone/pull-delay-ms'), '0')
check(await app.click('重試未完成項目'), 'Retry button offered for cancelled/partial items')
check(await app.waitFor("document.querySelector('.progress-box')?.innerText.includes('匯出結束') || document.querySelector('.progress-box')?.innerText.includes('匯出完成')", 120000), 'Retry finishes')
check(outputs(dir).length >= 4 && outputs(dir).some(f => f.includes('(1)')), 'Retry adds numbered files and never overwrites', outputs(dir).join(' | '))

// Theme, settings page, logs, narrow viewport.
await app.eval("document.querySelector('button[aria-label=切換淺色模式]').click()")
check(await app.waitFor("document.documentElement.dataset.theme === 'light'"), 'Light theme toggles')
await app.shot('07-list-light')
await app.eval("[...document.querySelectorAll('button')].find(b => b.textContent.includes('詳細記錄')).click()")
const logs = await app.eval("document.querySelector('pre.logs').innerText")
check(logs.length > 0 && !logs.includes('FAKE-SERIAL-0001') && logs.includes('[device]'), 'Detail log visible and device serial redacted')
await app.shot('08-logs-light')
await app.click('設定')
check(await app.waitFor("document.body.innerText.includes('必要工具與位置')"), 'Settings page opens')
await app.waitFor("document.body.innerText.includes('Android Debug Bridge')", 10000)
await app.shot('09-settings-light')
await app.send('Emulation.setDeviceMetricsOverride', { width: 420, height: 800, deviceScaleFactor: 0, mobile: false })
await sleep(300)
check(await app.eval("document.documentElement.scrollWidth <= window.innerWidth"), 'Settings fit a 420 px wide viewport without horizontal scroll')
await app.shot('10-settings-narrow-light')
await app.click('返回影片')
await sleep(200)
check(await app.eval("document.documentElement.scrollWidth <= window.innerWidth"), 'List fits a 420 px wide viewport')
await app.shot('11-list-narrow-light')
await app.send('Emulation.clearDeviceMetricsOverride')
const dpr = await app.eval('window.devicePixelRatio')
await app.close()

// Simulate a crash leftover: ownership record + marker with no live lock.
{
  const id = '0123456789abcdef0123456789abcdef'
  const tempRoot = path.join(dir, 'out', '.bili-temp')
  fs.mkdirSync(path.join(tempRoot, 'bili-export-' + id), { recursive: true })
  fs.writeFileSync(path.join(tempRoot, 'bili-export-' + id, '.bili-owned'), id)
  fs.writeFileSync(path.join(tempRoot, 'bili-export-' + id, 'video.m4s'), 'leftover')
  fs.mkdirSync(path.join(dir, '.bili-jobs'), { recursive: true })
  fs.writeFileSync(path.join(dir, '.bili-jobs', id + '.json'), JSON.stringify({ Id: id, TempRoot: tempRoot, OutputRoot: path.join(dir, 'out') }))
}
// Restart: theme persisted, still no query at startup.
const before = calls(dir).length
app = await launch(dir)
await sleep(2000)
check(calls(dir).length === before, 'Restart makes no ADB calls')
check(await app.waitFor("document.body.innerText.includes('找到 1 個先前未完成作業')"), 'Startup offers cleanup of crash leftovers')
await app.shot('12a-recover-prompt-light')
await app.click('清理')
check(await app.waitFor("document.body.innerText.includes('已清理 1 個殘留暫存')") && !fs.existsSync(path.join(dir, 'out', '.bili-temp', 'bili-export-0123456789abcdef0123456789abcdef')) && outputs(dir).length >= 4, 'Cleanup removes only the owned leftover and keeps outputs')
// Keyboard: open settings and return.
const tabTo = async label => {
  await app.eval('document.activeElement?.blur()')
  for (let i = 0; i < 20; i++) {
    await app.key('Tab', 'Tab', 9)
    if (await app.eval(`document.activeElement?.textContent?.trim() === ${JSON.stringify(label)}`)) return true
  }
  return false
}
check(await tabTo('設定'), 'Keyboard reaches 設定')
await app.key('Enter', 'Enter', 13, String.fromCharCode(13))
check(await app.waitFor("document.body.innerText.includes('必要工具與位置')"), 'Settings opens by keyboard')
check(await tabTo('返回影片'), 'Keyboard reaches 返回影片')
await app.key('Enter', 'Enter', 13, String.fromCharCode(13))
check(await app.waitFor("document.querySelector('.device') !== null && !document.body.innerText.includes('必要工具與位置')"), 'Returns to the list by keyboard')
check(await app.eval("document.documentElement.dataset.theme") === 'light', 'Theme persists across restart')
const saved = JSON.parse(fs.readFileSync(path.join(dir, 'settings.json'), 'utf8'))
check(saved.SettingsVersion === 2 && fs.existsSync(path.join(dir, 'settings.v1-backup.json')), 'v1 settings upgraded with original backup')

// Unplug: export is refused before transfer; reconnect does not resume.
await app.click('連線')
await app.waitFor("document.querySelectorAll('li.row').length === 4")
await app.eval("document.querySelector('li.row input[type=checkbox]').click()")
fs.writeFileSync(path.join(dir, 'tools/adb/fake-phone/devices.txt'), 'List of devices attached\n')
const pullsBefore = calls(dir).filter(c => c[2] === 'pull').length
await app.click('匯出所選')
check(await app.waitFor("document.body.innerText.includes('尚未開始匯出')"), 'Unplugged phone blocks export with reconnect guidance')
check(calls(dir).filter(c => c[2] === 'pull').length === pullsBefore, 'No transfer after unplug')
await app.shot('12-unplugged-light')
await app.close()

// Moved folder: relative tool paths still resolve from the new location.
{
  const moved = path.join(work, 'portable-moved')
  fs.rmSync(moved, { recursive: true, force: true })
  fs.cpSync(dir, moved, { recursive: true })
  fs.writeFileSync(path.join(moved, 'tools/adb/fake-phone/devices.txt'), ['List of devices attached', 'FAKE-SERIAL-0001\tdevice product:x model:SM_S9260 device:y', ''].join('\n'))
  fs.rmSync(dir, { recursive: true, force: true })
  const m = await launch(moved)
  await m.click('設定')
  const movedOk = await m.waitFor("document.body.innerText.includes('Android Debug Bridge')", 15000)
  check(movedOk, 'Moved folder: bundled-tool path still validates', movedOk ? '' : (await m.eval("document.querySelector('main').innerText")).slice(0, 500))
  await m.click('返回影片')
  await m.click('連線')
  check(await m.waitFor("document.querySelectorAll('li.row').length === 4"), 'Moved folder: connect and scan work')
  await m.close()
}

// Manual tool path entered in the settings UI (outside the program folder, no PATH).
{
  const manualDir = makeFolder('manual', { tools: false })
  const own = path.join(work, 'my-tools')
  fs.rmSync(own, { recursive: true, force: true })
  fs.mkdirSync(path.join(own, 'fake-phone'), { recursive: true })
  fs.copyFileSync(fakeAdb, path.join(own, 'adb.exe'))
  fs.cpSync(path.join(work, 'portable-moved', 'tools/adb/fake-phone'), path.join(own, 'fake-phone'), { recursive: true })
  const m = await launch(manualDir)
  await m.click('設定')
  await m.waitFor("document.body.innerText.includes('必要工具與位置')")
  await m.eval(`(() => { const i = document.querySelector('input[aria-label^="ADB"]'); i.value = ${JSON.stringify(path.join(own, 'adb.exe'))}; i.dispatchEvent(new Event('input')); })()`)
  await m.click('儲存設定')
  const savedOk = await m.waitFor("document.body.innerText.includes('設定已儲存') && document.body.innerText.includes('Android Debug Bridge')", 15000)
  check(savedOk, 'Manual ADB path saved from settings and validated by running it', savedOk ? '' : (await m.eval("document.querySelector('main').innerText")).slice(0, 600))
  check(JSON.parse(fs.readFileSync(path.join(manualDir, 'settings.json'), 'utf8')).Adb === path.join(own, 'adb.exe'), 'Manual path outside the folder stored as entered')
  await m.click('返回影片')
  await m.click('連線')
  check(await m.waitFor("document.querySelectorAll('li.row').length === 4"), 'Manual tool works for connect and scan')
  await m.close()
}

// Unwritable location guidance: the program folder is read-only for logs.
{
  const ro = makeFolder('readonly', { tools: false })
  fs.writeFileSync(path.join(ro, 'log'), 'not a folder')
  const ra = await launch(ro)
  check(await ra.waitFor("document.body.innerText.includes('可寫的位置')"), 'Unwritable program folder shows move-to-writable guidance')
  await ra.shot('13-unwritable-dark')
  await ra.close()
}

const remote = [...new Set(requests)].filter(u => !/^(https?:\/\/(tauri|ipc)\.localhost\/|data:|blob:|about:)/.test(u))
check(requests.length > 0 && remote.length === 0, 'WebView loaded nothing outside the app (no remote page/CDN) across all runs', `${requests.length} requests; outside: ${remote.join(' ')}`)
fs.writeFileSync(path.join(evidence, 'e2e-results.json'), JSON.stringify({ when: new Date().toISOString(), devicePixelRatio: dpr, isFakePhone: true, realFfmpeg: true, results }, null, 2))
const failed = results.filter(r => !r.ok)
console.log(`\n${results.length - failed.length}/${results.length} passed; DPR ${dpr}`)
process.exit(failed.length ? 1 : 0)

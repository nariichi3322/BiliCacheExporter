import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { generate, notice } from './licenses.mjs'

test('notices include nested attribution, full text and source; missing text fails closed', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bili-licenses-'))
  try {
    const pkg = { name: 'fixture', version: '1', license: 'MIT', source: 'https://example.invalid/source', dir }
    assert.throws(() => notice(pkg), /Missing license text/)
    fs.writeFileSync(path.join(dir, 'NOTICE'), 'Attribution alone is not license text')
    assert.throws(() => notice(pkg), /Missing license text/)
    fs.writeFileSync(path.join(dir, 'LICENSE'), 'Permission is hereby granted\nCopyright fixture')
    fs.mkdirSync(path.join(dir, 'vendor'))
    fs.writeFileSync(path.join(dir, 'vendor', 'NOTICE.txt'), 'Nested attribution')
    const text = notice(pkg)
    assert.match(text, /Permission is hereby granted\nCopyright fixture/)
    assert.match(text, /vendor\/NOTICE.txt/)
    assert.match(text, /Nested attribution/)
    assert.match(text, /https:\/\/example.invalid\/source/)
    assert.throws(() => notice({ ...pkg, license: null }), /Missing license declaration/)
    fs.writeFileSync(path.join(dir, 'LICENSE'), '')
    assert.throws(() => notice(pkg), /Empty notice/)
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test('locked dependencies include Vue internals, MPL sources and supplemental notices', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bili-licenses-'))
  try {
    const out = path.join(dir, 'THIRD-PARTY.txt')
    generate(out)
    const text = fs.readFileSync(out, 'utf8')
    assert.match(text, /## @vue\/shared /)
    assert.match(text, /## selectors .*MPL-2.0/)
    assert.match(text, /https:\/\/crates.io\/api\/v1\/crates\/selectors\//)
    assert.match(text, /Mozilla Public License Version 2.0/)
    assert.match(text, /Copyright \(c\) 2021 Bill Avery/)
    assert.match(text, /Copyright \(C\) Microsoft Corporation/)
    assert.match(text, /Microsoft WebView2 SDK native loader: BSD-3-Clause/)
    assert.match(text, /NOTICES AND INFORMATION/)
    assert.ok(!text.includes(os.homedir()))
    assert.ok(!/(^|[^\r])\n/.test(text), 'line endings must be CRLF only')
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

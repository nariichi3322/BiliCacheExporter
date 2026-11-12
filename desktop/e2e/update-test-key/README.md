# 測試專用的更新簽章金鑰

這組 minisign 金鑰**只供測試**，私鑰與密碼刻意公開在這裡。

- `test-only.key`、`password.txt`：`desktop/package.ps1` 用來簽本機整合流程的假更新包；`desktop/src-tauri/tests/fixtures/update/regenerate.ps1` 用來簽測試夾具。
- `test-only.key.pub`：編入程式的測試公鑰。程式只在更新來源被覆寫成本機（`BILI_UPDATE_SOURCE=http://127.0.0.1[:port]`）時才使用它，其餘情況只接受正式公鑰。

正式版的私鑰只存在 GitHub 的 secret，不在儲存庫裡。不要拿這組金鑰簽任何要發佈的檔案。

重新產生（之後要同步更新程式裡的測試公鑰常數並重跑 `regenerate.ps1`）：

```powershell
npx tauri signer generate --ci -f -p (Get-Content password.txt -Raw).Trim() -w test-only.key
```

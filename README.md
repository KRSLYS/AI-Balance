# AI Balance

**Website:** <https://aibalance.krslys.be/>

Track your AI subscriptions in one place, on **Windows, Linux, macOS, Android and iOS**.

AI Balance shows how much of each AI plan you have used: session and weekly limits, credits and
balances. On desktop it lives in the system tray and warns you before a limit runs out.

<p align="center"><img src="assets/icon.png" alt="AI Balance icon" width="128"></p>

## Supported providers

| Provider | Sign-in | Shows |
|---|---|---|
| Claude | Browser (claude.ai) | Session, Weekly, per-model weekly limits, Extra Usage |
| ChatGPT | Browser (OpenAI), or a device code | Session, Weekly, extra limits, Credits, Credit Limit |
| Cursor | Browser (cursor.com) | Total Usage, Cursor Models, Other Models, On-Demand |
| Copilot | Device code (GitHub) | Premium, Chat, Completions, Extra Usage |
| Kimi | Sign-in link (Kimi Code) | Weekly, Session |
| OpenRouter | API key or Management key | Balance, Today, This Week, This Month, Key Limit |
| Z.ai | API key | Session, Weekly, Web Searches |
| Ollama | API key | Session, Weekly, Monthly, Purchased Credits |

Every number comes from the provider's own API. AI Balance estimates nothing.

## Features

- One card per account, with a bar per limit and the time until it resets.
- Sign in with a link: AI Balance opens it for you, or copy it to any device or browser where you are
  already signed in. Nothing to type for most providers.
- Several accounts per provider, grouped together. Rename and reorder them.
- Automatic refresh (every 5 minutes by default) and a Refresh button.
- A notification when a limit reaches the threshold you choose.
- Desktop: system tray icon whose tooltip shows your fullest limits, close-to-tray, launch at login.
- 12 languages, chosen on first launch: English, العربية, 简体中文, Français, Deutsch, Español,
  Italiano, Português, Nederlands, Polski, Русский, Türkçe. Light and dark themes.

## Privacy

- Credentials stay on your device. On desktop they are stored in the operating system's keychain
  (Keychain, Windows Credential Manager, Secret Service); on phones, and on a desktop without a
  keychain, in the app's private data folder.
- A credential is sent only to its own provider. There is no AI Balance server, no analytics and no
  telemetry.
- The full policy is in [PRIVACY.md](PRIVACY.md). Publishing to the app stores is covered in
  [docs/STORES.md](docs/STORES.md).

## Install

Download the file for your system from the [latest release](https://github.com/KRSLYS/AI-Balance/releases/latest):

| System | File |
|---|---|
| Windows | `AIBalance_x.y.z_x64-setup.exe` |
| macOS (Apple Silicon and Intel) | `AIBalance_x.y.z_universal.dmg` |
| Linux | `.deb`, `.rpm` or `.AppImage` |
| Android | `AIBalance_x.y.z_android.apk` |
| iOS | `.ipa` (see [iOS](#ios)) |

The builds are not yet signed with paid certificates, so:

- **Windows** shows a SmartScreen notice: choose **More info**, then **Run anyway**.
- **macOS** blocks the first launch: right-click the app and choose **Open**, or run
  `xattr -dr com.apple.quarantine "/Applications/AI Balance.app"`.
- **Android** asks you to allow installing from your browser or file manager.

## Releasing

Every release is built by GitHub Actions ([release.yml](.github/workflows/release.yml)).
Push a tag and the workflow builds all five platforms and publishes a GitHub Release:

```bash
git tag v0.1.0
```

```bash
git push origin v0.1.0
```

Changing the version in [`.release-version`](.release-version) and pushing does the same without a
tag: the workflow creates the tag itself.

The version comes from the tag, so there is nothing to edit first. A tag with a suffix, such as
`v0.2.0-beta.1`, is published as a pre-release.

To try a build without publishing, open **Actions → Release → Run workflow**, enter a version and
leave **publish** unticked. The installers are attached to the run.

Every push also runs [ci.yml](.github/workflows/ci.yml): formatting, Clippy, the tests and a build
of the web UI.

### Secrets (optional)

Without any secrets the workflow still produces installers for every platform. Add these under
**Settings → Secrets and variables → Actions** to sign the mobile apps.

| Secret | What it is |
|---|---|
| `ANDROID_KEYSTORE_BASE64` | base64 of your Android signing keystore (also produces a signed `.aab` for Google Play) |
| `ANDROID_KEYSTORE_PASSWORD` | the keystore's password (also used as the key's password) |
| `ANDROID_KEY_ALIAS` | the key's alias (`aibalance` when not set) |
| `APPLE_API_ISSUER` | App Store Connect API issuer ID |
| `APPLE_API_KEY` | App Store Connect API key ID |
| `APPLE_API_KEY_BASE64` | base64 of the API key's `.p8` file |
| `APPLE_DEVELOPMENT_TEAM` | your Apple Developer team ID |

#### Android

Without `ANDROID_KEYSTORE_BASE64` each build is signed with a new throwaway key. Such an APK
installs, but it cannot update an earlier install: you have to uninstall first. Create a keystore
once and keep it safe, because every later update must be signed with the same one:

```bash
keytool -genkeypair -keystore aibalance.jks -storetype JKS -alias aibalance -keyalg RSA -keysize 2048 -validity 10000
```

```bash
base64 -w0 aibalance.jks
```

#### iOS

Apple only lets a signed app run on an iPhone.

- **With an Apple Developer account** ($99 a year): add the four `APPLE_*` secrets and the workflow
  builds a signed `.ipa` you can upload to TestFlight or the App Store.
- **Without one:** the workflow tries to build an unsigned `.ipa`. Install it with a sideloading
  tool such as AltStore or Sideloadly, which signs it with your own Apple ID for seven days at a
  time. This path is experimental, and it never blocks the release of the other platforms.

## Development

Requirements: [Node.js](https://nodejs.org) 20 or later, [Rust](https://rustup.rs), and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system.

```bash
npm install
```

```bash
npm run tauri dev
```

Other commands:

```bash
npm run dev                # the UI alone in a browser, with demo data
```

```bash
npm run tauri build        # installers for the system you are on
```

```bash
cd src-tauri && cargo test # the Rust tests
```

For the phone apps, run `npm run tauri android init` or `npm run tauri ios init` once, then
`npm run tauri android dev` or `npm run tauri ios dev`.

### How the code is laid out

- `src/` is the interface: React and TypeScript. `src/locales/` has one file per language; to add a
  language, copy `en.ts`, translate it, and add a line to `locales/index.ts`.
- `src-tauri/src/providers/` has one file per provider: the HTTP reads and the mapping from the
  provider's answer to metric lines, each with tests.
- `src-tauri/src/login.rs` holds the sign-in flows (browser with PKCE, device code).
- `src-tauri/src/store.rs` saves accounts, settings and credentials.
- `src-tauri/src/lib.rs` wires it together: commands, the refresh loop, notifications, the tray.

To add a provider, add its file under `providers/`, a variant to `ProviderKind` in `model.rs`, a
line in `providers/mod.rs`, and an entry in `src/providers.ts`.

## بالعربية

**AI Balance** تطبيق يعرض استهلاكك من اشتراكات الذكاء الاصطناعي (Claude و ChatGPT و Cursor و Copilot و Kimi
و OpenRouter و Z.ai و Ollama) في مكان واحد، ويعمل على Windows و Linux و macOS و Android و iOS.

- بيانات الدخول تبقى على جهازك ولا تُرسل إلا إلى المزوّد نفسه.
- الواجهة تدعم 12 لغة (منها العربية والإنجليزية والصينية ولغات أوروبية) والوضعين الفاتح والداكن.
- لإصدار نسخة جديدة يكفي رفع وسم (tag) مثل `v0.1.0`، وسيبني GitHub Actions كل المنصات وينشر الإصدار.

## Acknowledgements

The idea comes from [OpenUsage](https://github.com/robinebers/openusage) by Robin Ebers, a macOS
menu bar app. AI Balance is an independent project with its own code and is not affiliated with it.

## License

[MIT](LICENSE) © 2026 KRSLYS BV

# Project guidance

These instructions apply to the entire repository. Follow more specific
instructions in nested `AGENTS.md` files when present.

## Project overview

MultiRoblox is a Windows desktop application for managing and launching Roblox
accounts. It uses Tauri 2, a Rust backend, a plain HTML/CSS/JavaScript frontend,
and a C# helper built with the Windows .NET Framework compiler.

Keep changes focused on the requested behavior. Follow the existing architecture
and coding style; avoid unrelated refactors, formatting churn, new frameworks,
or dependencies when existing code or standard libraries suffice.

## Repository map

- `MultiRoblox/src/index.html`: application markup.
- `MultiRoblox/src/renderer.js`: frontend behavior and state.
- `MultiRoblox/src/styles.css`: layout, themes, and component styles.
- `MultiRoblox/src/tauri-bridge.js`: `window.api` bridge to Tauri commands/events.
- `MultiRoblox/src-tauri/src/commands.rs`: commands exposed to the frontend.
- `MultiRoblox/src-tauri/src/lib.rs`: command registration and app lifecycle.
- `MultiRoblox/src-tauri/src/native.rs`: Roblox launch and process management.
- `MultiRoblox/src-tauri/src/helper.rs`: native helper lifecycle and pipe protocol.
- `MultiRoblox/src-tauri/src/state.rs`: shared backend state.
- `MultiRoblox/src-tauri/src/{storage,jsonfile,settings,paths}.rs`: persisted data.
- `MultiRoblox/src-tauri/src/{crypto,encryption}.rs`: credential protection.
- `MultiRoblox/src-tauri/resources/RobloxNative.cs`: helper source.
- `MultiRoblox/src-tauri/resources/RobloxNative.exe`: tracked helper binary,
  embedded into the main executable at Rust compile time.
- `MultiRoblox/build.bat`: Windows helper/main build and versioned output in `dist`.

## Development rules

- Trace callers before changing shared behavior. For a Tauri command change,
  check its Rust implementation, registration, JavaScript bridge, and callers.
- Keep helper command names, argument formats, replies, and events compatible
  across C# and Rust. Keep stdout reserved for the pipe protocol; diagnostics
  belong on stderr.
- Preserve launch cancellation, process ownership tracking, and helper cleanup.
  Do not expand process termination to unrelated user processes.
- Preserve existing account IDs, stored data formats, encryption compatibility,
  and the `%APPDATA%\multiroblox` data location unless migration is in scope.
- Retain atomic writes and protections against overwriting unreadable data.
- Treat credentials, cookies, authentication tickets, API keys, webhook URLs,
  and screenshots as sensitive. Do not log or commit real user data.
- Validate external inputs at the backend boundary. Keep secrets out of frontend
  error messages and logs; retain URL restrictions and timeout/error handling.
- Keep UI text, themes, focus behavior, and accessibility consistent with nearby
  code. Clean up timers and event listeners when their owners are disposed.

## Build environment

Build and validate Windows behavior on Windows. A macOS/Linux checkout can be
used for editing and static review, but does not establish that Windows-specific
process control, DPAPI, WebView2, tray, or helper behavior works.

Respect the user's designated build machine. Do not install Windows toolchains,
download executables, or attempt cross-compilation on another host merely to
complete a check. Report checks that require the Windows machine.

Windows prerequisites:

- Rust stable with the `x86_64-pc-windows-msvc` toolchain.
- Visual Studio C++ Build Tools with the desktop C++ workload and Windows SDK.
- .NET Framework `csc.exe` for rebuilding the C# helper.
- WebView2 Runtime for running the application.

The direct Cargo build uses the static frontend in `src`; it does not require
Node.js, `npm install`, or the Tauri CLI.

## Building

Run these commands in Windows PowerShell from `MultiRoblox/`.

The README's `.\build.bat` reads the numeric `major.minor.patch` package version
from `src-tauri/Cargo.toml` using `cargo metadata` and outputs
`dist/release_<version>/MultiRoblox_v<version>.exe`.
After checking required compilers and validating the version, it removes the
entire same-version output directory before compiling; other versions are kept.
Do not store unrelated files in these generated release directories. Close any
running executable from that directory before rebuilding.

It stops with a nonzero exit code if preflight checks, output cleanup, helper
compilation, the main build, or the output copy fail. It also checks that the
helper output exists. Preserve these checks so failed builds cannot proceed
using an old helper. Keep directory deletion scoped to the validated version
under `dist`; never delete all releases or use an unchecked config value.

To rebuild the helper explicitly:

```powershell
$csc = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if (!(Test-Path $csc)) {
    $csc = "$env:WINDIR\Microsoft.NET\Framework\v4.0.30319\csc.exe"
}
if (!(Test-Path $csc)) { throw "Missing .NET Framework C# compiler" }
& $csc /nologo /optimize+ /platform:x64 /target:exe /r:System.Drawing.dll `
    /out:.\src-tauri\resources\RobloxNative.exe `
    .\src-tauri\resources\RobloxNative.cs
if ($LASTEXITCODE -ne 0) { throw "Native helper build failed" }
```

To compile only the main executable using the helper already present:

```powershell
cargo build --release --locked --manifest-path .\src-tauri\Cargo.toml --bin MultiRoblox
if ($LASTEXITCODE -ne 0) { throw "Main executable build failed" }
```

This direct command produces `src-tauri/target/release/MultiRoblox.exe`; it does
not perform the versioned output cleanup/copy provided by `build.bat`.
After a version bump, run `.\build.bat` first so Cargo can update `Cargo.lock`
before using commands with `--locked`.

Do not copy or report an older artifact as a successful new build. If the helper
source changes, rebuild the helper before the main executable, or explicitly
report that binary generation remains pending on Windows.

## Validation

- Run checks appropriate to the change. Documentation-only edits need a content
  review and whitespace check, not an application build.
- For Rust changes, run affected tests first. From `MultiRoblox/`, the existing
  library test suite is `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`.
  Tests currently cover crypto, JSON persistence, API parsing, and webhook URL
  validation. Add focused regression tests for non-trivial fixes.
- For frontend JavaScript edits, use `node --check src/renderer.js` and
  `node --check src/tauri-bridge.js` if Node is available. Syntax checks do not
  replace testing the affected UI in the Windows app.
- Validate relevant Windows flows when they change: launch/cancel, process
  tracking, account removal, helper shutdown/restart, tray exit, or screenshots.
  Use disposable test data; do not exercise destructive flows on real accounts.
- Do not silently use live account creation, screenshots, or outbound webhooks as
  tests. Use controlled fixtures or an explicitly designated test environment.
- Report what was checked, what passed or failed, and what remains unverified.

## Binaries, versions, and commits

- Do not change the tracked helper EXE for unrelated source/documentation work.
  When updating it, identify the source revision, build command, and SHA-256.
- Antivirus results and matching hashes are evidence about a specific artifact,
  not proof of safety. Do not disable protection or add exclusions as a build fix.
- `src-tauri/Cargo.toml` `[package].version` is the single manually maintained
  app version. Leave `version` absent from `tauri.conf.json` so Tauri inherits it.
  Cargo updates the package's `Cargo.lock` entry during a normal build; include
  that generated change in the version-bump commit.
  `package.json` currently describes the frontend tooling package; do not assume
  its version controls the Windows executable.
- Verify the built executable's version before preparing release assets.
  A version-looking commit message is not a Git tag or a published release.
- Keep `target/`, `dist/`, `node_modules/`, scratch output such as `native-build/`,
  and user data out of commits. Stage intended paths explicitly.
- Preserve `LICENSE`, `NOTICE`, and attribution. Do not rewrite history, create
  releases, or push changes unless the user has requested that action.

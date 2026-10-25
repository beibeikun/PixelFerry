# PixelFerry third-party components

PixelFerry is a Rust application. Dependency versions and source checksums are recorded in Cargo.lock. Distributions must carry the licenses of the selected crate versions and Skia binaries.

- Tauri 2: MIT OR Apache-2.0 — https://github.com/tauri-apps/tauri
- React: MIT — https://github.com/facebook/react
- shadcn/ui: MIT — https://github.com/shadcn-ui/ui (Base Nova component source included in frontend/src/components/ui)
- Base UI: MIT — https://github.com/mui/base-ui
- Tailwind CSS, Zustand, Lucide: MIT / MIT / ISC; exact versions are recorded in package-lock.json. The distribution includes npm package notices under licenses/.
- Microsoft Edge WebView2 Runtime is a Microsoft component. The included signed Evergreen Bootstrapper downloads the runtime only when missing. https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution
- rust-skia: MIT — https://github.com/rust-skia/rust-skia
- Skia: BSD-3-Clause and bundled component notices — https://skia.org/
- The build scripts namespace Skia's ICU C symbols with `pixelferry_` using LLVM tools to avoid collisions with Windows globalization imports. Rendering behavior is unchanged; the modification is recorded in `scripts/namespace-icu.ps1`.
- Microsoft Visual C++ runtime DLLs are included app-local from the installed Windows x64 runtime; they remain Microsoft components. https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files
- FFmpeg/ffprobe: https://ffmpeg.org/; the local binaries are copied unchanged from the existing project's Gyan build. Keep `tools/FFmpeg-license.txt` and `tools/FFmpeg-sha256.txt`. That build enables GPLv3; provide corresponding source for the exact binary build when publicly redistributing it. Build provider: https://www.gyan.dev/ffmpeg/builds/
- MSI Afterburner shared memory is read through Windows read-only file mappings. No MSI binaries are redistributed.

No T'OWEI proprietary DLLs or extracted application code are redistributed. Windows system APIs provide HID, power notifications, named pipes and DPAPI.

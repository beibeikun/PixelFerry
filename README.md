# PixelFerry · 像素渡轮

[![License: MIT](https://img.shields.io/badge/License-MIT-79bba0.svg)](LICENSE)

把画面送往不同 USB 设备。

Windows x64 屏幕控制平台，界面使用 Tauri 2 + React + TypeScript + shadcn/ui，设备、渲染、数据和存储继续使用 Rust。旧版 `../lg600-control` 保留用于对照和回退；本工程不修改旧程序、旧数据或原厂软件。

## 开发与运行

开发需要 Node.js 22+、Rust stable、`rustup component add llvm-tools` 和 Windows MSVC/SDK C++ 编译环境。也可使用工程本地 `.tools/windows-sdk`、便携 MSVC 编译器及 SDK BuildTools 与 Rust 自带的 LLD，`scripts/build.ps1` 会自动配置。构建脚本为 Skia ICU 导出加上独立前缀，避免与 Windows 的 ICU 符号重名；请使用该脚本构建。首次构建需要下载依赖与 Skia 预编译包。

最终用户运行不需要 Node、Rust 或 .NET 开发环境。Windows 界面依赖 Microsoft Edge WebView2 Runtime；安装包检测缺失后使用随附的微软引导程序联网安装。便携版缺失运行时时，可运行同目录的 `MicrosoftEdgeWebview2Setup.exe`。不支持缺少 WebView2 的完全离线首次安装。

从独立源码仓库构建时，先从 [FFmpeg 官网](https://ffmpeg.org/download.html)获取 Windows x64 的 `ffmpeg.exe`、`ffprobe.exe` 及该构建对应的许可文件，将它们放在一个目录，并把该目录传给准备脚本；本地旧工程的 `lg600-control/resources` 只是默认开发路径。

```powershell
./scripts/prepare-tools.ps1 -FfmpegDirectory C:\path\to\ffmpeg
Push-Location frontend
npm ci
npm run build
npm test
Pop-Location
./scripts/build.ps1 -Action test
./scripts/build.ps1 -Action build -Release
./target/release/pixelferry.exe --simulate --data-dir=C:\Temp\PixelFerry-demo
```

模拟模式提供 480×480、640×480 和三面 330×330 设备，共五块屏幕，不打开真实 USB 写通道。真实模式默认不绑定任何设备，需在界面点击绑定后开始输出。

```powershell
# 只读枚举
./target/release/pixelferry.exe --diagnose
# 在独立测试目录运行多设备端到端验收，结束后关闭测试后台
./target/release/pixelferry.exe --self-test --data-dir=C:\Temp\PixelFerry-selftest
# 对已运行的应用进行两小时采样；不主动绑定或打开 USB
./target/release/pixelferry.exe --soak-seconds=7200
```

Windows GUI 可执行文件在 PowerShell 中直接启动会立即返回；诊断、自动验收与采样可用 `Start-Process -Wait -WindowStyle Hidden` 并重定向标准输出／错误到文件。自动验收拒绝非空配置目录；不指定目录时自动创建独立临时目录。两小时采样写入用户数据目录的 `artifacts/`，记录写入与连接状态，不代替实屏观察。

正常启动使用 `%APPDATA%/PixelFerry/`。关闭窗口隐藏到托盘并继续播放；托盘“完全退出”停止后台及设备进程。场景与素材全局共享，每块屏独立选择场景、留边／裁切、画质、帧率与受支持的硬件控制。

编辑器：Shift/Ctrl 多选、空白拖动框选、Ctrl+A 全选、Esc 取消、方向键移动、Shift+方向键移动 10 像素、Delete 删除、Ctrl+Z / Ctrl+Shift+Z 撤销重做、Ctrl+S 保存。角柄等比缩放，边柄调整单轴，Alt 暂停吸附并使用居中缩放，上方圆点旋转，Shift 旋转对齐 15°。

### 文字与动态参数混排

添加或选中文字，在属性中打开“编辑文字与参数”。直接输入“余额：”等固定文字，再将 DeepSeek 或传感器参数插入光标位置；无需手写占位符。参数可重复插入、换行、复制、整体删除或拖动排序。选中文字或点击参数后，可单独调整字体、字号、颜色和粗体，也可恢复继承元素样式。

文字样式提供独立的“水平对齐”和“垂直对齐”，可组合左／中／右与上／中／下；旧场景默认仍为垂直居中。在混排编辑器中选中参数后，可将单位或货币符号字号设为数值字号的 25%–100%，数值本身不缩小，符号与数值共用基线。默认 100% 保持原外观，关闭单位或符号显示时保留已选比例。

传感器按稳定 ID 绑定，支持搜索名称、设备和单位；默认显示 1 位小数和单位。DeepSeek 提供余额、赠送余额、充值余额、今日消费估算、累计消费估算和峰谷时段，金额默认 4 位小数与货币符号，币种沿用账户设置。数值可选 0–8 位小数。数据缺失或过期时只将对应参数显示为 `--`，不会覆盖固定文字或其他参数。已有传感器与 DeepSeek 组件可“转换为混排文字”，保留原样式和绑定，转换可撤销。

弹窗内修改属于草稿，本地撤销不影响画布历史；“应用”一次提交并自动保存，“取消”放弃草稿。实际效果由同一 Rust 渲染器生成，每秒刷新数据。每个元素最多 256 个片段，文字累计上限 16000 字节。不支持公式、条件表达式或脚本。

混排内容随配置、场景复制和 `.pixelferry` 场景包保留，不导出账户凭据或动态采样值。旧版本只能显示参数名称组成的普通文本摘要，不能编辑混排内容；请勿使用旧版本重新保存包含混排的场景，以免丢失新字段与分段样式。

## 结构

| 模块 | 职责 |
| --- | --- |
| core | 带版本领域模型、校验、任意尺寸几何和编辑历史 |
| devices | 适配器接口、LG600 原生 HID、模拟设备、完整帧发送 |
| render | Skia 画面、中文排版、毛玻璃、FFmpeg 解码和 JPEG |
| services | Afterburner 只读采样、DeepSeek、定点消费记录和 DPAPI |
| storage | 原子配置、媒体导入、场景 ZIP 校验及旧版迁移 |
| app | Tauri 2 命令桥接、托盘、用户专属命名管道、后台和独立设备进程 |
| frontend | React 19 / TypeScript / Vite / Tailwind 4 / shadcn Base Nova 组件、Zustand 编辑历史 |

LG600 的 VID/PID、双接口、480×480、1020 字节 JPEG 分片及 80 FPS 重复末帧策略只在适配器层声明。后续型号新增内置 Rust 适配器，不修改通用场景模型。

输出接口携带显式尺寸和帧格式，支持 JPEG、RGBA8 和小端 RGB565。模拟设备分别声明三种格式，运行时按屏幕能力编码；预览仍使用同一帧的 RGBA 数据。JPEG 分包只由 LG600 适配器处理。

React 仅绘制编辑辅助框并显示 Rust 返回的 RGBA 画面，场景仍由 Skia 绘制。Tauri 通过类型明确的命令连接现有本地后台；文件选择、导入导出和 Key 操作由 Rust 完成。主窗口关闭时保存并收起，设备后台继续播放。前端具有保存队列和操作屏障，导入、导出、迁移与退出会等待尚未保存的修改。

界面使用奶油白、薄荷绿和珊瑚橘，支持深海蓝主题。工作台包含共享场景、画布和属性／图层面板；设备港口按物理设备及屏幕分别绑定；素材库可复用已有媒体。官方 shadcn 组件源码保存在 `frontend/src/components/ui/`，样式位于 `frontend/src/styles.css`，不依赖在线字体或 CDN。

## 迁移与场景包

“迁移旧版数据”选择包含旧 `state.json` 的目录。迁移复制素材、保留传感器 ID、旧设置、DeepSeek 偏好与可迁移的消费记录。同用户 DPAPI／Chromium v10 凭据可尝试解密后重新加密；失败提示重新输入，不阻断场景迁移。不会覆盖已有目标账户记录或 Key。旧目录不变，重复迁移同一目录会被拒绝。

`.pixelferry` 是版本化 ZIP 场景包；继续支持旧 `.lg600` 导入。只导出布局及引用素材，不包含账户、消费记录、设备绑定或启动设置。导入追加并重建 ID；同名追加“（导入）”。素材 SHA-256 校验，拒绝路径穿越、符号链接、重复路径、加密、缺失／损坏及超限内容。

保留旧版上限：32 场景、每场景 128 图层、1000 素材、单素材 1 GB、包内素材共 2 GB、清单 4 MB、动态素材最长 10 分钟、每场景一个可见动态图层。默认输出等比留边。

首版全局最多绑定 8 块屏幕，同时最多 4 路开启的动态输出；设备端解码与渲染任务有界。编辑器额外保留当前场景预览。Afterburner 采样独立于联网查询，不因 API 查询超时停止更新。

## 验收

执行结果与未完成的人工验收见 `docs/verification.md`。USB 写入成功不代表实屏显示正常，模拟验收不代表真机验收。真实模式连接前检查 LG600 Studio 和 T'OWEI 占用，提示用户自行退出。

第三方组件及 FFmpeg 来源见 `resources/THIRD_PARTY.md`。不随包附带原厂 DLL、Node 或 .NET 监控辅助程序。

## 开源协议

PixelFerry 自有源码采用 [MIT 协议](LICENSE)。第三方库和随包 FFmpeg 分别适用各自协议；FFmpeg 的 GPLv3 构建和再分发要求见 `resources/THIRD_PARTY.md`。构建缓存、开发工具、用户数据和发布二进制不纳入源码仓库。

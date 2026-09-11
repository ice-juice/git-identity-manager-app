# 御钥师 · Android / iOS 移动端开发计划

> 配套文档：`规划设计.md`（需求与架构）、`开发计划.md`（桌面 M0–M6）、`指纹识别解锁设计.md`、`密码与2FA管理设计.md`、`程序更新功能设计.md`、`发布新版本流程.md`
> 本文定位：把「用 Tauri 2 mobile 复用现有代码库做出 Android / iOS App」落成**可逐条执行**的开发计划。
> 制定日期：2026-09-11 · 基线版本：v1.3.1 · 基线分支：`dev`

---

## 第一部分 · 结论先行

### 1.1 一句话结论

**技术路线：同一个 `app/src-tauri` + 同一套 React 前端，靠 `cfg(desktop)` / `cfg(mobile)` 分层与响应式 UI 出四端，不另起代码库。**

理由是这个仓库的价值密度集中在三块纯 Rust 代码里，它们**一行不用改**就能在 Android / iOS 上跑：

| 模块 | 文件 | 为什么可以直接复用 |
|---|---|---|
| 加密与信封 | `vault/crypto.rs`、`vault/envelope.rs`、`vault/kdf.rs`、`vault/recovery.rs` | 纯 RustCrypto（argon2 / chacha20poly1305 / hkdf / sha2 / zeroize），无系统调用 |
| 加密持久化 | `store/mod.rs` | 只用 `std::fs` + AEAD，路径由 vault root 决定 |
| 零知识云同步 | `sync/engine.rs`、`sync/s3.rs`、`sync/backup.rs` | 自研 SigV4 + reqwest(rustls)，纯用户态 |
| TOTP | `totp.rs` | 纯算法 |
| 密钥生成/指纹 | `ssh/keygen.rs`、`ssh/key.rs` | `ssh-key` crate 纯 Rust，不调 `ssh-keygen` 二进制 |

反过来，**不可移植的不是某几个函数，而是「操作本机 Git / SSH」这件事本身**：手机上没有 `~/.ssh/config` 的消费者（OpenSSH）、没有 ssh-agent、没有 `git` 可执行文件，也没有别的进程能读到我们放下去的私钥。所以移动端不该是桌面版的等价复制。

### 1.2 移动端的产品定位

> **桌面端 = 工作站**（在这台电脑上真正干活）；**移动端 = 保险库的随身终端**（随时取用机密、随身看板、换机接力）。

移动端独有的价值有三条，都很实在：

1. **TOTP 本来就该在手机上。** 现在用户要么在桌面御钥师里看验证码（人不在电脑前就没用），要么另装 Authenticator（机密分裂成两套）。手机端一上，御钥师才算完整覆盖 2FA 场景。
2. **隐私账号随身查。** 密码管理器不进手机，等于半个。
3. **换机接力与应急。** 电脑丢了、重装了，手机上有一份可解密的密文 + 恢复密钥入口，比只有云端密文强得多。

### 1.3 功能矩阵（移动端 v1 范围）

| 能力 | 桌面 | 移动 v1 | 处理方式 |
|---|---|---|---|
| 解锁：访问密码 | ✅ | ⚠️ 兜底 | Argon2 参数受限，见 D2 |
| 解锁：恢复密钥 | ✅ | ✅ | HKDF，不吃内存，手机上最顺 |
| 解锁：生物识别 | ✅ | ✅ **核心** | 新增 Android/iOS backend，见 D3 |
| 2FA / TOTP | ✅ | ✅ **核心** | 相机扫码替代屏幕扫码 |
| 隐私账号（含历史、联动 TOTP） | ✅ | ✅ **核心** | UI 移动化 |
| 云端同步 / 快照恢复 | ✅ | ✅ **核心** | 引擎复用，但默认只读，见 D5 |
| 扫码配对入网 | — | ✅ **新增** | 移动端专属，见 D4 |
| 身份总览 | ✅ | ✅ 只读 | 去掉 agent / `ssh -T` 两维状态 |
| 公钥查看 / 复制 / 分享 | ✅ | ✅ | 手机上把公钥贴到 GitHub 很实用 |
| 离线 `.gambackup` 导入导出 | ✅ | ⚠️ P2 | 走系统文件选择器 / 分享面板 |
| 生成 / 导入 SSH 密钥 | ✅ | ⚠️ P2 | 算法能跑，但手机上生成的密钥要同步回桌面才有用 |
| SSH 配置编辑 | ✅ | ❌ | 手机上没有 OpenSSH 会去读它 |
| ssh-agent 托管 | ✅ | ❌ | 同上 |
| 仓库扫描 / 智能克隆 | ✅ | ❌ | 没有 `git` 二进制，也没有工作目录 |
| 屏幕扫码 | ✅ | ❌ | 换成相机扫码 |
| 托盘 / 自启动 / 单实例 / 应用内自更新 | ✅ | ❌ | 移动端无此概念，更新交给应用商店 |
| 环境安全自查 | ✅ | 🔄 改写 | 改为 root/越狱、锁屏、系统版本检测 |

被裁掉的 4 个页面（`ConfigPage.tsx`、`Agent.tsx`、`Repos.tsx`、`Clone.tsx`）恰好是桌面耦合最重的部分，这也侧面说明分层是干净的。

### 1.4 阻断性前置条件（必须先决策，否则计划无法执行）

| # | 事项 | 现状 | 影响 |
|---|---|---|---|
| P0 | **Android SDK / NDK / JDK** | ❌ **未安装**（本机无 `JAVA_HOME` / `ANDROID_HOME` / `NDK_HOME`，`%LOCALAPPDATA%\Android\Sdk` 不存在） | 阻断 `tauri android init` 与 Android 编译。实测 `cargo check --target aarch64-linux-android` 停在 `ring` 的构建脚本找不到 `aarch64-linux-android-clang` |
| P1 | **iOS 必须有 macOS + 完整 Xcode** | ✅ 有 Mac | `tauri ios init` 生成的 `gen/apple` 必须在 Mac 上跑一次；真机调试、签名、上传全在 Mac |
| P2 | **Apple Developer Program（$99/年）** | ❌ 无 | iOS 没有免商店分发路径：不上 App Store 也得走 TestFlight，一样要付费账号（自签只有 7 天有效期，不可用）。**M13 的实际阻断项** |
| P3 | **Google Play Console（$25 一次性）** | 未定 | Android 可以不上商店，直接在 GitHub Release 发 APK（和现在桌面版一致），所以 P3 是可选项 |
| P4 | **Android 签名 keystore 的保管** | 未建立 | keystore 一旦丢失，已发布的应用**永远无法更新**。必须在 M12 之前定好离线备份方案 |

Rust 侧的 4 个 Android target 已装好（`aarch64-linux-android`、`armv7-linux-androideabi`、`i686-linux-android`、`x86_64-linux-android`）。

**决策（2026-09-11）：Android 先行，iOS 等 P2 落实后再开 M13。**

### 1.5 顺带发现的桌面既有缺陷（不属于移动端范围，建议单独修）

`identity.rs` 的 `config_base_dir()` 在整个仓库里是**唯一**决定本机状态目录的地方，而它只认 `APPDATA`：

```rust
std::env::var("APPDATA").map(PathBuf::from)
    .unwrap_or_else(|_| std::env::temp_dir())
```

macOS / Linux 上没有 `APPDATA`，所以这两个平台的 `config.json`（工作空间路径、设置、`machine_id`）和 `biometric.env`（Touch ID 信封）**一直存在临时目录里**。macOS 的 `/var/folders` 会被系统周期性清理，表现就是「设置丢了 / Touch ID 解锁莫名要重新开启 / 认不到工作空间」。

M7a 引入的 `identity::init_base_dir()` 已经提供了修复所需的注入点，修法是在 `setup()` 里给桌面也注入 `app.path().config_dir()`。但这需要一次**数据迁移**（把临时目录里的既有文件搬到新位置，搬不到就退回默认），且要在 macOS / Linux 真机上验证，所以不塞进移动端计划，单独立项。

--- Android 在 Windows 上可以全流程本地开发与真机调试，能独立跑完 M7–M12；iOS 等 P1/P2 落实后再以增量方式接入（架构层面 M7–M11 已经把 iOS 的坑一并铺平，届时主要工作只剩 D3 的 Swift 实现与 M12 的签名分发）。

---

## 第二部分 · 现状盘点

### 2.1 三分类清单

**A 类 · 直接复用（零改动）**

`vault/`（crypto、envelope、kdf、recovery、header）、`store/`、`sync/`（engine、s3、backup）、`totp.rs`、`ssh/keygen.rs`、`ssh/key.rs`、`git/url.rs`、`git/infer.rs`、`git/owners.rs`、`git/github.rs`、`model/`、`error.rs`、`util.rs`、`importer.rs`。

**B 类 · 需要改造（加 cfg 分支或换实现）**

| 文件 | 问题 | 改造方向 |
|---|---|---|
| `identity.rs:51` `config_base_dir()` | `%APPDATA%` 取不到就回退 `temp_dir()`；移动端会落到临时目录，随时被系统清掉 | 改用 Tauri `app.path().app_data_dir()`，见 D1 |
| `workspace_path.rs` | 全套 Windows 保留名 / 盘符冒号校验 | 整块 `#[cfg(desktop)]`；移动端路径固定不可选 |
| `biometric/mod.rs:5-17` | 只有 windows / macos / linux 三个 backend | 新增 `android.rs`、`ios.rs`，见 D3 |
| `biometric/mod.rs:53` `bind_window()` | Windows HWND 专用 | 移动端空实现 |
| `clipboard/` + `arboard` | `arboard` 无条件依赖，移动端编不过 | 换 `tauri-plugin-clipboard-manager`，见 D6 |
| `qrscan.rs` + `xcap` | 屏幕截图扫码 | 移动端换 `tauri-plugin-barcode-scanner` 相机扫码 |
| `security/` | 检测项全是桌面语义 | 改写为 root/越狱/锁屏/系统版本 |
| `net/proxy.rs` | 含一处 `Command` 调用 | 该分支 desktop-only，代理配置本身可复用 |
| `platform/mod.rs` | `PlatformOps` 只有 secure_key_file / ssh_dir | 移动端实现为 no-op（沙箱内无需 chmod，也没有 ssh 目录） |
| `platform/mod.rs:28-74` updater 相关 | `updater_target()` 无 android/ios 分支 | `self_update_supported()` 移动端返回 false |
| `lib.rs:196-244` setup / window event | tray、single_instance、agent bootstrap、关闭行为 | 全部包 `#[cfg(desktop)]` |
| `sync/engine.rs:1053` `apply_pulled_ssh` | 会往本机 ssh 目录写 | 移动端只写 vault 内正本，见 D5 |
| `sync/engine.rs:1095` `ssh_text_for_sync` | 会把 ssh 配置改写成本机路径 | 移动端原样回传，**这是最容易出数据事故的点** |
| `vault/kdf.rs:12` `MEM_CEIL_KIB` | 上限 512MiB，手机会被系统杀 | 见 D2 |

**C 类 · 移动端不适用（`cfg(desktop)` 隔离即可）**

`agent/`（全部）、`ssh/managed.rs`、`ssh/config.rs`、`ssh/connect.rs`、`ssh/toolchain.rs`、`tray.rs`、`single_instance.rs`、`autostart.rs`、`update/`、`sys.rs`、`git/repo.rs`、`commands/window.rs`、`icons.rs`（托盘图标部分）。

### 2.2 外部进程调用清单（移动端一律不可用）

`Command::new` 共 7 个文件、21 处，全部落在 C 类或 B 类的 desktop 分支里：

| 文件 | 调用的二进制 |
|---|---|
| `sys.rs:37/88/95/105` | 探测到的 `ssh` / `ssh-add` / `git`、`taskkill`、`kill` |
| `agent/mod.rs` | `ssh-agent` / `ssh-add` |
| `commands/repo.rs:372/379/386/508` | `explorer` / `open` / `xdg-open`、`git` |
| `commands/assets.rs:124/446/454` | `notepad`、`cmd`、系统 opener |
| `autostart.rs:62/66/78` | `launchctl` |
| `single_instance.rs:355` | `kill` |
| `net/proxy.rs` | 1 处系统探测 |

好消息：**SSH 密钥生成没有调 `ssh-keygen`**，走的是 `ssh-key` crate，所以「在手机上生成 Ed25519」在技术上是通的。

### 2.3 前端现状

- 页面 17 个（`app/src/pages/`），布局与导航在 `ui/Layout.tsx`，自定义标题栏在 `ui/TitleBar.tsx`（因为 `tauri.conf.json` 里 `decorations: false`）。
- **样式是手写语义化 CSS（`index.css`），不是 Tailwind utility 堆砌** —— 这对移动化是重大利好：改 `.sidebar` / `.body` / `.nav-item` 几个类的媒体查询就能换骨架，不用逐个组件改 className。
- **`index.css` 目前 0 条 `@media`**，`.sidebar` 固定 `width: 176px`、`.titlebar` 固定 `height: 40px`、窗口最小尺寸 840×520 —— 全是桌面假设，需要新增断点。
- `.nav-item` 的 `padding: 5px 8px` 触摸目标远小于 44px，需要移动端单独放大。
- `lib/ipc.ts` 是统一的 invoke 封装 —— 意味着「桌面专属命令在移动端返回 Unsupported」这个策略可以在一个文件里统一处理。

---

## 第三部分 · 目标架构

### 3.1 目录结构增量

```
app/
├── src/
│   ├── lib/
│   │   └── platform.ts             新增：isMobile / isAndroid / isIOS 与能力探测
│   ├── ui/
│   │   ├── Layout.tsx              保留，桌面侧栏
│   │   ├── MobileShell.tsx         新增：底部 Tab 外壳 + 安全区
│   │   └── TitleBar.tsx            移动端不渲染
│   └── pages/
│       ├── Pair.tsx                新增：扫码配对入网
│       └── (其余页面响应式改造，不新建移动端副本)
└── src-tauri/
    ├── src/
    │   ├── biometric/
    │   │   ├── android.rs          新增：Keystore HMAC backend
    │   │   └── ios.rs              新增：Keychain + biometryCurrentSet backend
    │   ├── mobile/                 新增：移动端专属编排
    │   │   ├── mod.rs
    │   │   ├── paths.rs            沙箱目录解析
    │   │   ├── lifecycle.rs        进后台即锁
    │   │   └── pairing.rs          配对入网协议
    │   └── platform/
    │       └── mobile.rs           新增：PlatformOps 的 no-op 实现
    ├── gen/
    │   ├── android/                由 `tauri android init` 生成，需提交
    │   └── apple/                  由 `tauri ios init` 生成（必须在 Mac 上），需提交
    ├── capabilities/
    │   ├── desktop.json            拆分现有 capabilities
    │   └── mobile.json             新增
    ├── tauri.android.conf.json     新增：平台覆盖配置
    └── tauri.ios.conf.json         新增
```

### 3.2 cfg 分层纪律

沿用 `platform/mod.rs` 开头那条既有约定（「业务代码禁用 `cfg!(windows)`」），扩展一条：

> **业务代码不写 `cfg(target_os = "android")`。** 平台差异只允许出现在 `platform/`、`biometric/`、`mobile/`、`clipboard/` 四个目录，以及 `lib.rs` 的注册段。

### 3.3 命令注册策略（重要实现细节）

`tauri::generate_handler!` 是宏，**无法条件拼接命令列表**。两种做法：

- ❌ 写两个 `generate_handler!` 分支：命令表要维护两份，极易漏。
- ✅ **保持单一注册表，把桌面专属命令的函数体在移动端改为返回 `AppError::Unsupported`。**

配套新增 `error.rs` 的 `Unsupported(&'static str)` 变体（code = `UNSUPPORTED_PLATFORM`）。前端 `lib/ipc.ts` 统一识别该 code，配合 `lib/platform.ts` 提前隐藏入口，正常路径下用户不会碰到这个错误——它只是兜底护栏。

好处：前端 TypeScript 类型定义完全不分叉，`ipc.ts` 一份到底。

---

## 第四部分 · 六个关键技术决策

### D1 · 工作空间路径：移动端没有「选目录」这回事

**问题。** `identity.rs:51`：

```rust
pub fn config_base_dir() -> PathBuf {
    std::env::var("APPDATA").map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}
```

Android / iOS 上没有 `APPDATA`，会静默回退到临时目录。**后果是保险库可能被系统在任意时刻清空** —— 这是必须在 M7 就修掉的一级风险。

**决策。**

1. 移动端 vault root 固定为 `<app_data_dir>/workspace`，由 `app.path().app_data_dir()` 解析（Android → `/data/user/0/<pkg>/files`，iOS → `Library/Application Support`），不给用户选择权。
2. `Init.tsx` 的「选择工作空间目录」步骤在移动端跳过，初始化向导直接从「设置访问密码 / 扫码配对」开始。
3. `workspace_path.rs` 的合法性校验整块 `#[cfg(desktop)]`。
4. **关掉系统自动备份**：Android `AndroidManifest.xml` 设 `android:allowBackup="false"`、`android:dataExtractionRules`；iOS 对 vault 目录设 `NSURLIsExcludedFromBackupKey`。理由：`vault.json` 里有访问密码信封，让它进 iCloud / Google 备份等于凭空多一条攻击面，而我们本来就有自己的零知识 S3 通道。这一条要同步写进 README 的安全说明。

顺带把桌面端也改成走 Tauri 的 path API（`%APPDATA%` 硬编码本来就不够体面），行为保持不变。

### D2 · Argon2id 参数是最大的隐性坑

**问题。** `vault/kdf.rs:12` 定义 `MEM_CEIL_KIB = 512 * 1024`，`calibrate()` 以 500ms 为目标抬内存，桌面机器通常会停在 256–512MiB。而 KDF 参数写在 `vault.json` 头部，并由 `sync/engine.rs:206 upload_vault_header` 同步到云端——**所有设备共用同一份参数**。

手机上一次性申请 512MiB 原生内存：中低端 Android 会被 LMK 直接杀进程，iOS 会触发 Jetsam。也就是说，**一个在桌面上正常创建的保险库，手机可能根本打不开，而且报的是「闪退」而不是可理解的错误**。

**三层决策。**

1. **新建保险库默认走跨设备安全上限。** 新增 `MEM_CEIL_MOBILE_SAFE_KIB = 128 * 1024`，标定时以它为上限、用提高 `iters` 补足强度（Argon2id 128MiB + 更多轮次的强度完全够，OWASP 建议值远低于此）。设置里保留「我不用移动端，追求最高强度」的高上限选项。
2. **老保险库不强制迁移，但给一键降参。** 桌面设置页新增「降低 KDF 参数以便手机接入」：本质是用新参数重新包裹密码信封（`envelope::wrap_with_password` 是毫秒级操作，不触碰任何业务数据），完成后推一次云端头部。
3. **移动端日常根本不走密码 KDF。** 入网一次后把 MK 装进本机生物识别信封（D3），日常解锁只有生物识别 / 设备凭证。密码解锁降级为兜底，且必须：① 在后台线程执行；② UI 预告耗时；③ 若 header 里的 `mem_kib` 超过本机安全阈值，**直接拒绝并提示「请在桌面端降低参数，或改用恢复密钥」**，而不是硬跑到闪退。

**一个容易误改的点，先写在这里避免返工：** `kdf.rs:29 parallelism()` 用本机 `available_parallelism()` clamp 到 4。派生时用的是 **header 里存的** `parallelism`，不是本机值，所以手机核数不同**不会**导致解不开——`parallelism()` 只影响新建保险库时的标定。不要「顺手」把它改成本机自适应，那会破坏跨设备解密。

### D3 · 生物识别：现有抽象刚好够用，但必须自己写原生代码

**好消息。** `biometric/mod.rs:5-17` 通过五个自由函数切换后端：

```rust
availability() / enroll(key_ref, challenge) / derive(key_ref, challenge)
verify_presence(prompt) / remove(key_ref)
```

新增 `android.rs` / `ios.rs` 即可，`envelope::wrap_with_biometric` 与 `biometric/store.rs` 完全复用。而且生物识别信封**本来就不上云、天生一机一份**，语义与移动端完美契合。

**关键认知：官方 `tauri-plugin-biometric` 不够用。** 它只提供 `authenticate`（在场确认），对应本项目的「路线 A · 查看 TOTP/密码前重认证」，够用；但**做不了路线 B 的硬件绑定信封**（它不给你任何可用作 KEK 原料的硬件输出）。所以：

- 路线 A → 直接用官方插件，省事。
- 路线 B → **必须自己写 Kotlin / Swift 插件代码。这是整个移动端唯一必须写原生代码的地方。**

**Android 映射。** Keystore 生成 `HMAC-SHA256` 密钥：`setUserAuthenticationRequired(true)`、`setInvalidatedByBiometricEnrollment(true)`、尽量 `setIsStrongBoxBacked(true)`。`enroll` = 建密钥 + 对 challenge 出一次 MAC；`derive` = BiometricPrompt 授权后对同一 challenge 再出 MAC（HMAC 确定性，两次结果一致）。与 Windows Hello「签名 challenge」语义一一对应；重新录指纹会使密钥失效，正好触发现有的 `AppError::BiometricStale` 与 `status.stale` 流程，前端不用改。

**iOS 映射（有个坑）。** Secure Enclave 不支持 HMAC。直觉方案是 P-256 密钥 + `SecKeyCreateSignature`，但 **ECDSA 签名带随机数，同一 challenge 两次签名结果不同，不能当 KEK 原料**。两个可行解：

- 用 `kSecKeyAlgorithmECDHKeyExchangeStandardX963SHA256` 做 ECDH（确定性），密钥留在 Secure Enclave；
- **（推荐）** 把 32 字节随机 secret 存进 Keychain，`kSecAttrAccessControl = biometryCurrentSet`、`kSecAttrAccessible = WhenUnlockedThisDeviceOnly`，`derive` = 取出 secret 后 `HMAC(secret, challenge)`。

推荐后者：与现有 macOS `security-framework` 实现同源，代码可大量复用；`biometryCurrentSet` 同样能在改动 Face ID 录入后自动失效，保住 `stale` 语义。

**别忘了：iOS 不是 `target_os = "macos"`。** 现有所有 `#[cfg(target_os = "macos")]` 都不覆盖 iOS，凡是要共享的地方必须显式写 `any(target_os = "macos", target_os = "ios")`。这条会在 Cargo.toml 和源码里各踩一次。

iOS 还需在 Info.plist 补 `NSFaceIDUsageDescription`，否则调用即崩。

### D4 · 入网：手机怎么拿到 MK

三条路，建议都做：

**路线 A（P0）· 恢复密钥 + 云端拉取。** 恢复密钥走 HKDF（`envelope.rs:37 recovery_kek`），**完全不吃 Argon2**，天生适合手机。`sync/engine.rs:244 preview_cloud_restore` 与 `:307 restore_from_cloud` 直接复用。唯一体验障碍：用户得在手机上手打 S3 endpoint / bucket / access key / secret —— 所以路线 B 才是体验关键。

**路线 B（P0）· 扫码配对。** 桌面端新增「添加手机」：

1. 桌面生成一次性配对密钥（32B 随机），用它 HKDF 出 KEK 包裹 MK，作为 `pair/<随机id>` 对象上传到用户自己的桶；
2. 二维码里只放 `{endpoint, bucket, ak, sk, pairId, pairKey}`（< 500 字节，QR 容量绰绰有余）；
3. 手机扫码 → 拉取 → 解出 MK → **立即写入本机生物识别信封** → 删除 `pair/` 对象。

安全约束（必须实现，不是建议）：二维码等同于全库凭证，所以桌面侧要**限时显示（120 秒）后自动失效**、显示期间禁止截屏、手机取回后立刻服务端删除对象、配对密钥一次性不复用。

**路线 C（P2）· 离线 `.gambackup` 导入。** 走系统文件选择器，`sync/backup.rs` 直接复用。适合完全不配云存储的用户。

### D5 · 同步方向：手机默认只读，防止把桌面数据推没了

**问题。** `sync/engine.rs:575 push_to_cloud` 的语义是「以本地全量状态生成 manifest」。手机如果在**未完整 pull** 的状态下 push，桌面独有的对象会被判成「已删除」。这是本项目移动端最可能造成真实数据损失的一条路径。

**决策（四条硬规则）。**

1. 手机启动后必须 `pull_from_cloud` 成功，才解锁写操作。复用现有 `writesLocked` / `startupNote` 机制 —— `Layout.tsx:110` 已经有这个提示条 UI，直接搬。
2. 手机端只允许改 **TOTP、隐私账号、图标、分组**（这些按 id 合并友好）；**身份、密钥、仓库在移动端只读**。
3. `apply_pulled_ssh`（`engine.rs:1053`）在移动端不碰任何本机 ssh 目录，只保留 vault 内正本。
4. `ssh_text_for_sync`（`engine.rs:1095`）在移动端**原样回传拉到的内容**，绝不能让手机把 ssh 配置「改写成手机路径」再推上去污染云端。

第 3、4 条必须配单元测试锁死（M10 验收项）。

### D6 · 移动端特有的安全面

| 项 | 措施 |
|---|---|
| **进后台即锁** | 前端 `visibilitychange` → `vault_lock`；再加 Android `onStop` / iOS `applicationDidEnterBackground` 原生兜底。桌面的「开机免验证天数」在移动端语义改为「生物识别免密」 |
| **防截屏 / 任务切换器泄露** | Android 给 Activity 加 `FLAG_SECURE`；iOS 在 `willResignActive` 时盖一层遮罩视图 |
| **剪贴板** | `arboard` → `tauri-plugin-clipboard-manager`（Android 仅 SDK 28+ 支持，低版本会写入空串，要在 UI 上处理）。**Android 13+ 复制后会弹内容预览气泡，必须设 `ClipDescription.EXTRA_IS_SENSITIVE`**，否则 TOTP 验证码直接显示在系统 UI 上 |
| **剪贴板定时清空** | 现有「N 秒自动清空」（`commands/secrets_ui.rs`）依赖进程内定时器，App 进后台后 Android 不保证执行 → 改为「进后台立即清空」+ 前台定时器双机制 |
| **环境自查改写** | root / 越狱检测、是否设置了屏幕锁、系统版本、开发者选项与 USB 调试状态 |
| **Android 权限最小化** | 只申请 `INTERNET` + `CAMERA`（扫码，且运行时申请）。不要 `READ_EXTERNAL_STORAGE`，文件导入走 SAF |

---

## 第五部分 · 里程碑计划

总体：**Android 主线 M7–M12 约 5–6 周（1 人）；iOS 增量约 2–3 周（前提是有 Mac）。**

---

### M7 · 编译打通与骨架生成（1 周）

目标：`tauri android dev` 能在真机上启动并显示现有界面（功能可以是残的，但不能崩）。

因为 Android SDK/NDK 尚未安装（P0），M7 拆成两半：**M7a 纯源码改造，可在 Windows 上用桌面构建验证**；**M7b 需要 SDK，由 Android 编译器驱动**。

#### M7a · 源码改造（✅ 已完成，2026-09-11）

| 任务 | 状态 | 落地位置 |
|---|---|---|
| Cargo.toml 依赖 target 条件化 | ✅ | 见下表 |
| `AppError::Unsupported` + `UNSUPPORTED_PLATFORM` code | ✅ | `error.rs` |
| `PlatformOps` 的移动端 no-op 实现 | ✅ | `platform/mobile.rs`（新增） |
| `updater_target()` 补 android/ios；`self_update_supported()` 移动端返回 false | ✅ | `platform/mod.rs` |
| **D1 路径注入点** | ✅ | `identity.rs` 新增 `init_base_dir()` + `OnceLock`；`setup()` 里移动端注入 `app_data_dir()` |
| `AppState` 注册挪进 `setup()`（保证沙箱路径先于 `AppConfig::load()`） | ✅ | `lib.rs` |
| 应用级桌面接线全部 `#[cfg(desktop)]`（tray / single_instance / updater 插件与调度 / agent bootstrap / 关闭行为 / ssh 迁移） | ✅ | `lib.rs` |
| capabilities 按平台拆分 | ✅ | `capabilities/default.json`（加 `platforms`）+ `capabilities/mobile.json`（新增） |
| Android 平台覆盖配置（`minSdkVersion: 28`） | ✅ | `tauri.android.conf.json`（新增） |
| Rust Android target 安装 | ✅ | 4 个 target 就位 |

`minSdkVersion` 定 28 的理由：`tauri-plugin-clipboard-manager` 在 SDK 28 以下会写入空串（D6），而 D3 要用的 `setUserAuthenticationParameters` / StrongBox 也要 28+。

**M7a 验收结果**：桌面 `cargo test` **238 passed / 0 failed**、`cargo check` 零警告；`cargo tree --target aarch64-linux-android` 中 `arboard` / `xcap` / `rfd` / `tauri-plugin-updater` / `tray-icon` **全部不再出现**，而 Windows 依赖图中它们仍在（无桌面回归）。

#### M7b · 需要 Android SDK 之后（待办）

1. 环境：Android Studio + SDK Platform / Platform-Tools / Build-Tools / Command-line Tools / NDK(Side by side)；设 `JAVA_HOME`、`ANDROID_HOME`、`NDK_HOME`。
   > **坑：`NDK_HOME` 必须指向具体版本目录**（如 `...\ndk\29.0.13113456`）。官方文档里的 `$(ls -1 $ANDROID_HOME/ndk)` 在装了多个 NDK 时会拼出非法路径，这是 tauri#11841 的已知问题。
2. `tauri android init`，提交 `gen/android/`。
3. **模块级 cfg 级联**：`cargo check --target aarch64-linux-android` 会逐个报出编不过的模块（`tray`、`agent`、`ssh/managed|config|connect|toolchain`、`update`、`clipboard`、`qrscan`、`autostart`、`single_instance`、`git/repo`、`sys` 等）。按编译器提示逐个加 `#[cfg(desktop)]`，并把对应的 `commands/*` 函数体在移动端改为返回 `AppError::Unsupported`（保持单一 `generate_handler!` 注册表，见 3.3）。
   > 这一步**刻意留到有编译器反馈时再做**：盲改会遗漏，也容易误伤桌面。
4. `tauri android dev` 真机启动。

原 M7 任务 2 的依赖条件化明细（已完成，留档备查）：

   > ⚠️ **Cargo 的 target cfg 表达式不认 Tauri 的 `desktop` / `mobile` 别名**（那是 `tauri-build` 注入的 rustc cfg，只能用在源码里）。Cargo.toml 里必须显式列举 target_os。

   | 依赖 | 现状 | 改为 |
   |---|---|---|
   | `arboard` | 无条件 → **移动端编不过** | `cfg(any(target_os="windows", target_os="macos", target_os="linux"))` |
   | `xcap` | `cfg(not(target_os="linux"))` → **会被 Android/iOS 拉进来** | `cfg(any(target_os="windows", target_os="macos"))` |
   | `rfd` | `cfg(not(windows))` → **同样会被拉进来** | 显式桌面三平台 |
   | `tauri-plugin-updater` | 无条件 | 桌面三平台 |
   | `tauri` 的 `tray-icon` feature | 无条件 | 用 feature + target 依赖拆分 |
   | `security-framework`、`objc2-*` | `cfg(target_os="macos")` | 需共享的改 `any(macos, ios)` |
   | 新增（留到用时再加） | — | `tauri-plugin-biometric`、`tauri-plugin-barcode-scanner`、`tauri-plugin-clipboard-manager`、`tauri-plugin-os`，全部 `cfg(any(target_os="android", target_os="ios"))` |

   插件依赖刻意不在 M7a 加：没有 Android 工具链就无法编译验证，加了等于放一堆未经检验的版本号。等 M8/M9 真正调用时随代码一起加。

**交付物**：Android debug APK 能装能开；`cargo check --target aarch64-linux-android` 通过；桌面三端 `cargo test` 全绿（无回归）。

**验收**：① 真机启动不崩、能看到解锁页；② 保险库目录落在 `/data/user/0/com.jeck.gitkeymaster/files/workspace`（`adb shell run-as` 验证），**不在临时目录**；③ 桌面 CI 无回归。

---

### M8 · 移动端外壳与导航（✅ 已完成，2026-09-11）

**任务与落地**

1. `lib/platform.ts`（新增）：**刻意把「是不是手机」和「要不要紧凑布局」拆成两件事**——
   - `isMobilePlatform()` 决定**能力**（要不要注册桌面专属路由、要不要画窗口按钮）；
   - `useIsCompact()` 决定**布局**（底部 Tab 还是左侧栏，`matchMedia("(max-width: 640px)")`）。

   收益是可验收性：在 Windows 上把浏览器窗口拉窄就能验收移动端布局，不必先装模拟器；而桌面窗口有 840px 最小宽度（`tauri.conf.json`），打包后的桌面端不会误触紧凑布局。
   平台判定用 WebView UA（含 iPadOS 13+ 伪装成 Macintosh 时的 `maxTouchPoints` 兜底），**不引入 `@tauri-apps/plugin-os`**——没有 Android 工具链时加依赖等于放一个无法编译验证的版本号。
2. `ui/MobileShell.tsx`（新增）：顶部精简栏（Logo / 名称 / 锁定状态 / 主题 / 立即锁定）+ 可滚动内容区 + 底部 5 Tab（总览 / 验证码 / 账号 / 同步 / 设置），`env(safe-area-inset-*)` 适配刘海与手势条。不显示工作空间路径（移动端路径固定在沙箱内，用户无从选择也无需知道）。
3. `App.tsx`：按 `compact` 选外壳、移动端不渲染 `TitleBar`；`/config`、`/agent`、`/repos`、`/clone`、`/identities/new` 在移动端**连路由都不注册**，避免深链接或历史记录把用户带到一个必然报错的页面。
4. `index.css`：新增 `.m-*` 外壳样式 + `@media (max-width: 640px)` 断点（原文件 **0 条媒体查询**）；触摸目标提到 ≥44px（原 `.nav-item` 只有 `padding: 5px 8px`）；并排两栏在竖屏堆叠。
5. `index.html`：viewport 补 `viewport-fit=cover` 并禁用缩放，另加 `color-scheme`。

**验收结果**（Vite dev + CDP 模拟 390×844；用浏览器侧 IPC 桩渲染已解锁态，不改任何产品代码）

- 5 个 Tab 导航正常、激活态高亮正确，设置页等密集页面在竖屏可读；
- `document.scrollWidth == clientWidth == 390`，`.m-content` 内**零个**横向溢出元素；
- 桌面窗口控制按钮在紧凑态**从 DOM 卸载**（可交互元素 8 → 3），证明是 React 条件渲染生效而非仅 CSS 隐藏；
- `tsc -b && vite build` 通过，oxlint 无错。

**遗留给 M11 的已知问题**（本次实测看到，不属于 M8 范围）

- 身份总览仍显示 Agent 维度与「一键加载 Agent」红色警告条、「新建身份」按钮——移动端要砍成两维只读；
- 设置页仍有「工作空间与备份」「关于与更新（含应用内自更新）」等桌面项，需按平台隐藏。

---

### M9 · 解锁与生物识别（1.5–2 周 · 最重的原生活）

**任务**

1. **D2 落地**：新增 `MEM_CEIL_MOBILE_SAFE_KIB`；标定加「跨设备兼容」模式；移动端密码解锁挪到后台线程 + 超阈值拒绝 + UI 耗时预告；桌面设置页加「降低 KDF 参数以便手机接入」。
   **→ 纯 Rust 部分已完成（见下方 M9a）；剩余 UI 耗时预告归 M11、后台线程归 M9 主体。**
2. 恢复密钥解锁在移动端打通（HKDF 路径，最先能用）。
3. **`biometric/android.rs` + Kotlin 插件**：Keystore HMAC + BiometricPrompt，实现 `availability / enroll / derive / verify_presence / remove` 五个函数（D3）。
4. 路线 A 重认证接官方 `tauri-plugin-biometric`。
5. **D6 落地**：进后台即锁（前端 + 原生双保险）、`FLAG_SECURE`、剪贴板换插件 + 敏感标记 + 进后台清空。
6. `Unlock.tsx` / `Init.tsx` 移动化：生物识别优先、去掉目录选择步骤。

**交付物**：手机上可用恢复密钥 + 生物识别解锁并锁定。

**验收**：① 128MiB 参数保险库在 4GB 内存真机上解锁不 OOM；② 512MiB 参数保险库给出**可理解的拒绝提示而非闪退**；③ 系统里新增一枚指纹后，App 正确进入 `stale` 状态并要求改用密码/恢复密钥重新启用；④ App 切后台再回来必须重新验证；⑤ 任务切换器里看不到内容；⑥ 复制 TOTP 后系统预览气泡不显示明文。

#### M9a · D2 纯 Rust 部分 ✅ 已完成

不依赖 Android SDK，`cargo test` 可验收。

| 位置 | 改动 |
| --- | --- |
| `vault/kdf.rs` | `MEM_CEIL_MOBILE_SAFE_KIB = 128MiB`；`KdfProfile{CrossDevice,DesktopOnly}`；`local_mem_ceiling_kib()`（按 `cfg(mobile)` 取值）；`ensure_affordable()`；`calibrate_with(target, profile)`，`calibrate()` 默认走 `CrossDevice` |
| `error.rs` | `KdfTooHeavy{needed_mib,ceiling_mib}` → code `KDF_TOO_HEAVY`，文案直接给出"桌面端降参"或"恢复密钥"两条出路 |
| `vault/mod.rs` | `unlock_with_password` / `verify_password` / `change_password` 前置 `ensure_affordable`；新增 `kdf_params()` 与 `relax_kdf(password, target)` |
| `commands/vault.rs` | `get_kdf_info`（含 `mobileCompatible`）、`relax_kdf_for_mobile`（标定 → 降参 → `kick_publish`） |
| `lib.rs` / `ipc.ts` | 注册两个命令 + 前端 `KdfInfo` 类型与调用封装 |

三个关键设计取舍：

- **`clamp_to_safe_bounds()` 的 512MiB 上限不动。** 它 clamp 后的值直接参与 KEK 派生，收紧上限会让历史重参数保险库算出不同 KEK、**永久打不开**。已加回归测试 `clamp_preserves_legacy_heavy_params` 锁死这一点。兼容性改从两头解决：新建时 `CrossDevice` 标定收口，老库靠 `relax_kdf` 主动降参。
- **`relax_kdf` 只重新包裹密码信封**，毫秒级，不重加密任何业务数据；恢复信封（HKDF）与生物识别信封都不依赖 KDF 参数，因此不受影响（测试已断言降参后恢复密钥仍可解锁）。目标参数由调用方传入而非内部标定，便于测试注入快参数。落盘失败会回滚内存头部，避免"内存已换、磁盘还是旧的"分叉。
- **护栏不加在 `unlock_with_recovery`**：HKDF 路径本来就便宜，这也正是超限保险库在手机上的逃生通道。

**已验收**：`cargo test` 245 passed / 0 failed，含 7 个新增用例（上限关系、跨设备标定不越界、护栏边界值放行/超限拒绝、历史参数 clamp 不变、降参幂等、降参后密码与恢复密钥双路可解锁、错误密码与仍超限目标被拒）。

---

### M10 · 入网配对与同步（1.5 周）

**任务**

1. **D4 路线 B**：`mobile/pairing.rs` + 桌面「添加手机」二维码（限时 120 秒、禁截屏、一次性、取回即删）；手机侧 `Pair.tsx` 用 `tauri-plugin-barcode-scanner` 扫码。
2. **D5 落地**：移动端 `pull` 优先 + `writesLocked` 门禁；身份/密钥/仓库只读；`apply_pulled_ssh` 与 `ssh_text_for_sync` 的移动端分支。
3. S3 配置的手工录入表单移动化（作为扫码失败的兜底）。
4. `sync/scheduler.rs` 移动端策略调整：不做后台定时同步（移动端后台执行不可靠），改为**前台恢复时同步**。

**任务外但必须做的测试**

5. 单元测试锁死 D5 第 3、4 条：构造「手机 pull 后 push」场景，断言云端 ssh 正本与桌面独有对象**不被改写、不被删除**。

**交付物**：桌面出码 → 手机扫码 → 自动拉取全量数据 → 写入本机生物识别信封 → 可离线解锁。

**验收**：① 全新手机 90 秒内完成入网；② 手机改 TOTP 后桌面能拉到；③ **手机 push 后桌面的身份/密钥/ssh 配置零变化**（数据安全红线）；④ 配对二维码超时后失效、`pair/` 对象已删除。

---

### M11 · 核心页面移动化（2 周）

**任务**

1. `Totp.tsx`：移动端主场。相机扫码替代屏幕扫码；验证码大字号 + 倒计时环 + 点击复制 + 长按菜单；分组横向滑动。
2. `Accounts.tsx`：列表/详情两级（移动端不适合桌面的并排布局）；密码二次验证接生物识别；历史版本移动化。
3. `Overview.tsx`：身份只读卡片，状态四维砍成两维（密钥 / 配置存在性）；公钥「复制 / 系统分享」。
4. `Sync.tsx`：推拉进度、快照列表、恢复确认的移动化。
5. `Settings.tsx`：移动端隐藏自启动 / 托盘 / 关闭行为 / 应用内更新；新增「进后台即锁」「生物识别」「环境自查（改写版）」。
6. `security/` 检测项改写（root/越狱、锁屏、系统版本）。

**交付物**：5 个核心 Tab 功能完整可用。

**验收**：TOTP 从启动到读出验证码 ≤ 3 次点击；单手可完成全部高频操作；深浅色主题在移动端正确（`index.css` 的多主题变量已有，只需验证）。

---

### M12 · 打包、签名、分发与 CI（1.5 周）

**任务**

1. Android：生成上传 keystore（**离线备份两份，P4**）、`gen/android` 签名配置、`tauri android build` 出 APK + AAB、版本号与 `versionCode` 策略。
2. `ci.yml` 增量：`cargo check --target aarch64-linux-android`（用 `nttld/setup-ndk`）；有 Mac 后加 `aarch64-apple-ios`（`cargo check` 不需要完整签名）。
3. `release.yml` 增量：`v*` 标签触发 Android APK/AAB 签名产物，keystore 与口令进 GitHub Secrets；沿用现有中英双包与 `.sig` 的组织方式。
4. 移动端**关闭自更新入口**（`platform::self_update_supported()` 返回 false），文案改为「请到应用商店更新」。
5. 文档：README 增补移动端安装/入网/安全说明；更新 `docs/release-download-guide.md`、`docs/发布新版本流程.md`；CHANGELOG。
6. 合规：Android 数据安全表单（声明「不收集、不共享」）；iOS 需额外声明 `ITSAppUsesNonExemptEncryption`（用了非豁免加密，要填出口合规）。

**交付物**：可分发的签名 Android 包 + 完整发版流程文档。

**验收**：全新手机安装签名包 → 扫码入网 → 生物识别解锁 → 读 TOTP，全链路通过；CI 在 `dev` 推送时能拦住移动端编译回归。

---

### M13 · iOS 增量（2–3 周 · 依赖 P1/P2）

架构在 M7–M11 已铺平，iOS 剩下的主要是：

1. Mac 上 `tauri ios init`，提交 `gen/apple/`；Info.plist 补 `NSFaceIDUsageDescription` + `NSCameraUsageDescription`。
2. `biometric/ios.rs` + Swift 插件（D3 推荐方案：Keychain + `biometryCurrentSet` + HMAC）。
3. `willResignActive` 遮罩、vault 目录排除 iCloud 备份。
4. 证书 / Provisioning Profile / TestFlight / App Store 审核。

**iOS 审核风险预判**：本类应用（本地加密密钥管理、不含第三方登录、不收集数据）通常无实质障碍，但要准备好①出口合规声明；②隐私清单（PrivacyInfo.xcprivacy）；③审核员看不到「有意义功能」时的说明文案（因为没有账号体系，审核员一进来是空库——建议准备演示用的说明与截图）。

---

## 第六部分 · 风险登记

| # | 风险 | 等级 | 应对 |
|---|---|---|---|
| R1 | 桌面老保险库 512MiB 参数导致手机闪退 | **高** | D2 三层方案；M9 验收项②专门测它 |
| R2 | 手机 push 覆盖/删除桌面数据 | **高** | D5 四条硬规则 + M10 单测锁死 |
| R3 | 移动端沙箱目录被误解析到临时目录，保险库丢失 | **高** | D1；M7 验收项②用 `adb` 实测路径 |
| R4 | iOS Secure Enclave 签名不确定性，误用 ECDSA 做 KEK 原料 | 中 | D3 已定方案（Keychain + HMAC），M13 先写 roundtrip 测试再接 UI |
| R5 | 配对二维码泄露 = 全库失守 | 中 | 限时 120s、一次性、禁截屏、取回即删 |
| R6 | 无 Mac 导致 iOS 线停滞 | 中 | Android 先行，架构已兼容，不阻塞主线 |
| R7 | Android keystore 丢失，应用无法再更新 | 中 | M12 强制两份离线备份 |
| R8 | Android 13+ 剪贴板预览泄露 TOTP | 中 | `EXTRA_IS_SENSITIVE`，M9 验收项⑥ |
| R9 | 一套代码四端，桌面回归 | 中 | cfg 分层纪律（3.2）+ CI 保持桌面三平台 `cargo test` |
| R10 | App Store 审核以「功能不完整」拒绝 | 低 | M13 准备演示说明与截图 |

---

## 第七部分 · 执行顺序建议

```
立即可做（不依赖任何采购决策）
  └─ M7 → M8 → M9 → M10 → M11 → M12   [Android 完整闭环，约 5–6 周]

并行推进的决策项
  ├─ P4 Android keystore 保管方案      （M12 前必须定）
  ├─ P3 是否上 Google Play             （可选，不影响 APK 直发）
  └─ P1/P2 Mac + Apple 开发者账号      （定了才开 M13）

有 Mac 之后
  └─ M13 iOS 增量                      [约 2–3 周]
```

**第一步具体动作**：在 `dev` 分支上开 M7，从 Cargo.toml 的 target 条件化 + D1 路径修复起手 —— 这两件事即使后来移动端不做，也是对桌面版有益的技术债清理（`arboard` 无条件依赖、`%APPDATA%` 硬编码）。

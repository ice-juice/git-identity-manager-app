# 2FA/TOTP 与隐私账号管理设计

> 配套文档：`规划设计.md`（保险库总体架构）、`程序更新功能设计.md`、`网络代理设置设计.md`
> 本文定位：在现有 Git 身份/密钥保险库之上，新增两个消费级模块——**2FA/MFA(TOTP) 管理**与**隐私账号（平台账号密码）管理**，复用同一套加密、二次验证与云同步基建。
> 制定日期：2026-09-10
> 状态：实现中（P1–P6 已按本文落地）

---

## 第一部分 · 目标与范围

### 1.1 要解决什么

本应用已经是一个本机加密保险库，天然适合再托管两类高敏感机密：

| 模块 | 用户诉求 |
|---|---|
| 2FA / TOTP | 集中保管各平台的两步验证密钥，随时取一次性验证码，避免依赖手机 App |
| 隐私账号 | 保管平台账号密码，尤其解决"同一平台多账号"（如多个 GitHub 账号）的归类与检索 |

两者与现有 SSH 密钥/身份管理在形态上高度同构：**存机密 → 分组 → 检索 → 二次验证后查看**。因此本设计的第一原则是**最大化复用现有基建，不重造轮子**。

### 1.2 功能目标

#### 模块一：2FA / MFA (TOTP)

| 编号 | 目标 |
|---|---|
| A1 | 手动导入：粘贴 Base32 密钥 + 填写 issuer/account/算法/位数/周期 |
| A2 | otpauth URI 导入：解析 `otpauth://totp/...` 一次回填全部参数 |
| A3 | 二维码图片导入：选择本地图片，解码得到 otpauth URI |
| A4 | 屏幕二维码识别：一键截取所有显示器全屏，自动扫码，多命中让用户选择 |
| A5 | 添加备注、关联平台网站 URL、自定义图标（内置图标库 + 本地上传自动裁剪为 128x128 正方形并高压缩） |
| A6 | 分组管理（自定义分组、组内排序） |
| A7 | 快速检索（issuer/account/note/group/url 即时模糊匹配） |
| A8 | 获取一次性密码：进入界面默认掩码遮挡（`••••••`），点击小眼睛图标 👁️ 才展示 6/8 位验证码与环形倒计时；支持网格(方块)与紧凑列表双布局形态切换 |
| A9 | 每次查看验证码需通过访问密码二次验证（可吃免密查看时效） |
| A10 | 设置中可配置"免密查看时效"（独立于开机免密解锁） |
| A11 | 导入后可重新取回原始密钥 / otpauth 链接 / 重新生成 otpauth 二维码，便于迁移到其他设备；每次查看原始密钥必须验证访问密码 |

#### 模块二：隐私账号管理

| 编号 | 目标 |
|---|---|
| B1 | 保存平台账号：平台名、用户名/邮箱、密码、URL、备注、标签、自定义图标（内置或自定义上传） |
| B2 | 分组管理（自定义分组、组内排序） |
| B3 | 同平台多账号：按平台聚合，组内区分并排列多个账号；支持平台单卡片折叠/展开与顶部一键全展开/全折叠 |
| B4 | 快速检索（平台/用户名/备注/标签/URL 多词 AND 匹配） |
| B5 | 密码二次验证后查看/复制；用户名可免验证复制 |
| B6 | 可选联动 2FA：账号关联到某个 TOTP 条目，内嵌显示验证码（默认掩码，点 👁️ 查看） |
| B7 | 密码历史版本：改密时自动留存旧密码，可回看/回滚；入口低调（默认收起，非常用） |

### 1.3 非目标（本期不做）

- 不做浏览器自动填充 / 扩展集成。
- 不做密码强度分析 / 泄露检测 / 密码生成器（可后置为增强项）。
- 不做 HOTP（计数型）；仅做 TOTP（时间型），覆盖绝大多数平台。
- 不做 Google Authenticator 的 `otpauth-migration://` 批量迁移解析（后置增强项，见 §8）。
- 不做实时摄像头扫码（用截屏方案替代，规避权限与兼容性）。

---

## 第二部分 · 数据模型

### 2.1 元数据 / 机密分离（沿用现有铁律）

现有架构把**可展示元数据**（`VaultData`，可跨 IPC 回前端）与**机密**（`Secrets`，只在 Rust 内存出现、绝不跨 IPC）分离。新模块严格照此执行：

- 元数据（issuer、备注、URL、分组、算法参数等）→ 新增两个加密文件，可返回前端用于列表渲染。
- 机密（TOTP 种子、账号密码）→ 并入现有 `Secrets`，**只在生成验证码/reveal 时于 Rust 内使用**，前端只拿到"当前验证码"或 reveal 出的明文终值。

### 2.2 新增结构（`model/mod.rs`）

```rust
/// 一个 TOTP 条目（元数据；种子单独存 Secrets）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TotpEntry {
    pub id: String,
    pub issuer: String,            // 平台/服务名，如 "GitHub"
    pub account: String,          // 账号标识，如 "techn4950"
    pub note: Option<String>,
    pub url: Option<String>,      // 关联平台网站
    pub group: Option<String>,
    pub algorithm: String,        // "SHA1"(默认) / "SHA256" / "SHA512"
    pub digits: u8,               // 6(默认) / 8
    pub period: u32,              // 30(默认)
    pub icon: Option<String>,     // 图标标识："builtin:<name>" 或 "custom:<sha256>"
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,       // 多端合并按此取新
}

/// 一个隐私账号（元数据；密码单独存 Secrets）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountEntry {
    pub id: String,
    pub platform: String,         // 平台名（多账号聚合键）
    pub username: String,         // 登录名/邮箱（同平台内区分键）
    pub display_name: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub icon: Option<String>,     // 图标标识："builtin:<name>" 或 "custom:<sha256>"
    pub pinned: bool,             // 置顶
    pub sort_order: i32,
    pub totp_ref: Option<String>, // 关联的 TotpEntry.id（可选）
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 分组元数据（TOTP 与账号各持一份）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GroupMeta {
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i32,
}

/// 新增加密容器（data/totp.enc）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TotpData {
    pub entries: Vec<TotpEntry>,
    pub groups: Vec<GroupMeta>,
    #[serde(default, serialize_with = "serde_maps::ordered_string_map")]
    pub deleted_entries: HashMap<String, String>, // id -> 删除时间（墓碑）
}

/// 新增加密容器（data/accounts.enc）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountData {
    pub entries: Vec<AccountEntry>,
    pub groups: Vec<GroupMeta>,
    #[serde(default, serialize_with = "serde_maps::ordered_string_map")]
    pub deleted_entries: HashMap<String, String>,
}
```

### 2.3 机密扩展（`model/mod.rs` 的 `Secrets`）

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountSecret {
    pub password: String,
    /// 预留：安全问题、备用邮箱等自定义字段。
    #[serde(default)]
    pub extra_fields: HashMap<String, String>,
    /// 密码历史版本（旧密码，改密时前插；见 §5.5）。
    #[serde(default)]
    pub history: Vec<PasswordHistoryItem>,
}

/// 一条历史密码（机密，随 Secrets 加密存储，绝不跨 IPC 除 reveal）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PasswordHistoryItem {
    pub password: String,
    pub replaced_at: String, // 该密码被替换（下线）的时间
}

// Secrets 增加两个字段（#[serde(default)] 平滑升级旧库）：
//   pub totp_seeds: HashMap<String, String>,        // totpId -> Base32 种子
//   pub account_secrets: HashMap<String, AccountSecret>, // accountId -> 密码等
```

> `Secrets` 已随云同步作为 `data/secrets.json` 整体传输；把种子/密码放进去天然获得同步与备份能力。代价是任一改动会重传整个 secrets（数据量小，可接受；后续变大再拆分单独对象）。

### 2.4 存储落盘（沿用 `store/mod.rs` 的 seal/open）

```
<工作空间>/data/
├── identities.enc   （现有：VaultData）
├── secrets.enc      （现有 + 扩展 totpSeeds / accountSecrets）
├── totp.enc         （新增：TotpData）
└── accounts.enc     （新增：AccountData）
```

沿用 `LABEL_METADATA` 子密钥、`nonce(24) || XChaCha20-Poly1305 密文` 格式。`store/mod.rs` 新增：`load_totp/save_totp`、`load_accounts/save_accounts`，实现与现有 `load_data/save_data` 完全对称。

---

## 第三部分 · 二次验证与"免密查看时效"（已定：独立内存窗口）

### 3.1 为什么不复用开机免密（grace_days）

现有 `session.rs` 的 grace 是"整个 App 免密解锁"，DPAPI 持久化、粒度为天。TOTP 验证码每 30 秒刷新、查看极频繁，若每次弹密码体验很差；而直接复用开机免密又会把"查看机密"的门槛降到和"打开程序"一样，安全语义不清。

**结论（已确认）**：新增一个**独立、内存级、不落盘**的"查看宽限窗口"，专用于 OTP/密码查看，与开机免密解锁解耦。

### 3.2 机制

- `AppState` 新增 `reveal_grace: Mutex<Option<Instant>>`（进程内；锁定/休眠/退出即失效，不写磁盘、不进 DPAPI）。
- 设置项 `reveal_grace_minutes: u32` 存 `AppConfig`（`0` = 每次都验；`1/5/15/30` 分钟可选），风格对齐现有 `grace_days`。
- 流程：
  1. 用户查看某条 OTP/密码 → 若 `reveal_grace` 未过期，直接放行。
  2. 否则弹框输入访问密码 → 后端 `vault.verify_password()` 成功 → 按 `reveal_grace_minutes` 刷新窗口到期时间 → 返回终值。
- 锁定路径（现有 `lock_in_memory`、休眠锁定、自动锁定）统一清空 `reveal_grace`。

### 3.3 后端命令形态

```rust
// 生成验证码；窗口内 password 可为 None，否则必须重认证。
#[tauri::command]
fn totp_generate_code(state, id: String, password: Option<String>) -> Result<TotpCode>;
// -> { code: "123456", period: 30, remainingSeconds: 17 }

// 查看/复制账号密码；同样受 reveal_grace 约束。
#[tauri::command]
fn account_reveal_password(state, id: String, password: Option<String>) -> Result<String>;
```

参考现有 `commands/write.rs::reveal_key_material` 的重认证 + `util::audit` 写审计模式（脱敏，只记 id 与动作，不记明文）。

### 3.4 剪贴板与展示安全（按需查看与默认掩码）

- **默认掩码保护**：进入 2FA 模块时，所有条目的 OTP 验证码默认显示为 `••••••`（不请求后端生成），只有用户点击对应条目的“小眼睛”图标 👁️ 时，才按需向后端请求生成 6/8 位验证码并开始倒计时。隐私账号的密码与关联 2FA 同样默认掩码。
- **布局形态切换**：2FA 管理和账号管理均支持**方块卡片视图（Grid）**与**紧凑列表视图（List）**两种布局，用户可一键切换并在本地配置中记忆偏好。
- **剪贴板自动清空**：OTP/密码复制到剪贴板后**自动清空**（默认 20 秒，可配置）。
- **脱敏审计**：所有"查看/复制/导出"写脱敏审计事件。

---

## 第四部分 · 模块一：TOTP 生成与导入

### 4.1 otpauth 归一化

四种导入方式最终都归一到一组 otpauth 参数：

```
otpauth://totp/{issuer}:{account}?secret={BASE32}&issuer={issuer}
        &algorithm={SHA1|SHA256|SHA512}&digits={6|8}&period={30}
```

- A1 手动：直接填字段。
- A2 URI：`url` crate 解析 query。
- A3 图片：解码二维码 → 得到 URI → 走 A2。
- A4 屏幕：见 §4.3。

### 4.2 TOTP 算法（RFC 6238，建议手写）

已有 `hmac` + `sha2`，仅需补 `sha1`。核心约 30 行：

```
T = floor(unix_now / period)
msg = 8字节大端(T)
hs = HMAC-{alg}(base32_decode(secret), msg)
offset = hs[len-1] & 0x0f
bin = (hs[offset..offset+4] as u32) & 0x7fffffff
code = bin % 10^digits  // 左补零到 digits 位
```

手写而非引第三方，契合项目"最小依赖"风格，且便于审计。

### 4.3 屏幕二维码识别（已定：一键全屏扫码）

- 用户点"扫描屏幕" → Rust 用截屏 crate 抓取**所有显示器**全屏位图。
- 逐屏用二维码解码 crate 扫描，收集所有 `otpauth://` 命中。
- 0 命中 → 提示未发现；1 命中 → 直接回填预览；多命中 → 前端列出让用户选。
- 扫描完成立即释放位图缓冲，不落盘、不进日志。

### 4.4 新增 Rust 依赖

| 用途 | 选型 | 备注 |
|---|---|---|
| Base32 解码 | `data-encoding` | 轻量、无 unsafe |
| SHA1（TOTP 默认） | `sha1` | 补齐 sha2 未覆盖的 SHA1 |
| 二维码解码 | `rqrr`（+ `image`） | 纯 Rust，跨平台 |
| 二维码生成 | `qrcode`（+ `image`） | 用于 A11 重新导出 otpauth 二维码 |
| 截屏 | `xcap` | 跨平台多显示器，主推 |

> TOTP 生成不引 `totp-rs`，手写实现（见 4.2）。

### 4.5 取回原始密钥 / 二维码导出（A11）

很多 TOTP 软件导入后就再也看不到原始密钥，导致换设备时无法迁移。本模块支持**逆向取回**：把已保存的种子 + 元数据重新拼回 otpauth URI，并按需渲染成二维码，让用户用手机 App 或其他管理器重新扫码导入。

- **拼回 URI**：从 `Secrets.totp_seeds` 取种子，结合 `TotpEntry` 的 issuer/account/algorithm/digits/period，按 §4.1 格式生成 `otpauth://totp/...`。
- **重新生成二维码**：用 `qrcode` 把该 URI 渲染成 PNG/SVG，前端展示供扫码。二维码只在内存生成并直接推给前端渲染，**不落盘**。
- **纯文本取回**：也可只显示 Base32 密钥与完整 otpauth 链接（供手动复制），复制后剪贴板按 §3.4 自动清空。
- **强制重认证**：这是最敏感的操作（等于泄露完整种子），**每次都必须走 `vault.verify_password()` 重认证**。

> 设计取舍：与"查看验证码"不同，取回原始密钥**不吃 `reveal_grace` 免密窗口**——因为它一次性交出整把种子，风险远高于看一次 6 位码，因此无论免密窗口是否有效，都强制重新输入访问密码。写独立审计事件"取回 TOTP 原始密钥 id=..."（脱敏，不记种子）。

后端命令：

```rust
// 取回原始密钥 + otpauth 链接（强制重认证，不吃 reveal_grace）。
#[tauri::command]
fn totp_reveal_secret(state, id: String, password: String) -> Result<TotpSecretReveal>;
// -> { secretBase32, otpauthUri }

// 重新生成 otpauth 二维码（强制重认证）。返回 base64 PNG 供前端 <img> 渲染。
#[tauri::command]
fn totp_export_qr(state, id: String, password: String) -> Result<String>; // base64(png)
```

---

## 第五部分 · 模块二：隐私账号与多账号处理

### 5.1 聚合视图与折叠伸缩（解决"一个 GitHub 多账号"）

- 列表默认按 `platform` 聚合分组；组内列出多个 `username`（例如 `GitHub` 组下并列 `techn4950`、`vortaq-ci-bot`、`juice520` 等）。
- **伸缩折叠机制**：
  - 每一个平台卡片均支持点击头部箭头（▾ / ▸）独立折叠与展开，折叠后只显示平台名、图标、账号数量摘要与快捷添加按钮，方便快速通览全局。
  - 顶部操作栏提供“**一键全部展开 / 全部折叠**”快捷开关，配合搜索框即时定位。
  - 同样支持方块卡片网格与紧凑表格列表双布局切换。

### 5.2 组内排序策略（可切换）

`pinned` 置顶 → `last_used_at` 最近使用 → `sort_order` 手动 → 用户名字母序。

### 5.3 检索

搜索框同时匹配 `platform + username + display_name + note + tags + url`，支持 `github techn` 式多词 AND，前端即时过滤并高亮。

### 5.4 交互细节

- 新增时 `platform + username` 重复则提示疑似重复（不强制拦截）。
- 用户名免验证复制；密码走 §3 二次验证。
- 复制/查看更新 `last_used_at`，让"最近使用"排序有效。
- 详情页若 `totp_ref` 非空，直接内嵌显示当前验证码（复用模块一生成命令）。

### 5.5 密码历史版本（B7）

**触发**：`account_update` 检测到密码字段变化时，把旧密码前插进 `AccountSecret.history`（`{ password, replaced_at = now }`），当前密码写 `password`。仅密码变化才留存；改备注/URL 等元数据不产生历史。

**容量与清理**：默认保留最近 N=10 条（可配 `AppConfig.account_history_limit`），超出丢弃最旧的；提供"清空该账号历史"操作。删除账号时其历史随 `account_secrets[id]` 一并移除。

**入口低调（按你的要求）**：详情页不直接铺开历史；在密码行旁放一个不显眼的"历史版本"小入口（如时钟图标/次级菜单），点开才展开列表。列表默认掩码，显示时间 + 掩码密码。

**查看/回滚**：
- 查看某条历史明文 → 走 §3 二次验证（受 `reveal_grace` 约束，与查看当前密码同级）。
- 回滚：把选中历史密码设为当前密码 → 当前密码同时作为一条新历史前插（保证不丢失）→ 写审计。

**安全**：历史即旧密码，等同机密，同级加密、绝不跨 IPC（除 reveal）。所有查看/回滚/清空写脱敏审计。

**同步/备份**：`history` 属于 `AccountSecret`，天然随 `data/secrets.json` 同步与备份，无需额外对象。多端改密可能产生历史分叉——本期采用简单策略：合并时以 `AccountEntry.updated_at` 较新一端的 `AccountSecret`（含 history）为准，不做逐条历史并集（避免复杂度；可后置增强）。

### 5.6 图标系统与高保真压缩管道

为了让 TOTP 列表和平台账号一目了然，本系统提供一套轻量、高保真且极省云端空间的图标方案：

#### 1. 内置主流平台图标库（零存储开销）
- 前端内置 30+ 款开发者与云服务常用平台矢量/SVG 图标（如 `GitHub`, `GitLab`, `Gitee`, `Google`, `Microsoft`, `Apple`, `AWS`, `Cloudflare`, `OpenAI`, `Vercel`, `Docker`, `Linux`, `NPM`, `Telegram`, `Discord`, `Twitter/X`, `Aliyun`, `TencentCloud` 等）。
- 用户录入 Issuer 或 Platform 名称时，系统自动做不区分大小写模糊匹配，自动推荐并预选内置图标。
- 标识格式：`builtin:github`，不占用工作空间与云端任何物理存储。

#### 2. 自定义图标上传与自动裁剪压缩管道
用户可为任意小众或私有系统上传自定义图片（支持 PNG / JPG / JPEG / WEBP / SVG / ICO / BMP）：

```text
[用户选择本地图片] 
       │
       ▼
[Rust 侧 image crate 解码] 
       │
       ▼
[自动居中裁剪为 1:1 正方形] (取短边居中最大正方形)
       │
       ▼
[Lanczos3 抗锯齿缩放到 128x128] (兼顾 Retina 高清与极致轻量)
       │
       ▼
[高保真无损/近无损压缩] (WebP 质量 92 或 Oxipng 深度压缩 PNG)
       │
       ▼
[生成内容哈希 SHA256] ──> 单张图标体积压缩至 2KB ~ 6KB
       │
       ▼
[落盘 <工作空间>/icons/<hash>.webp] (内容寻址，相同图片全局去重)
```

#### 3. 云端存储与同步优化
- **去重存储**：图标以内容哈希命名（如 `icons/3a7f9c...webp`），多个账号或 TOTP 引用同一图标时零重复开销。
- **极小云端开销**：单张图标仅 2~5KB，即便存储 100 个自定义平台图标，总云端增量也不超过 300KB。
- **同步集成**：`sync/engine.rs` 将 `icons/<hash>.webp` 作为加密逻辑对象整体同步，pull 时无缝还原到本地。

#### 4. 后端命令
```rust
// 接收本地图片路径或二进制，完成裁剪与压缩，落盘并返回 hash 标识
#[tauri::command]
fn icon_upload_custom(state: State<AppState>, file_path: String) -> Result<CustomIconInfo>;
// -> { iconRef: "custom:3a7f9c...", dataUrl: "data:image/webp;base64,..." }

// 获取内置图标列表
#[tauri::command]
fn icon_list_builtin() -> Result<Vec<BuiltinIconInfo>>;
```

---

## 第六部分 · 云同步与备份集成

改动集中在 `sync/engine.rs` 与 `model/mod.rs` 合并逻辑：

| 位置 | 改动 |
|---|---|
| `push_to_cloud` 的 `logical_objects` | 增加 `data/totp.json`、`data/accounts.json`（种子/密码已含于 `data/secrets.json`，自动带走） |
| `pull_from_cloud_inner` 还原分支 | 增加两个新对象的解包写回 |
| `merge_vault_data` 同层新增合并函数 | `merge_totp_data` / `merge_account_data`：条目按 `updated_at` 取新，`deleted_entries` 墓碑用 `timestamp_newer_or_eq`（照搬身份合并模式） |
| `sync/backup.rs` 的 `BackupPayload` | 增加 `totp_data` / `account_data` 字段；整包加密逻辑不变 |

> 多端合并复用现有"较新覆盖较旧 + 墓碑传播删除"范式（见 `model/mod.rs::merge_vault_data`），无需引入新一致性模型。

---

## 第七部分 · 前端改动（React）

| 位置 | 改动 |
|---|---|
| `app/src/pages/Totp.tsx`（新增） | TOTP 列表/分组/搜索/导入向导/验证码卡片（环形倒计时） |
| `app/src/pages/Accounts.tsx`（新增） | 账号聚合列表/搜索/多账号并列/详情；密码行旁低调的"历史版本"次级入口 |
| `app/src/App.tsx` | 增加 `/totp`、`/accounts` 路由 |
| `app/src/ui/Layout.tsx` | 侧边导航增加两个入口 |
| `app/src/lib/ipc.ts` | 增加类型 + 命令封装（见下） |
| `app/src/ui/`（新增通用件） | 二次验证弹框、OTP 倒计时环、掩码显示/复制即清空 |
| `app/src/pages/Settings.tsx` | 增加"免密查看时效"设置项 |

新增 IPC 命令（`lib.rs` 注册；`commands/totp.rs`、`commands/accounts.rs` 新模块）：

```
totp_list / totp_add / totp_update / totp_delete
totp_generate_code / totp_parse_uri / totp_import_from_image / totp_scan_screen
totp_reveal_secret / totp_export_qr   (取回原始密钥 / 导出二维码，强制重认证)
account_list / account_add / account_update / account_delete
account_reveal_password / account_touch (更新 last_used_at)
account_history_list / account_reveal_history / account_rollback_history / account_clear_history (B7)
icon_upload_custom / icon_list_builtin / icon_get_custom
get_reveal_grace_minutes / set_reveal_grace_minutes
```

命令层保持"薄封装"（`commands/mod.rs` 注释要求），机密绝不跨 IPC 回传。

---

## 第八部分 · 安全要点

- TOTP 种子与账号密码等同私钥：同级静态加密、绝不进日志、绝不跨 IPC（除 reveal 终值）。
- 前端默认掩码；剪贴板自动清空。
- 查看/复制/导入/导出写脱敏审计（`util::audit`）。
- 锁定 / 休眠 / 自动锁定统一清空 `reveal_grace`。
- 屏幕扫码位图仅在内存、用完即弃。
- otpauth URI、二维码内容解析做长度/字符校验，防异常输入。
- 取回原始密钥/导出二维码是最高敏感操作：**强制每次重认证、不吃 `reveal_grace`**，生成的二维码位图不落盘、用完即弃，独立审计。

---

## 第九部分 · 实施阶段与验收

| 阶段 | 内容 | 验收 |
|---|---|---|
| P1 数据层 | 模型 + `store` 读写 + `Secrets` 扩展 + 基础 CRUD 命令 | 单测：落盘密文不含明文、往返一致（照 `store` 现有测试） |
| P2 TOTP 核心 | RFC6238 生成 + 手动/URI/图片导入 + 二次验证 + reveal_grace + 取回原始密钥/导出二维码(A11) | 单测：与已知向量比对；重认证窗口逻辑；URI 拼回与解析往返一致 |
| P3 屏幕扫码 | `xcap` 截屏 + `rqrr` 解码 + 多命中选择 | 手测：屏幕含二维码时正确识别 |
| P4 账号模块 | 多账号聚合/检索/排序 + reveal + 密码历史版本(B7) | 手测：多 GitHub 账号归类正确；改密留存历史、查看需重认证、回滚不丢当前密码 |
| P5 分组 + 前端 + 设置 | 两个页面 + 导航 + 免密时效设置 | 手测：分组/检索/掩码/复制清空 |
| P6 同步 + 备份合并 | `logical_objects` + 墓碑合并 + 备份包 | 单测：合并取新/墓碑；双端往返 |

### 版本内务
- `Cargo.toml` 新增依赖归入新分区注释（如 `# ---- 2FA/账号管理 ----`），与现有 `# ---- M5 ----` 风格一致。
- `CHANGELOG.md` 记录新功能。
- 旧库平滑升级：所有新字段 `#[serde(default)]`，新 `.enc` 文件不存在时返回默认。

---

## 附录 · 待确认/可选增强

- Google Authenticator `otpauth-migration://` 批量导入（protobuf 解析）。
- 密码生成器 / 强度提示。
- 账号自定义字段（安全问题、备用邮箱）UI（模型已预留 `extra_fields`）。
- TOTP 图标自动抓取平台 favicon。

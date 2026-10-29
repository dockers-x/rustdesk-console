# 自有签名客户端

[English](self-signed-client.en.md)

本功能为你自己的 RustDesk 源码构建准备签名配置和品牌资源，不提供客户端在线构建，也不修改官方客户端。官方发布的二进制仍信任其内置公钥，不能验证你的签名。

源码核对基线：客户端 `bd9262397208c509ea97af61f0d44621ef25611c`，其 `hbb_common` 为 `5d52530b636fb296a324260b074780cf559e0093`。升级客户端后，应重新核对下列协议和路径。

## 1. 启用功能

自有签名客户端默认关闭。在系统设置的功能模块区启用“自有签名客户端”，才会显示专属菜单及密钥、签名、品牌、预置选项和文档入口。关闭后，这些入口隐藏，后端拒绝相关操作。

关闭功能不会撤销已下载的 `custom.txt`，也不会让已构建客户端忘记它的验证公钥。需要撤销旧配置时，应重新分发配置或更新客户端信任的公钥。

## 2. 准备独立的签名密钥

这里使用 Ed25519，不是 PEM、RSA 或 ECDSA。你可以在页面一键生成，也可以导入已有密钥。

### 在页面一键生成

1. 用管理员账户进入系统设置，启用“自有签名客户端”。
2. 打开专属菜单，选择“密钥与签名”，点击“一键生成密钥对”。首次生成直接执行；已有密钥时，先备份原密钥文件，再确认替换。
3. 成功后，页面显示验签公钥和指纹。私钥由服务器生成并安全保存，接口不回显私钥，页面也没有私钥导出按钮。
4. 复制“验签公钥（标准 base64）”中的完整内容，保留末尾的 `=`，用于第 4 节的客户端源码替换。不要误用 hbbs 公钥。

不需要先在外部生成密钥才能启用功能。若选择导入，填写私钥及可选的匹配公钥；开启功能后才能导入或清空密钥。关闭时只保留功能开关，不能配置密钥或签名。

| 材料 | 原始字节长度 | 说明 |
| --- | --- | --- |
| Ed25519 seed | 32 | 可用于确定性派生密钥对，必须保密。 |
| libsodium secret key | 64 | 格式为 `seed || public_key`，不是任意 64 字节，也不是 32 字节 seed。 |
| public key | 32 | 用于客户端验签，可以分发。 |
| signature | 64 | 对原始 JSON 字节的 Ed25519 签名。 |

上述字节使用标准 Base64 表示，保留必要的 `=` 填充。导入 64 字节密钥时，派生公钥必须与后 32 字节一致。不要把 OpenSSH、PKCS#8 或 PEM 文本直接当作这种 Base64。

客户端配置签名密钥应独立于 hbbs/hbbr 的服务器密钥、Console 的 JWT 密钥以及 Windows/macOS 应用签名证书。私钥只保留在受控的 Console 配置和离线备份中，不进入客户端源码、构建包、日志或公开仓库。

### 存储位置、备份与恢复

密钥不写入普通 `config.yaml`。独立文件的精确路径是 `<配置文件所在目录>/features/self-signed-client.json`，其中同时保存功能开关、公钥、私钥和已保存的预置配置；Unix 上目录必须为 `0700`、文件必须为 `0600`。启动参数 `-c` / `--config` 决定配置文件路径，相对路径以 Console 进程的工作目录为准。

- 默认 `-c ./conf/config.yaml` 对应 `./conf/features/self-signed-client.json`。
- 本仓库 Docker Compose 将主机 `./conf` 挂载到容器 `/app/conf`，因此容器文件 `/app/conf/features/self-signed-client.json` 对应主机 `./conf/features/self-signed-client.json`。
- 自定义启动参数或容器挂载时，按实际配置路径定位。必须持久化这个目录，不能把密钥放进 `resources`、上传目录、客户端发布包或其他 Web 可访问位置。

备份时先停止 Console，使用服务账户或有相应权限的服务器管理员复制整个文件到加密离线备份位置，不要通过浏览器接口导出或把文件内容打印到日志。以下为 Unix 示例；请把备份路径替换为你的私有备份目录：

```sh
umask 077
install -d -m 700 /safe/offline-backup/rustdesk-console
install -m 600 ./conf/features/self-signed-client.json /safe/offline-backup/rustdesk-console/self-signed-client.json
```

恢复前同样先停止 Console，并备份当前文件。把原文件恢复到新实例实际配置目录的 `features/self-signed-client.json`，保留运行 Console 的服务账户所有权，恢复目录 `0700` 和文件 `0600` 后再启动。默认路径的复制示例：

```sh
install -d -m 700 ./conf/features
install -m 600 /safe/offline-backup/rustdesk-console/self-signed-client.json ./conf/features/self-signed-client.json
```

文件中的开关状态也会恢复。启动后检查公钥和指纹是否与备份前一致，再测试签名。程序会拒绝宽松权限、符号链接或错误文件类型，不能用放宽权限来处理读取失败。此文件包含签名私钥，备份也必须按私钥保护。

公钥、指纹和 `custom.txt` 都不能恢复丢失的私钥。没有备份时只能生成新密钥；要让客户端接受新密钥生成的配置，必须更新 fork 中的验签公钥并重新构建、分发客户端。不要把“一键生成”当作找回旧密钥。

## 3. 签名配置的准确格式

`src/common.rs::read_custom_client` 的读取顺序是：

1. 对 `custom.txt` 做标准 Base64 解码。
2. 用该函数内部的 `const KEY` 验证 Ed25519 attached signature。
3. 从验签返回的消息字节中读取 JSON 对象。

文件内容为 `Base64(signature || UTF-8 JSON)`。它不是明文 JSON、分离的 `.sig` 文件、JWT 或加密配置。签名后不能修改 JSON 的空格、换行或字段顺序；外层 Base64 文本首尾的空白会被读取器去掉。

签名配置可被任何拿到文件的人解码。不要在其中存放 API token、私钥或不应公开的密码。

配置示例中的服务器地址和公钥需要替换为你自己的值：

```json
{
  "app-name": "AcmeDesk",
  "default-settings": {
    "custom-rendezvous-server": "id.example.com:21116",
    "relay-server": "relay.example.com:21117",
    "api-server": "https://console.example.com",
    "key": "YOUR_HBBS_PUBLIC_KEY_BASE64",
    "theme": "dark",
    "enable-webrtc": "Y"
  },
  "override-settings": {
    "enable-file-transfer": "N",
    "hide-server-settings": "Y",
    "display-name": "Acme Support"
  },
  "disable-settings": "N"
}
```

所有选项值都是字符串。不要把 `"Y"` 写成 `true`，或把端口、超时等字符串写成 JSON 数字。未知字段即使能通过读取器，也不代表有功能会消费它。

### 默认、覆盖与内置选项

`libs/base/src/config/keys.rs` 是当前客户端选项分类的依据：

| 分类 | 对应列表 | 示例与边界 |
| --- | --- | --- |
| 会话显示 | `KEYS_DISPLAY_SETTINGS` | `view-only`、`view-style`、`image-quality`、`show-remote-cursor`。 |
| 本地客户端 | `KEYS_LOCAL_SETTINGS` | `theme`、`lang`、`enable-webrtc`、`enable-tcp-punch`、`enable-udp-punch`、`enable-ipv6-punch`、`relay-fallback-delay`。 |
| 被控端和网络 | `KEYS_SETTINGS` | `enable-file-transfer`、`approve-mode`、`custom-rendezvous-server`、`api-server`、`key`、`ice-servers`。 |
| 内置行为 | `KEYS_BUILDIN_SETTINGS` | `display-name`、`avatar`、`hide-server-settings`、`hide-powered-by-me`、`preset-user-name`、`preset-device-group-name`、`preset-strategy-name`。源码中的 `BUILDIN` 是现有拼写。 |

显示、本地和被控端选项各有独立的默认及覆盖表。配置读取优先级为覆盖值高于用户保存值，用户保存值高于默认值。覆盖选项可被当前客户端识别为固定值，但这不是不可绕过的安全隔离。

内置选项写入同一张 `BUILTIN_SETTINGS` 表，不存在独立的默认表与覆盖表。若两组都给同一个内置选项赋值，后读取的 `override-settings` 值覆盖前者。

`display-name` 和 `avatar` 用于连接请求中的身份展示，不是首页 logo。`preset-address-book-*`、`preset-device-name`、`preset-device-username`、`preset-note` 属于被控端配置；`preset-user-name`、`preset-device-group-name`、`preset-strategy-name` 属于内置配置。预置归类依赖设备向 Console 上报系统信息以及目标用户、组、策略确实存在，不是单凭签名就在服务端获得权限。

`enable-webrtc`、打洞选项和 `relay-fallback-delay` 在签名配置中可以进入 `LocalConfig` 对应的默认或覆盖表。Console heartbeat 策略只写 `Config`，不能借此把这些本地选项当作动态策略下发。已有本地保存值也可能覆盖 `default-settings` 的默认值。

### 顶层硬设置

`app-name` 单独处理，会改变运行时应用名称和 `get_uri_prefix()` 生成的 URI scheme。它不会自动替换安装包名称、系统图标、应用标识、快捷方式或代码签名。

其余顶层字符串进入 `HARD_SETTINGS`。当前源码确认的消费项如下：

| 键 | 可用值及作用 |
| --- | --- |
| `conn-type` | `incoming` 仅接收连接；`outgoing` 仅发起连接。省略时不限制为其中一类。 |
| `disable-tcp-listen` | 严格为 `Y` 时禁用相应 TCP 监听路径，不等同于禁用全部网络传输。 |
| `disable-settings` | 严格为 `Y` 时禁用设置入口。 |
| `disable-ab` | 严格为 `Y` 时禁用地址簿。 |
| `disable-account` | 严格为 `Y` 时禁用账户功能。 |
| `disable-installation` | 严格为 `Y` 时禁用客户端的安装路径。 |

这五个开关用 `N` 或省略表示不启用。源码还读取 `password` 和 `salt`，但它们是预置永久密码的专用存储材料，不是普通明文密码字段，本功能不提供其生成或通用编辑。不要把 `logo`、`appicon` 或其他没有消费代码的键当作可用硬设置。

## 4. 在你自己的 fork 中配置信任

1. 完成第 2 节的密钥生成与备份，复制页面显示的完整验签公钥。
2. 克隆你自己的客户端 fork，并初始化其子模块：`git submodule update --init --recursive`。
3. 在 `src/common.rs` 找到 `read_custom_client` 内的 `const KEY`，只把该常量替换为刚复制的 32 字节公钥对应的 Base64 文本。不要替换其他服务器公钥，不要把私钥写进源码。
4. 在 Console 配置应用名称、默认值、覆盖值及硬设置；需要保留草稿时点击“保存预置配置”。再点击“下载签名 custom.txt”。签名下载使用当前页面配置，不等同于保存草稿；下载后不要再编辑文件内容。
5. 按该 fork 的 `.github/workflows/flutter-build.yml` 准备 Rust、Flutter、原生依赖及构建环境。当前 README 的部分简易构建说明针对已弃用的 Sciter，不应当作 Flutter 构建步骤。
6. 准备下面的品牌资源后，使用该平台的 Flutter 构建和打包流程。桌面脚本入口为 `python3 ./build.py --flutter --hwcodec`，它不是免安装依赖的一键跨平台构建服务。
7. 按第 6 节把下载的 `custom.txt` 放入实际发布包的加载位置，再检查安装包是否保留它。停止并重新启动客户端及相关后台进程，检查应用名称、配置和验签结果后再分发。后续配置仍用同一私钥签名时，不必仅因配置内容变化而替换源码公钥，但需要替换部署的 `custom.txt` 并重启。

Console 不替你构建、签署或公证客户端。Windows Authenticode、macOS codesign/notarization 及移动平台签名仍由你自己的发布流程负责。

## 5. Logo 与应用图标

当前 Flutter `loadLogo()` 读取打包资源，不读取 `HARD_SETTINGS["logo"]`、Base64 图片或 Console 图片 URL。签名 `custom.txt` 不能单独改变首页 logo。

把导出的 PNG 资源放入你自己 fork 的以下位置，然后重新构建 Flutter 资源：

```text
flutter/assets/logo.png
flutter/assets/logo_light.png
flutter/assets/logo_dark.png
flutter/assets/icon.png
```

亮色主题优先 `logo_light.png`，暗色主题优先 `logo_dark.png`，缺失时回退到 `logo.png`。Logo 的显示区域最大为 300 × 60 个逻辑像素，保持宽高比；这不是文件必须满足的像素尺寸。建议用透明 PNG，按实际显示大小提供高分辨率图。`icon.png` 是界面图标，缺失或解码失败时回退到现有的 `assets/icon.svg`。

系统应用图标需要另行处理，不能由签名 JSON 修改：

- Windows 可执行文件图标：`flutter/windows/runner/resources/app_icon.ico`，由 `Runner.rc` 引入。
- macOS 应用图标：`flutter/macos/Runner/AppIcon.icns`，由 Xcode 资源与 `Info.plist` 引用。
- Linux 安装包图标与桌面入口：检查 `build.py` 中打包的 `res/128x128@2x.png`、`res/scalable.svg` 及 `.desktop` 文件。
- Android/iOS：分别需要原生应用资源和签名流程，桌面资源包不是移动应用构建包。

托盘还可能使用 Flutter bundle 中的 `icon.png`，缺失时使用编译进 Rust 的平台资源。因此替换界面图标不等于替换所有系统、托盘和安装包图标。

## 6. custom.txt 的加载位置

| 目标 | 已核实的加载规则 |
| --- | --- |
| 桌面 debug | 先读进程工作目录中的 `./custom.txt`；文件可读取时即处理并返回，即使签名错误也不会再尝试旁边的文件。不存在或不可读时再尝试可执行文件旁的路径。 |
| Windows/Linux release | 实际运行的可执行文件所在目录中的 `custom.txt`，不是随意选择的工作目录。便携版应核对解包后真正启动的程序目录。 |
| macOS release | 可执行文件目录再进入 `../Resources/custom.txt`，常见应用包路径为 `YourApp.app/Contents/Resources/custom.txt`。 |
| Android/iOS | 未验证可用的文件旁加载部署路径。当前 Dart 初始化传入空的 `customClientConfig`；存在 Rust/JNI 字符串入口不代表已完成移动端资源注入。 |
| Flutter Web | 不运行这套 Rust 读取器，不能按桌面文件路径加载 `custom.txt`。 |

更换配置后，退出并重新启动客户端及相关后台进程。安装或更新流程是否复制 `custom.txt`，需要按你实际制作的包验收。

## 7. 验收与限制

- 在独立测试环境确认公钥匹配、`custom.txt` 签名可验证，且未把私钥带入发布包。
- 分别检查已有用户配置和新配置目录下的默认值、覆盖值、地址簿预置及 WebRTC 本地选项，避免把旧值覆盖默认值误判为签名失败。
- 检查亮色与暗色 logo、系统图标、安装后加载路径、重启后配置和更新后的资源保留情况。
- 关闭 Console 功能模块后，确认专属菜单隐藏，密钥和签名相关请求被后端拒绝。
- 当前客户端 `src/hbbs_http/record_upload.rs` 的 `ENABLE` 默认是 `false`，未发现启用赋值；`src/server/video_service.rs` 只有开启时才创建上传线程。签名、录屏策略或 Console 的上传接口不能证明自动录制上传已经可用。

自有签名只证明配置来自你信任的私钥持有者，不提供官方 RustDesk 签名、官方商业许可、完整 Pro 功能、服务端连接 ACL 或即时撤销保证。也不能阻止能够修改源码或替换二进制的用户移除限制。分发自己的构建时，仍需遵守客户端及其依赖的许可证和商标要求。

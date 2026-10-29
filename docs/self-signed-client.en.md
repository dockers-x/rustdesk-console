# Self-signed clients

[简体中文](self-signed-client.zh-CN.md)

This feature prepares signed configuration and branding assets for your own RustDesk source build. It does not build clients online or modify official clients. Official binaries still trust their embedded public key and cannot verify signatures made with your private key.

Source baseline: client `bd9262397208c509ea97af61f0d44621ef25611c`, with `hbb_common` at `5d52530b636fb296a324260b074780cf559e0093`. Recheck these contracts and paths when upgrading your client.

## 1. Enable the feature

Self-signed clients are disabled by default. Enable “Self-signed clients” in the feature modules section of system settings to reveal its menu and key, signing, branding, preset-option and documentation controls. Disabling it hides those controls, and the backend rejects the related operations.

Disabling the feature does not revoke an exported `custom.txt` or remove the verification key from a client you have already built. To withdraw an old configuration, distribute a replacement or update the public key trusted by your clients.

## 2. Prepare a separate signing key

This uses Ed25519, not PEM, RSA or ECDSA. Generate a key pair in the UI or import an existing key.

### Generate a key pair in the UI

1. Sign in as an administrator and enable “Self-signed clients” in system settings.
2. Open its menu, select “Keys & signing”, and click “Generate key pair”. The first generation runs directly. If a key is already configured, back up its file before confirming replacement.
3. After success, the server has automatically saved the new key pair, and the page displays the verification public key and fingerprint. Only the generation response also returns the new private key, with `Cache-Control: no-store`; the UI immediately starts downloading `self-signed-client-keypair.json` without retaining the private key in query or mutation caches.
4. Copy the complete “Verification public key (standard base64)” value, including trailing `=` padding, for the source change in section 4. Do not substitute your hbbs public key.

The downloaded JSON contains `algorithm`, `public_key`, `private_key` and `fingerprint`. It is an **unencrypted private-key backup**, not a client configuration or build resource. Use HTTPS and a trusted administrator browser. Confirm the download completed, move it out of shared download or sync folders into encrypted offline storage, and restrict local file access (for example, `chmod 600` on Unix). Never distribute it with clients or upload it to a public repository.

Settings reads and key imports do not return the private key. There is no download endpoint for an existing stored key: each “Generate key pair” action replaces it with a new pair. If the download is blocked or lost, use the server-side backup procedure below rather than regenerating merely to retrieve the current key. To restore a downloaded key-pair backup, enable the feature and import its `private_key` value, optionally with its matching `public_key`; do not paste the whole JSON into the private-key input. This backup does not contain the saved profile or enabled flag.

You do not need an externally generated key to enable the feature. For imports, supply the private key and optionally a matching public key. Key import or clearing requires the feature to be enabled. While disabled, only the feature toggle remains available; keys and signing cannot be configured.

| Material | Raw byte length | Meaning |
| --- | --- | --- |
| Ed25519 seed | 32 | Deterministically derives the key pair. Keep it secret. |
| libsodium secret key | 64 | `seed || public_key`, not an arbitrary 64-byte string or a 32-byte seed. |
| Public key | 32 | Verifies client configuration and may be distributed. |
| Signature | 64 | An Ed25519 signature over the original JSON bytes. |

Represent these bytes with standard Base64, keeping any required `=` padding. For a 64-byte imported key, the derived public key must match its last 32 bytes. OpenSSH, PKCS#8 and PEM text are not interchangeable with this Base64 representation.

Keep this key separate from your hbbs/hbbr server key, Console JWT key, and Windows/macOS application signing certificates. Retain the private key only in controlled Console configuration and offline backups, never in client source, build bundles, logs or public repositories.

### Storage, backup and recovery

Keys are not written into the ordinary `config.yaml`. The exact separate path is `<configuration-directory>/features/self-signed-client.json`. That file stores the enabled flag, public key, private key and saved preset profile together. On Unix, the directory must be `0700` and the file `0600`. The `-c` / `--config` argument selects the configuration file; relative paths resolve from the Console process's working directory.

- The default `-c ./conf/config.yaml` uses `./conf/features/self-signed-client.json`.
- This repository's Docker Compose mounts host `./conf` at container `/app/conf`. The container file `/app/conf/features/self-signed-client.json` therefore maps to host `./conf/features/self-signed-client.json`.
- For custom arguments or mounts, resolve the actual configuration path. Persist this directory. Never place the key file in `resources`, an upload directory, a client release bundle or any Web-accessible location.

The one-time browser download supplements, rather than replaces, a full server-side backup. Stop Console before backing up. Using the service account or an authorized server administrator, copy the entire feature file into encrypted offline backup storage. Do not serve that file through a browser API or print its contents into logs. This Unix example uses a placeholder private backup directory; replace it with your own:

```sh
umask 077
install -d -m 700 /safe/offline-backup/rustdesk-console
install -m 600 ./conf/features/self-signed-client.json /safe/offline-backup/rustdesk-console/self-signed-client.json
```

To restore, stop Console and back up its current file first. Restore the original file at `features/self-signed-client.json` under the new instance's actual configuration directory. Preserve ownership by the account running Console, set the directory to `0700` and file to `0600`, then start Console. For the default path:

```sh
install -d -m 700 ./conf/features
install -m 600 /safe/offline-backup/rustdesk-console/self-signed-client.json ./conf/features/self-signed-client.json
```

The saved enabled flag is restored too. After startup, compare the public key and fingerprint with the original values before testing signing. The application rejects permissive modes, symbolic links and incorrect file types; do not relax permissions to work around a read failure. The backup contains a signing private key and requires the same protection as the live file.

A public key, fingerprint or `custom.txt` cannot recover a lost private key. Without a backup, generate a new key and update your fork's verification public key, then rebuild and redistribute the client to trust the new signatures. “Generate key pair” does not recover the previous key.

## 3. The signed configuration format

`src/common.rs::read_custom_client` performs these steps:

1. Decode `custom.txt` with standard Base64.
2. Verify the Ed25519 attached signature using the function's local `const KEY`.
3. Parse the verified message bytes as a JSON object.

The file contains `Base64(signature || UTF-8 JSON)`. It is not plain JSON, a detached `.sig` file, a JWT or encrypted configuration. Do not change JSON whitespace, line breaks or field order after signing. The loader trims whitespace around the outer Base64 text.

Anyone who has the signed file can decode its contents. Do not include API tokens, private keys or passwords that must remain confidential.

Replace the addresses and server public key in this example with your own values:

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

Every option value is a string. Do not replace `"Y"` with `true`, or encode string-valued ports and timeouts as JSON numbers. A loader accepting an unknown field does not mean any client feature consumes it.

### Defaults, overrides and built-in options

The current client classifies options in `libs/base/src/config/keys.rs`:

| Category | Source list | Examples and boundaries |
| --- | --- | --- |
| Session display | `KEYS_DISPLAY_SETTINGS` | `view-only`, `view-style`, `image-quality`, `show-remote-cursor`. |
| Local client | `KEYS_LOCAL_SETTINGS` | `theme`, `lang`, `enable-webrtc`, `enable-tcp-punch`, `enable-udp-punch`, `enable-ipv6-punch`, `relay-fallback-delay`. |
| Controlled device and network | `KEYS_SETTINGS` | `enable-file-transfer`, `approve-mode`, `custom-rendezvous-server`, `api-server`, `key`, `ice-servers`. |
| Built-in behavior | `KEYS_BUILDIN_SETTINGS` | `display-name`, `avatar`, `hide-server-settings`, `hide-powered-by-me`, `preset-user-name`, `preset-device-group-name`, `preset-strategy-name`. `BUILDIN` is the existing source spelling. |

Display, local and controlled-device options have separate default and override maps. Overrides take precedence over saved user values, which take precedence over defaults. The current client can identify override options as fixed, but this is not tamper-proof security isolation.

Built-in options use one `BUILTIN_SETTINGS` map, not separate default and override maps. If both sections contain the same built-in option, `override-settings` is processed later and replaces the earlier value.

`display-name` and `avatar` identify the connection requester; they are not the home-screen logo. `preset-address-book-*`, `preset-device-name`, `preset-device-username` and `preset-note` are controlled-device settings. `preset-user-name`, `preset-device-group-name` and `preset-strategy-name` are built-in settings. Enrollment presets depend on sysinfo reaching Console and the referenced users, groups and strategies existing. A signature alone does not grant server permissions.

In a signed profile, `enable-webrtc`, punch-through options and `relay-fallback-delay` can populate the default or override maps used by `LocalConfig`. Console heartbeat policies write `Config` instead. They cannot dynamically deliver these local options through the same policy mechanism. Existing saved values may also take precedence over `default-settings`.

### Top-level hard settings

`app-name` is handled separately. It changes the runtime application name and the URI scheme returned by `get_uri_prefix()`. It does not automatically rename installers, system icons, application identifiers, shortcuts or code-signing identities.

Other top-level string fields enter `HARD_SETTINGS`. These consumers are confirmed in the current source:

| Key | Values and effect |
| --- | --- |
| `conn-type` | `incoming` accepts connections only; `outgoing` initiates connections only. Omit it to avoid either restriction. |
| `disable-tcp-listen` | Exactly `Y` disables the relevant TCP listener paths, not every network transport. |
| `disable-settings` | Exactly `Y` disables settings access. |
| `disable-ab` | Exactly `Y` disables the address book. |
| `disable-account` | Exactly `Y` disables account features. |
| `disable-installation` | Exactly `Y` disables the client's installation paths. |

Use `N` or omit one of the five switches to leave it disabled. The source also reads `password` and `salt`, but these hold specialized preset permanent-password storage, not a generic plaintext password. This feature does not generate or expose general editing of them. Do not treat `logo`, `appicon` or other keys without consuming code as supported hard settings.

## 4. Configure trust in your own fork

1. Complete key generation and backup in section 2, and copy the full verification public key displayed in Console.
2. Clone your own client fork and initialize its submodules with `git submodule update --init --recursive`.
3. Find the local `const KEY` inside `read_custom_client` in `src/common.rs`. Replace only that constant with the copied Base64 representation of your 32-byte public key. Do not replace unrelated server keys or put the private key in the source.
4. Set the application name, defaults, overrides and hard settings in Console. Click “Save preset profile” to retain the draft, then “Download signed custom.txt”. Signing exports the current page values; it does not save the draft. Do not edit the downloaded file's contents.
5. Prepare Rust, Flutter, native dependencies and build tools according to that fork's `.github/workflows/flutter-build.yml`. Some short build instructions in the current README target deprecated Sciter, not Flutter.
6. Prepare the branding assets below, then use the platform's Flutter build and packaging process. The desktop script entry point is `python3 ./build.py --flutter --hwcodec`; it is not a dependency-free, cross-platform build service.
7. Place the downloaded `custom.txt` at the release-bundle loading location in section 6 and verify that your installer retains it. Stop and restart the client and relevant background processes. Check the application name, settings and signature before distribution. Later configuration signed by the same private key does not require changing the source public key solely because the values changed, but you must deploy the replacement `custom.txt` and restart.

Console does not compile, code-sign or notarize your clients. Your release process remains responsible for Windows Authenticode, macOS codesign/notarization and mobile platform signing.

## 5. Logos and application icons

The current Flutter `loadLogo()` reads bundled assets, not `HARD_SETTINGS["logo"]`, Base64 images or Console image URLs. A signed `custom.txt` alone cannot change the home-screen logo.

Place exported PNG assets at these locations in your own fork, then rebuild the Flutter assets:

```text
flutter/assets/logo.png
flutter/assets/logo_light.png
flutter/assets/logo_dark.png
flutter/assets/icon.png
```

Light mode prefers `logo_light.png`; dark mode prefers `logo_dark.png`. Both fall back to `logo.png` when the variant is missing. The logo fits within 300 × 60 logical pixels while preserving its aspect ratio. This is a display constraint, not a required source-image resolution. Transparent PNGs with enough resolution for the intended display size are recommended. The in-app `icon.png` falls back to the existing `assets/icon.svg` if missing or undecodable.

System application icons require separate packaging work and cannot be changed by signed JSON:

- Windows executable icon: `flutter/windows/runner/resources/app_icon.ico`, referenced by `Runner.rc`.
- macOS application icon: `flutter/macos/Runner/AppIcon.icns`, referenced by Xcode resources and `Info.plist`.
- Linux package icons and launchers: inspect the `res/128x128@2x.png`, `res/scalable.svg` and `.desktop` files packaged by `build.py`.
- Android/iOS: use each platform's native application resources and signing process. A desktop asset bundle is not a mobile application build bundle.

The tray can also load `icon.png` from the Flutter bundle, falling back to platform resources compiled into Rust. Replacing the in-app icon does not replace every system, tray and installer icon.

## 6. Where custom.txt is loaded

| Target | Verified loading behavior |
| --- | --- |
| Desktop debug | First tries `./custom.txt` in the process working directory. If readable, it processes that file and returns, even if verification fails. If absent or unreadable, it tries the executable-relative path. |
| Windows/Linux release | `custom.txt` beside the executable that actually runs, not an arbitrary working directory. For portable packages, check the directory of the executable started after extraction. |
| macOS release | `../Resources/custom.txt` relative to the executable directory, typically `YourApp.app/Contents/Resources/custom.txt`. |
| Android/iOS | No validated file-based deployment path. Current Dart initialization passes an empty `customClientConfig`; Rust/JNI string entry points do not establish a complete mobile asset-injection flow. |
| Flutter Web | Does not run this Rust loader and cannot use the desktop file-loading paths. |

Exit and restart the client and its relevant background processes after replacing the profile. Verify whether your actual installation and update packages copy `custom.txt`.

## 7. Acceptance checks and limits

- In an isolated environment, confirm that the public key matches, `custom.txt` verifies, and no private key appears in the release bundle.
- Check defaults, overrides, address-book presets and WebRTC local options with both existing user configuration and a fresh configuration directory. Saved values overriding defaults are not signature failures.
- Check light/dark logos, system icons, the installed loading path, configuration after restart and asset retention after updates.
- Disable the Console feature module and confirm that its menu disappears and the backend rejects key and signing operations.
- In the current client, `ENABLE` in `src/hbbs_http/record_upload.rs` defaults to `false`, with no enabling assignment found. `src/server/video_service.rs` creates an upload thread only when it is enabled. A signed profile, recording policy or Console upload endpoint does not establish working automatic recording uploads.

A self-signature establishes that a configuration came from a holder of your trusted private key. It does not provide an official RustDesk signature, commercial license, complete Pro feature set, server-side connection ACL or immediate revocation guarantee. Users who can modify the source or replace the binary can remove client restrictions. Distributing your builds still requires compliance with the client and dependency licenses and applicable trademark requirements.

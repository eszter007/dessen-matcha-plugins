# dessen-matcha-plugins

[matcha-reader](https://github.com/eszter007/matcha-reader) features ported to
[CrossPoint](https://github.com/crosspoint-reader/crosspoint-reader) as
[Dessen](https://github.com/Free-Ink/dessen) apps: SD-card plugins, no firmware fork.

| Plugin | What it does |
|---|---|
| [Insights](plugins/insights) | matcha's Insights screen: streak, this week, books finished, days read, total time, longest streak, sessions, average session, month calendar, language tabs, and a page per book |

## Requirements

CrossPoint built with Dessen apps enabled (`${dessen_apps.build_flags}` and
`${dessen_apps.lib_deps}` in the board's env), and **Settings → Connectivity →
Plugins** turned on.

## Install

Copy a folder from [`dist/`](dist) to `/plugins/` on the SD card, then restart
the device so the plugin's event subscription is picked up. It shows up under
**Home → Plugins**.

## Build

```sh
git clone --recurse-submodules https://github.com/eszter007/dessen-matcha-plugins
cd dessen-matcha-plugins
make
```

Needs a RISC-V GCC; the one PlatformIO installs for ESP32-C3/S3 is used by
default (`RISCV=` to point elsewhere). Output goes to `dist/<plugin>/`.

## Notes for Dessen apps

- `DSN_BITMAP_MASK` draws **clear** bits (FreeInk's icon convention), not set
  bits as `dessen_abi.h` says.
- Apps get 64 KB and `dsn_fs_read` reads whole files only.
- Icons are from [Tabler Icons](https://tabler.io/icons) (MIT), as in matcha.

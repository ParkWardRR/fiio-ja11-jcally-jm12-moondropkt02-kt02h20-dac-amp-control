# 📦 Installing `ktctl`

> Everything here is **pre-hardware-validation** — the tool builds and runs, and
> all `--fake` functionality works, but real-device I/O has not been confirmed.
> See [`docs/PROTOCOL.md`](../docs/PROTOCOL.md) §7.

## Build from source

```bash
cd ktctl
cargo build --release
# binary at ktctl/target/release/ktctl
```

Requires a Rust toolchain (1.75+) and, for the default `usb` feature, libusb:

* **macOS**: `brew install libusb`
* **Debian/Ubuntu**: `sudo apt-get install libusb-1.0-0-dev pkg-config`
* **Fedora**: `sudo dnf install libusbx-devel pkgconf-pkg-config`

To build without any USB dependency (fake-device / protocol work only):

```bash
cargo build --release --no-default-features
```

## Linux device permissions

Without a udev rule you'll need `sudo` to claim the interface. Install the
bundled rule for non-root access:

```bash
sudo cp packaging/udev/99-ktctl.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
# replug the dongle
```

## Shell completions

```bash
ktctl completions bash > /etc/bash_completion.d/ktctl        # bash
ktctl completions zsh  > "${fpath[1]}/_ktctl"                # zsh
ktctl completions fish > ~/.config/fish/completions/ktctl.fish
```

## Optional config file

`ktctl` reads `~/.config/ktctl/config.toml` (or `$XDG_CONFIG_HOME/ktctl/`):

```toml
# Which master-gain (0x17) encoding to use, once hardware confirms it.
gain-encoding = "x2560-le"   # or "x10-be"
# Default to the in-memory fake device even without --fake.
default-fake = false
```

CLI flags (`--gain-encoding`, `--fake`) always override the file.

## Quick check (no hardware)

```bash
ktctl --fake peq get           # PEQ table + real response curve
ktctl --fake state             # Status screen
ktctl --fake                   # interactive TUI
ktctl list                     # enumerate connected devices
```

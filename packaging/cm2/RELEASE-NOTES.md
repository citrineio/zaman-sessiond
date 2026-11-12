Installable Zaman component packages for Debian 12 / ARM64, including a private Qt 6.10.2 + Quickshell 0.3 runtime.

Includes sessiond/CLI/input helpers, resident menu and notifications, shared helpers/transfer, Pegasus theme, and explicit Sway session startup. These packages do not replace Walnut's kernel, firmware or GPU drivers. Pegasus application, InputPlumber and emulators remain separately installed dependencies. F12 provides keyboard menu access during bring-up.

This is a prerelease for CM2 integration. Build checks run in Debian 12 ARM64; physical GPU, audio, controller and emulator acceptance on the CM2 is still required. A release built from a device-build branch uses that device's source snapshot; a main-branch release uses the published component checkpoints recorded inside each package.

On the CM2, download `install-zaman.py` and run `python3 install-zaman.py`. It verifies release SHA256 checksums and installs the matched package set. Run `zaman-start` from a local text console to test. Kiosk autologin is not enabled automatically.

To publish current local Q6A source, run `publish-device.py` as the existing Zaman login user with authenticated GitHub CLI. It pushes a separate device-build branch and waits for GitHub to publish the resulting packages. It does not install anything or restart the Q6A's services.

# Surge Engine catalog addition

User-confirmed Flathub ref on 15 October 2026:
`app/org.opensurge2d.OpenSurge/aarch64/stable`, version 0.6.1.3-1, commit
`9ac92e7e6d886f85acc6c8c1634ec524055a3cea8676fd67eaf2784591dc654a`.
The catalog tracks the stable branch rather than pinning this observation.

The paired Dukkan patch adds the seed and transaction authorization. This patch
adds daemon authorization, managed-helper authorization and Pegasus metadata.
The existing metadata/gallery/icon fallback behavior applies automatically.
No controller-profile change or game-specific setting is applied. Package
availability is confirmed; actual Play/Guide/Exit and controller acceptance on
the Q6A remain to be tested. The shared-catalog refactor remains separate.

Build the paired Dukkan change with its existing CMake workflow. Here, run:

```sh
cargo test --locked --offline
cargo build --release --locked --offline --bin zaman-sessiond
```

Close managed games, storefronts and Transfer normally. Then, as the kiosk user:

```sh
bash tools/activate-surge-catalog.sh
zamanctl storefront launch dukkan
```

The activation script backs up the two installed Python helpers and any prior
`99-sprint35-surge.conf`. It installs the matching helpers, points only the user
sessiond service at this checkout's rebuilt daemon, and restarts that daemon.
Existing unit environment and other drop-ins remain. InputPlumber, Quickshell,
compositor settings and the legacy installer are not modified. Keep this checkout
in place while its daemon path is configured. An activation failure restores
the previous helpers/drop-in and attempts to restart the previous daemon.

The source review confirmed five-list agreement; 27 Dukkan source tests, five
transaction tests, ten Pegasus sync tests and twenty managed-helper tests pass
locally. The activation script passed isolated success and rollback simulations.
Rust/Qt compilation and real service activation require the device toolchain.

The transaction-lock test now mocks its unrelated launcher start-time read,
matching the other lifecycle fixtures. This keeps lock validation independent
of the authoring runtime's PID/procfs namespace mapping.

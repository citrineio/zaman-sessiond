# Zaman background Transfer and Artwork & Game Info

Accepted Sprint 7 source snapshot, 19 October 2026. See
[`../../docs/sprint7-closeout.md`](../../docs/sprint7-closeout.md) for acceptance
evidence, limitations and hardening scope.

## Source boundaries

- `receiver/`: LocalSend-compatible receiver, durable inbox/import implementation,
  supervised Skyscraper adapter and the menu's local client.
- `tests/`: receiver, import, artwork and deployment tests; optional large-file probe.
- `ui-tests/`: Qt render tests at 1x and 2x scaling.
- `menu/`, `menu-tests/`: captured integration pages and tests. The complete QML
  application and its other components live in `citrineio/zaman-menu`.
- `pegasus/`: ROM refresh integration. The complete theme lives in
  `citrineio/zaman-pegasus`.

The publication script verifies that captured page/test copies agree with the
menu repository, that menu QML agrees with its live counterpart, that receiver
source agrees with the active release, and that the theme agrees with the live
theme. It records only a source inventory; it never publishes the local activation
journal, receiver identity, database, game content or account configuration.

## Tests

From the sessiond repository:

```bash
python3 -B -m unittest discover -s tools/transfer/tests -p 'test_*.py'
```

Run JavaScript tests in the complete menu checkout, not in the captured subset:

```bash
node /path/to/zaman-menu/tests/test-transfer.cjs
node /path/to/zaman-menu/tests/test-scrape.cjs
node /path/to/zaman-menu/tests/test-entry-animation.cjs
```

For a new test build directory, point Qt tests at that same complete menu:

```bash
cmake -S tools/transfer/ui-tests -B /path/to/transfer-ui-build \
  -DMENU_QML_ROOT=/path/to/zaman-menu/qml
cmake --build /path/to/transfer-ui-build --target transfer-ui-test -j2
ctest --test-dir /path/to/transfer-ui-build --output-on-failure
```

Reuse the existing build directory on the accepted device. Publication itself
requires neither compilation nor activation.

## Deployment status

`deploy.py`, `baselines.json` and `extra-baselines.json` are the guarded original
candidate deployment mechanism. They retain historical device-baseline checks;
they are not a general-purpose installer for arbitrary systems or a replacement
for the accepted update chain. In particular the later icon and close-animation
update has additional ownership records outside the base deployment.

Existing installations must retain their owner-run update packages and rollback
records. Do not rerun initial deployment over an active installation. A consolidated
installer/updater is deferred to separate hardening work.

Runtime requires the established system registry, sessiond/menu contracts and
Skyscraper 3.21.0. No Python pip packages are required. No settings, credentials,
registry data, controller mappings or personal game library are bundled here.

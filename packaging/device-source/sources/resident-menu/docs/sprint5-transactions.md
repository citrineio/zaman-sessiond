# Sprint 5 — Dukkan activity, notifications and branding

The menu integrates with the independent Dukkan transaction worker. It displays
queued/running/waiting/cancelling jobs live, with current-step progress when
available. Percentage updates are not written to notification history. The
Notifications page shows its empty message when both visible history and live jobs
are empty; it does not retain a blank activity card or inactive action hints.

Terminal result delivery uses ordered `transaction:DATABASE_UUID:SEQUENCE` event
keys. The notification server persists the record and per-database receipt
watermark before sending the matching `acceptedEvent` acknowledgement. Retrying an
already accepted event does not duplicate it after server restart, history clear
or dismissal. Storage integrity is part of this guarantee. Successful installs
use muted green; existing DND and dismissal semantics remain.

Targets are strictly typed `dukkan-transaction:UUID_HEX`. Selecting one follows the
existing menu input-lease release and managed storefront launch, with worker-side
job/catalog validation. Running gameplay is protected; return to the library first.
History dismissal/clear never cancels a transaction. Dukkan separately remembers
handled navigation nonces so ordinary new launches return to Discover.

## Branding

Dukkan's menu mark uses the exact owner-supplied Arabic sign PNG, preserving color,
transparency and landscape aspect. Steam uses unchanged official emblem geometry,
normalized to absolute Bézier commands and drawn through Qt Quick Canvas at the
current size with oversampling. This avoids an optional SVG image plugin. Both
store rows have aligned labels and their existing focus states. Resume uses the
active store's mark; Home's Storefronts category keeps one generic store icon.
See [asset attribution](../assets/storefronts/ATTRIBUTION.md).

Storefront components load only where used. The earlier simplified/raster artwork
is retained as historical source material, not used for the active menu marks.

## Validation and builds

The owner accepted the final icon and empty-state corrections on the device.
Fourteen actual-source JavaScript suites cover navigation/lifecycle contracts.
Python checks cover notification history, durable receipts and the private D-Bus
protocol. The Qt presentation test loads real QML, checks empty/repopulate states,
stable progress, actual text and icon pixels at two sizes/focus colors, and runs
at 1× and 2× display scale. Source tests do not replace device acceptance.

For subsequent menu test builds, retain the existing build directory/cache:

```sh
cmake -S tests/ui -B build-branding-test -DCMAKE_BUILD_TYPE=Release
cmake --build build-branding-test -j2
ctest --test-dir build-branding-test --output-on-failure --no-tests=error
```

QML/JS changes are loaded from source by this test; they do not require recompiling
its C++ executable. C++/CMake test changes do. Publishing the tested source does
not run a build or restart a service. The menu reuses accepted sessiond binaries;
this sprint introduces no Rust/helper rebuild or dependency installation.

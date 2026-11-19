# Contributing

Keep Cargo.lock committed and update enzUI and the foundation pins together. Shared widget and platform issues belong in the framework.

```sh
cargo fmt --all -- --check
ICED_TEST_BACKEND=software cargo test --locked --all-features -- --test-threads=1
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Keep tests focused on playback, launch/drop routing, queues, persistence,
provider contracts and account isolation. Consolidate related cases and avoid
checks that repeat setters or fixed presentation strings. Use the synthetic
fixture for transport checks and private libraries for native service review.

Never put tokens, passwords, real credentials or authenticated stream URLs into
fixtures, logs or repository files. Keep network requests off the UI thread,
bound response and artwork memory, and reject completions from retired requests,
accounts or profiles.

Translations live in `assets/locales/`. Only curated README screenshots belong
in `docs/screenshots/`; temporary review data and personal build policies stay
outside tracked files. Check small-window controls, light/dark themes, keyboard
input, native file opening and affected provider flows on the actual binary.

Contributions are under MIT;
preserve applicable dependency and media licenses when distributing binaries.

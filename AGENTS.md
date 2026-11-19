# Video contributor notes

Read README.md and relevant format/service documentation. Keep application logic
in Rust and reuse enzUI controls, theme tokens and native services. Keep SDK,
web-engine and foundation revisions pinned together; do not edit cached sources.

Keep playback, network requests, decoding and file work off the UI thread. Bound
workers, responses, artwork and queued work. Reject stale completions when the
active media, server, account or profile changes. Keep credentials in the keyring;
never write tokens, passwords or authenticated stream URLs to preferences or logs.

Run formatting, behavior tests and strict Clippy for code changes. Keep tests for
transport, queues, resume, persistence, provider isolation and credential safety;
consolidate overlapping cases rather than testing each setter or current layout.
Use disposable media and isolated XDG paths for native checks. Check both color
modes, a small player window, native file opening and the affected service flows.

Translation catalogs live in assets/locales/. Only curated README screenshots
belong in docs/screenshots/. Keep temporary scripts, captures and review evidence
under ignored AGENTS/. Personal build limits belong outside tracked files. Reuse
compatible Cargo targets without copying artifacts or editing fingerprints.
Stage explicit paths; never commit user state, tokens or private media libraries.

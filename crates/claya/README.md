# claya

Name placeholder.

[Claya](https://claya.app) is a macOS app that rewrites its own interface from a
chat prompt. It ships as a signed `.dmg`, not as a Rust library, so there is
nothing here to depend on yet — this crate exists so the name stays with the
project.

If a reusable piece is extracted later, it will land here. The most likely
candidate is the versioned module store: an append-only history where every edit
is an `INSERT`, rollback is a pointer move, and each version boundary snapshots
application state.

- Site — <https://claya.app>
- Docs — <https://claya.dev>
- Source — <https://github.com/jaideep2/claya>

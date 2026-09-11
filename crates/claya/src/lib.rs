//! Name placeholder for [Claya](https://claya.app).
//!
//! Claya is a macOS app that rewrites its own interface from a chat prompt. It is
//! distributed as a signed `.dmg` rather than as a Rust library, so there is no
//! API here yet — this crate reserves the name for the project.
//!
//! If a reusable component is extracted later it will be published here. The
//! likeliest candidate is the versioned module store: an append-only history in
//! which every edit is an `INSERT`, rollback is a pointer move, and every version
//! boundary snapshots application state.
//!
//! - Site: <https://claya.app>
//! - Docs: <https://claya.dev>
//! - Source: <https://github.com/jaideep2/claya>

/// Where the application itself lives. This crate is not the application.
pub const HOMEPAGE: &str = "https://claya.app";

#[cfg(test)]
mod tests {
    #[test]
    fn homepage_is_set() {
        assert!(super::HOMEPAGE.starts_with("https://"));
    }
}

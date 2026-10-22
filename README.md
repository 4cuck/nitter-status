# Nitter Status

Live status for the teapawt frontends: [status.d420.net](https://status.d420.net) (also [d420.net](https://d420.net)).

[Screenshot](screenshot.png)

The public list is nitter.cf, xitter.cf, teapaw.org, teapawt.net, and teapawt.org. Scraping is allowed on those instances. Ask the operator before scraping any other Nitter instance.

## Run

- Install [Rust](https://www.rust-lang.org/)
- Copy `.env.example` to `.env` and adjust
- Execute `cargo run --release`

Keep `.env`, `.admin-keys`, `sqlite.db`, and `sessions.db` off the public repo. `ADMIN_KEYS` in `.env.example` is a placeholder hash, not a real login key.

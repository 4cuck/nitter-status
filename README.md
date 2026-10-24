# Nitter Status

Live status for the teapawt frontends: [status.d420.net](https://status.d420.net) (also [d420.net](https://d420.net)).

[Screenshot](screenshot.png)

The public list is nitter.cf, nitter.gq, xitter.cf, xitter.gq, teapaw.org, teapawt.net, teapawt.org, and goyimx.com. Scraping is allowed on nitter.cf, nitter.gq, xitter.cf, xitter.gq, teapaw.org, teapawt.net, and teapawt.org. Ask the operator before scraping any other Nitter instance.

## Run

- Install [Rust](https://www.rust-lang.org/)
- Copy `.env.example` to `.env` and adjust
- Execute `cargo run --release`

Keep `.env`, `.admin-keys`, `sqlite.db`, and `sessions.db` off the public repo. `ADMIN_KEYS` in `.env.example` is a placeholder hash, not a real login key.

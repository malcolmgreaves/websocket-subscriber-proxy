# websocket-subscriber-proxy

A websocket proxy server. Provides a many-to-one style subscription: many clients proxied to a single producer.

## Demo

Make sure to install [`websocat`](https://github.com/vi/websocat) with `cargo install websocat`.

1. Start websocket proxy server: `cargo run --release`
2. Register a publisher: `websocat ws://0.0.0.0:8080/publish/1d103620-2534-4ba8-8836-fe397b46ee18`
3. Regsiter one or more subscribers: `websocat ws://0.0.0.0:8080/subscribe/1d103620-2534-4ba8-8836-fe397b46ee18`


## Development

**BEFORE DEVELOPING, INSTALL THE `pre-commit` hooks: `pre-commit install`**.

Make sure you have [`pre-commit`](https://github.com/pre-commit/pre-commit) installed. If you have [`uv`](https://docs.astral.sh/uv/) you can do `uv tool install pre-commit`.

Make sure you have `cargo` installed. If you don't, then [install `rustup` to manage your Rust toolchain](https://www.rust-lang.org/tools/install).

- `cargo fmt`: Format code.
- `cargo check && cargo clippy`: Typecheck and lint code.
- `cargo build`: Compile.
- `cargo run`: Start the websocket proxy server.

Use `--release` for production.

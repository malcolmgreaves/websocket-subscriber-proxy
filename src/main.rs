mod errors;
mod proxy_websocket;
mod server;
mod types;
mod utils;

use tokio;
use tower_http::cors::CorsLayer;

use crate::{
    server::WebsocketProxyServer,
    utils::{bool_env_var, unsigned_env_var},
};

const ENABLE_BIDIRECTIONAL_ENV_NAME: &str = "ENABLE_BIDIRECTIONAL";
const MAX_CHANNEL_SIZE_ENV_NAME: &str = "MAX_CHANNEL_SIZE";
const PORT_ENV_NAME: &str = "PORT";

#[tokio::main]
async fn main() {
    //
    // handle configuration
    //
    let allow_subscriber_to_forward_to_publisher = bool_env_var(ENABLE_BIDIRECTIONAL_ENV_NAME);
    eprintln!(
        "Allow subscriber to forward messages **to** the publisher? ({}): {}",
        ENABLE_BIDIRECTIONAL_ENV_NAME, allow_subscriber_to_forward_to_publisher
    );

    let max_channel_size = unsigned_env_var(MAX_CHANNEL_SIZE_ENV_NAME).unwrap_or(10_000);
    eprintln!(
        "Maximum channel size for buffering websocket messages ({}):          {}",
        MAX_CHANNEL_SIZE_ENV_NAME, max_channel_size
    );

    let port: u16 = unsigned_env_var(PORT_ENV_NAME)
        .map(|p| {
            p.try_into()
                .map_err(|e| format!("Invalid port value, must be a positive integer in 2^16: {}", e))
                .unwrap()
        })
        .unwrap_or(8080_u16);
    eprintln!(
        "Server port ({}):                                                                {}",
        PORT_ENV_NAME, port
    );

    //
    // construct websocket proxy server
    //
    let app = WebsocketProxyServer::router()
        .layer(CorsLayer::permissive())
        .with_state(WebsocketProxyServer::new(
            max_channel_size,
            allow_subscriber_to_forward_to_publisher,
        ));

    //
    // serve
    //
    let hostname = format!("0.0.0.0:{port}");
    eprintln!("----------------------------------------------------------------------------------------");
    eprintln!("Server starting on {hostname}");
    let listener = tokio::net::TcpListener::bind(hostname).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

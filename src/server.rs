use axum::{Router, extract::ws::Message, routing::get};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

use crate::{
    proxy_websocket::{register_publisher, register_subscriber},
    types::{DashCounter, Id},
};

/// A publisher's websocket connection.
///
/// Allows many subscribers to connect and receive messages from the publisher.
/// Additionally, allows many subscribers to send messages to the publisher.
#[derive(Debug)]
pub struct Connection {
    pub(crate) receive_from: broadcast::Receiver<Message>,
    pub(crate) send_to: mpsc::UnboundedSender<Message>,
}

impl Clone for Connection {
    /// The broadcast receiver channel is resubscribed when a Connection is cloned.
    fn clone(&self) -> Self {
        Self {
            receive_from: self.receive_from.resubscribe(),
            send_to: self.send_to.clone(),
        }
    }
}

/// The Egress Service
#[derive(Debug, Clone)]
pub struct WebsocketProxyServer {
    pub(crate) publisher_connections: Arc<DashMap<Id, Connection>>,
    pub(crate) subscriber_connections: Arc<DashCounter<Id>>,
    pub(crate) max_channel_size: usize,
    pub(crate) allow_subscriber_to_forward_to_publisher: bool,
}

impl WebsocketProxyServer {
    /// Creates a new websocket proxy server.
    ///
    /// The `max_channel_size` parameter determines the maximum number of messages that can be buffered per connection.
    /// Messages are buffered from the publisher. The first subscriber per `Id` will receive all buffered messages.
    /// Once sent, messages from the publisher are removed from the buffer.
    ///
    /// The `allow_subscriber_to_forward_to_publisher` parameter determines whether subscribers can forward messages to publishers.
    /// If true, then any subsriber can send a message to the publisher. If false, then the proxy server discards all messages
    /// that the subscriber sends along its websocket.
    pub fn new(max_channel_size: usize, allow_subscriber_to_forward_to_publisher: bool) -> Self {
        WebsocketProxyServer {
            publisher_connections: Arc::new(DashMap::new()),
            subscriber_connections: Arc::new(DashCounter::new()),
            max_channel_size,
            allow_subscriber_to_forward_to_publisher,
        }
    }

    /// The axum router for the websocket proxy server.
    ///
    /// Has two routes -- `/publish` and `/subscribe`:
    ///
    /// - `/publish/{id}`: Registers a new publisher connection for the id.
    ///                    There can only be one publisher per id.
    ///                    Any one that subscribes to the id will be connected to this publisher.
    ///
    /// - `/subscribe/{id}`: Connects a subscriber to a publisher. A publisher must already be
    ///                      registered before a subscriber can connect. Once connected, the subscriber
    ///                      will be able to interact with the publisher.
    pub fn router() -> Router<Self> {
        Router::new()
            .route("/publish/{id}", get(register_publisher))
            .route("/subscribe/{id}", get(register_subscriber))
    }
}

impl Default for WebsocketProxyServer {
    /// Defaults to buffering 10k messages per connection and disabling subscriber forwarding.
    fn default() -> Self {
        Self::new(10_000, false)
    }
}

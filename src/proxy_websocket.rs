use axum::{
    body::Body,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::Response,
};
use dashmap::Entry;
use futures_util::{SinkExt, StreamExt};

use std::sync::Arc;
use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc, oneshot,
    },
    task::JoinHandle,
};

use futures::FutureExt;

use crate::{
    errors::{Error, Result},
    server::{Connection, WebsocketProxyServer},
    types::DashCounter,
    types::Id,
};

// // // // // // // // // // // // // // // // // // // // // // // // // // //
//                                                                            //
//                     Publisher <> WS Proxy Server                           //
//                                                                            //
// // // // // // // // // // // // // // // // // // // // // // // // // // //

/// Registers a publisher connection with the proxy server.
///
/// This function establishes a connection between the publisher and the proxy server,
/// allowing the publisher to send messages to subscribers and receive responses from them.
///
/// Messages sent from the publisher are broadcasted to all connected subscribers.
pub async fn register_publisher(
    ws: WebSocketUpgrade,
    State(proxy_server): State<WebsocketProxyServer>,
    Path(id): Path<Id>,
) -> Result<Response<Body>> {
    eprintln!("[publish] Received request to initiate publisher connection for ID: {id}");

    let (send_to_publisher_rx, receive_from_subscriber_tx) = match proxy_server.publisher_connections.entry(id.clone())
    {
        Entry::Occupied(_) => return Err(Error::IdAlreadyRegistered(id)),
        Entry::Vacant(entry) => {
            // channel used by *other endpoints* to send messages to the publisher
            let (send_to_publisher_tx, send_to_publisher_rx) = mpsc::unbounded_channel::<Message>();

            // channel used by *other endpoints* to receive responses from the publisher
            let (receive_from_subscriber_tx, receive_from_subscriber_rx) =
                broadcast::channel::<Message>(proxy_server.max_channel_size);

            // we're now maintaining the publisher connection
            entry.insert(Connection {
                receive_from: receive_from_subscriber_rx,
                send_to: send_to_publisher_tx.clone(),
            });

            (send_to_publisher_rx, receive_from_subscriber_tx)
        }
    };

    let publisher_connections = proxy_server.publisher_connections.clone();

    Ok(ws.on_upgrade(move |socket| {
        handle_publisher_websocket(
            socket,
            id.clone(),
            proxy_server.subscriber_connections.clone(),
            send_to_publisher_rx,
            receive_from_subscriber_tx,
            proxy_server.allow_subscriber_to_forward_to_publisher,
        )
        .then(async move |_| {
            // this only terminates when the publisher connection is closed
            // therefore, we drop the publisher connection from the proxy_server server's state
            eprintln!("[publish] Ensuring websocket connection is removed for ID: {id}");
            publisher_connections.remove(&id);
        })
    }))
}

/// Handles incoming WebSocket connections for a publisher.
///
/// For each message sent by the publisher (websocket), it will be sent to all subscribers
/// (broadcast channel). If there are no subscribers, the message will be buffered. Messages
/// over the broadcast channel's capacity will be dropped in least-recently-added order.
///
/// If `allow_subscriber_to_forward_to_publisher` is true, then each message received from
/// any subscriber (mpsc channel), it will be sent to the publisher (websocket). However, if
/// this value is false, then any message sent from a subscriber will be ignored.
pub async fn handle_publisher_websocket(
    socket: WebSocket,
    id: Id,
    subscribers: Arc<DashCounter<Id>>,
    mut send_to_publisher_rx: mpsc::UnboundedReceiver<Message>,
    receive_from_subscriber_tx: broadcast::Sender<Message>,
    allow_subscriber_to_forward_to_publisher: bool,
) {
    eprintln!("[publish] Upgrading to websocket connection for ID: {}", id);

    // channel for the publisher connection
    let (mut ws_tx, mut ws_rx) = socket.split();

    let (signal_publisher_hangup_tx, mut signal_publisher_hangup_rx) = oneshot::channel::<()>();

    // Task: forward anything sent on `send_to_publisher_rx` into the websocket (`ws_tx`).
    let forward_to_publisher_task: JoinHandle<()> = tokio::spawn({
        let id = id.clone();

        async move {
            if !allow_subscriber_to_forward_to_publisher {
                eprintln!("[publish] IGNORING messages from subscriber to publisher with ID {id}");
                return;
            } else {
                loop {
                    tokio::select! {
                        _ = &mut signal_publisher_hangup_rx => {
                            eprintln!("[publish] Received signal to hangup from publisher with ID {id}");
                            break;
                        },
                        from_subscriber = send_to_publisher_rx.recv() => {
                            match from_subscriber {
                                Some(message) => {
                                    match ws_tx.send(message).await {
                                        Ok(()) => (),
                                        Err(error) => {
                                            // websocket is closed!
                                            eprintln!("[publish] Websocket closed for publisher with ID: {id} -- ending connection: {error}");
                                            break;
                                        }
                                    }
                                },
                                None => {
                                    eprintln!("[publish] failed to receive message from subscriber! Ignoring...");
                                },
                            }
                        },
                    };
                }
            }
            eprintln!("[publish] ended forward_to_publisher_task for ID: {id}");
        }
    });

    // Task: forward anything sent from the publisher (`ws_rx`) to the subscriber (`receive_from_subscriber_tx`)
    let forward_from_publisher_task: JoinHandle<()> = tokio::spawn({
        let id = id.clone();
        async move {
            loop {
                match ws_rx.next().await {
                    Some(Ok(message)) => {
                        if subscribers.contains(&id) {
                            match receive_from_subscriber_tx.send(message) {
                                Ok(_) => (),
                                Err(error) => {
                                    eprintln!(
                                        "[publish] Failed to send message to subscriber from publisher with ID: {id}: {error} -- ending connection: {error}",
                                    );
                                    break;
                                }
                            }
                        } else {
                            eprintln!(
                                "[publish] Dropping message from publisher ({id}) because there are no currently connected subscribers."
                            );
                        }
                    }
                    Some(Err(error)) => {
                        eprintln!(
                            "[publish] could not receive message from publisher with ID {id} due to error: {error}"
                        );
                        break;
                    }
                    None => {
                        eprintln!("[publish] publisher with ID {id} disconnected, unknown reason");
                        break;
                    }
                }
            }

            // When we break out of the loop, it is because the websocket connection to the publisher is closed.
            eprintln!("[publish] ended forward_from_publisher_task for ID: {id}");
            match signal_publisher_hangup_tx.send(()) {
                Ok(()) => (),
                Err(error) => {
                    eprintln!("[publish] Failed to signal hangup from publisher with ID {id}: {error:?}");
                }
            }
        }
    });

    // Wait for both tasks to finish.
    match tokio::join!(forward_to_publisher_task, forward_from_publisher_task) {
        (Ok(()), Ok(())) => {
            eprintln!("[publish] cleanly ended forward_to_publisher_task and forward_from_publisher_task for ID: {id}");
        }
        (Err(fwd_to_run_error), Err(fwd_from_run_error)) => {
            eprintln!(
                "[publish] error occurred while ending forward_to_publisher_task and forward_from_publisher_task for ID: {id}:\n\tforward_from_publisher: {fwd_from_run_error}\n\tforward_to_publisher: {fwd_to_run_error}"
            );
        }
        (Err(error), Ok(_)) => {
            eprintln!("[publish] error occurred while ending forward_to_publisher task: {error}");
        }
        (Ok(_), Err(error)) => {
            eprintln!("[publish] error occurred while ending forward_from_publisher task: {error}");
        }
    }

    eprintln!("[publish] finished handling publisher connection for ID: {id}");
}

// // // // // // // // // // // // // // // // // // // // // // // // // // //
//                                                                            //
//                      Subscriber <> WS Proxy Server                         //
//                                                                            //
// // // // // // // // // // // // // // // // // // // // // // // // // // //

/// Registers a new subscriber connection with the proxy server.
///
/// Returns an error if there are no publishers for the Id.
/// Once connected, this allows the subscriber to receive messages from the publisher.
/// Additionally, connected subscribers can send messages to the publisher.
pub async fn register_subscriber(
    ws: WebSocketUpgrade,
    State(proxy_server): State<WebsocketProxyServer>,
    Path(id): Path<Id>,
) -> Result<Response<Body>> {
    eprintln!("[subscribe] Received request to initiate proxied connection for ID: {id}");

    let connection = match proxy_server.publisher_connections.entry(id.clone()) {
        Entry::Vacant(_) => return Err(crate::errors::Error::IdNotFound(id)),
        Entry::Occupied(entry) => entry.get().clone(),
    };

    // make sure others know that we have at least one subscriber connected to this publisher
    proxy_server.subscriber_connections.increment(id.clone());

    Ok(ws.on_upgrade(move |socket| {
        handle_subscriber_websocket(
            socket,
            id.clone(),
            connection,
            proxy_server.allow_subscriber_to_forward_to_publisher,
        )
        .then(async move |_| {
            // at this point, the subscriber has disconnected
            // so we can let others know that we have one less subscriber connected
            eprintln!("[subscribe] ensuring we are no longer tracking a single subscriber for ID: {id}");
            proxy_server.subscriber_connections.decrement(id)
        })
    }))
}

/// Handles websocket requests from a subscriber to the publisher.
///
/// Any message from the publisher (broadcast channel) is sent to the subscriber (websocket).
/// If `allow_subscriber_to_forward_to_publisher` is true, then any message from the subscriber
/// (websocket) is sent to the publisher (mpsc channel).
///
/// Terminates when the subscriber (websocket) disconnects or when the publisher disconnected
/// (mpsc channel closure).
pub async fn handle_subscriber_websocket(
    socket: WebSocket,
    id: Id,
    publisher_connection: Connection,
    allow_subscriber_to_forward_to_publisher: bool,
) {
    eprintln!(
        "[subscribe] Handling subscriber websocket proxy to publisher for ID: {}",
        id
    );

    // channel for the subscriber connection
    let (mut ws_subscriber_tx, mut ws_subscriber_rx) = socket.split();

    let Connection {
        receive_from: mut receive_from_publisher,
        send_to: send_to_publisher,
    } = publisher_connection;

    let (signal_subscriber_hangup_tx, mut signal_subscriber_hangup_rx) = oneshot::channel::<()>();

    // Task: forward anything sent from the publisher to the subscriber's websocket
    let forward_to_subscriber_task: JoinHandle<()> = tokio::spawn({
        let id = id.clone();
        async move {
            loop {
                match receive_from_publisher.recv().await {
                    Ok(message) => match ws_subscriber_tx.send(message).await {
                        Ok(()) => (),
                        Err(error) => {
                            // websocket is closed!
                            eprintln!(
                                "[subscribe] Websocket closed for subscriber with ID: {id} -- ending connection: {error}"
                            );
                            break;
                        }
                    },
                    Err(RecvError::Lagged(n_behind)) => {
                        eprintln!(
                            "[subscribe] Warning: subscriber lagged by {n_behind} messages from publisher! We'll try to be faster next time :("
                        );
                    }
                    Err(RecvError::Closed) => {
                        eprintln!(
                            "[subscribe] publisher connection closed for ID: {id} -- disconnecting from subscriber"
                        );
                        break;
                    }
                }
            }
            // When we break out of the loop, it is because the websocket connection to the subscriber is closed
            // OR we can no longer comunicate with the publisher.
            eprintln!("[subscribe] ended forward_to_subscriber_task for ID: {id}");

            match signal_subscriber_hangup_tx.send(()) {
                Ok(()) => (),
                Err(error) => {
                    eprintln!("[subscribe] Failed to signal hangup from subscriber with ID {id}: {error:?}");
                }
            }
            // It's ok to drop our cloned reference of the connection -- it's still in the map for _another_ subscriber!
        }
    });

    // Task: forward anything sent from the subscriber's websocket to the publisher
    let forward_from_subscriber_task: JoinHandle<()> = tokio::spawn({
        let id = id.clone();
        async move {
            if !allow_subscriber_to_forward_to_publisher {
                eprintln!("[subscribe] IGNORING inbound messages from subscriber to publisher with ID {id}");
                return;
            } else {
                loop {
                    tokio::select! {
                        _ = &mut signal_subscriber_hangup_rx => {
                            eprintln!("[subscribe] subscriber hangup signal received for ID: {id}");
                            break;
                        },
                        subscriber_response = ws_subscriber_rx.next() => {
                            match subscriber_response {
                                Some(Ok(message)) => {
                                    match send_to_publisher.send(message) {
                                        Ok(()) => {}
                                        Err(error) => {
                                            eprintln!(
                                                "[subscribe] Failed to send message to publisher from subscriber with ID: {id}: {error} -- ending connection: {error}",
                                            );
                                            break;
                                        }
                                    }
                                }
                                Some(Err(error)) => {
                                    eprintln!(
                                        "[subscribe] Failed to receive message from subscriber with ID: {id}: {error} -- ending connection"
                                    );
                                    break;
                                }
                                None => {
                                    eprintln!("[subscribe] subscriber for ID {id} disconnected for an unknown reason");
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // When we break out of the loop, it is because the websocket connection to the subscriber is closed
            // OR we can no longer comunicate with the publisher.
            eprintln!("[subscribe] ended forward_from_subscriber_task for ID: {id}");
            // It's ok to drop our cloned reference of the connection -- it's still in the map for _another_ subscriber!
        }
    });

    // Wait for the forwarding tasks to complete.
    match tokio::join!(forward_to_subscriber_task, forward_from_subscriber_task) {
        (Ok(()), Ok(())) => {
            eprintln!(
                "[subscribe] cleanly ended forward_to_subscriber_task and forward_from_subscriber_task for ID: {id} -- re-inserting publisher's connection"
            );
        }
        (Err(fwd_to_subscriber_error), Err(fwd_from_subscriber_error)) => {
            eprintln!(
                "[subscribe] error occurred while ending forward_to_subscriber_task and forward_from_subscriber_task for ID: {id}:\n\tforward_from_subscriber: {fwd_from_subscriber_error}\n\tforward_to_subscriber: {fwd_to_subscriber_error}"
            );
        }
        (Err(error), Ok(_)) => {
            eprintln!(
                "[subscribe] error occurred while ending forward_to_subscriber task: {error} -- cannot restore publisher connection! Dropping and hope that the publisher reconnects."
            );
        }
        (Ok(_), Err(error)) => {
            eprintln!(
                "[subscribe] error occurred while ending forward_from_subscriber task: {error} -- cannot restore publisher connection! Dropping and hope that the publisher reconnects."
            );
        }
    }

    eprintln!("[subscribe] Finished handling websocket connection for subscriber for ID: {id}");
}

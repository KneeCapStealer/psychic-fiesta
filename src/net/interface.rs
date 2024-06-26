use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use chrono::Utc;
use futures::executor;
use tokio::sync::Mutex;

use crate::{
    game::{GameAction, PieceColor},
    net::{
        net_utils::{get_available_port, get_local_ip, hex_decode_ip, hex_encode_ip},
        p2p::{
            net_loop::{client_network_loop, host_network_loop},
            queue::{
                check_for_response, get_outgoing_queue_len, new_transaction_id,
                pop_incoming_gameaction, push_outgoing_queue,
            },
            P2pPacket, P2pRequest, P2pRequestPacket, P2pResponse, P2pResponsePacket,
        },
        status,
    },
};

/// Start the host network peer on a LAN connection.
/// Returns the join code for the client
pub fn start_lan_host() -> String {
    let port = executor::block_on(get_available_port()).unwrap();
    let socket = executor::block_on(tokio::net::UdpSocket::bind(("0.0.0.0", port))).unwrap();

    let local_ip = get_local_ip().unwrap();

    let encoded_ip = hex_encode_ip(SocketAddr::new(IpAddr::V4(local_ip), port)).unwrap();
    executor::block_on(status::set_join_code(&encoded_ip));

    executor::block_on(status::set_connection_status(
        status::ConnectionStatus::PendingConnection,
    ));

    host_network_loop(socket);

    encoded_ip
}

/// Start the client network peer on a LAN connection.
pub fn start_lan_client() {
    let port = executor::block_on(get_available_port()).unwrap();
    let socket = executor::block_on(tokio::net::UdpSocket::bind(("0.0.0.0", port))).unwrap();

    executor::block_on(status::set_connection_status(
        status::ConnectionStatus::PendingConnection,
    ));

    // Start client network loop, with 10 pings pr. second
    client_network_loop(socket, 1);
}

/// Sends a join request to the host.
/// This function should only be called by the client, and only after the client network loop has
/// started, via. `start_lan_client()`.
///
/// ## Params
/// * `join_code` - The join code sent by the host.
/// * `username` - The clients username.
pub fn send_join_request(join_code: &str, username: &str) -> u16 {
    let join_request = P2pRequest::new(
        status::CONNECT_SESSION_ID,
        executor::block_on(new_transaction_id()),
        P2pRequestPacket::Connect {
            join_code: join_code.to_owned(),
            username: username.to_owned(),
        },
    );
    let host_addr = hex_decode_ip(join_code).unwrap();
    println!("Asking to join Host at {:?}", host_addr);

    println!("Pushing to queue");

    executor::block_on(push_outgoing_queue(
        P2pPacket::Request(join_request.clone()),
        None,
    ))
}

/// Check if the connection request sent with `send_join_request()` has gotten an response.
/// If a packet has been recieved, and if that packet is a correct response, the function will
/// return the clients assigned piece color, as well as the hosts username.
///
/// ## Params
/// * `transaction_id` - The id of the join request
pub fn check_for_connection_resp(
    transaction_id: u16,
) -> Option<anyhow::Result<(PieceColor, String)>> {
    println!("Checking for resp");
    match executor::block_on(check_for_response(transaction_id)) {
        Some(resp) => match resp {
            P2pPacket::Response(resp) => match resp.packet {
                P2pResponsePacket::Connect {
                    client_color,
                    host_username,
                } => {
                    println!("Got resp");
                    executor::block_on(status::set_connection_status(
                        status::ConnectionStatus::connected(),
                    ));
                    println!("Set connection status");
                    executor::block_on(status::set_session_id(resp.session_id));
                    println!("Set session id");
                    executor::block_on(status::set_other_username(&host_username));
                    println!("Set username");
                    Some(Ok((client_color, host_username)))
                }
                P2pResponsePacket::Error { kind } => {
                    Some(Err(anyhow!("Got Error response: {:?}", kind)))
                }
                _ => Some(Err(anyhow!("Got wrong response Packet"))),
            },
            _ => Some(Err(anyhow!("Got request packet instead of response"))),
        },
        None => {
            println!("Got no resp :(");
            None
        }
    }
}

/// A blocking function which sends a join request to the host, and waits for a response. The
/// function is in a loop, so if a packet goes lost, it will send a new one after 5 seconds.
///
/// ## Params
/// * `join_code` - The join code sent by the host.
/// * `username` - The clients username.
pub fn connect_to_host_loop(
    join_code: &str,
    username: &str,
) -> anyhow::Result<(PieceColor, String)> {
    executor::block_on(status::set_join_code(join_code));
    let host_addr = hex_decode_ip(join_code).unwrap();
    executor::block_on(status::set_other_addr(host_addr));
    set_my_username(username);
    println!("Starting to connect...");
    let mut connection_tick = tokio::time::interval(Duration::from_millis(500));
    loop {
        let join_id = send_join_request(join_code, username);

        let time = Utc::now();
        println!("Request sent at {:?}", time.to_string());
        print!(
            "Queue len: {}",
            executor::block_on(get_outgoing_queue_len())
        );
        println!("!!!");

        for _ in 0..10 {
            executor::block_on(connection_tick.tick());
            if let Some(resp) = check_for_connection_resp(join_id) {
                return resp;
            }
        }
    }
}

/// Get the next game action from the other user.
pub fn get_next_game_action() -> Option<GameAction> {
    executor::block_on(pop_incoming_gameaction())
}

/// Send a game action to the other user.
/// The function is not blocking the thread until it gets a response.
///
/// ## Params:
/// * `action` - The game action you want to send, is of type `GameAction`
/// * `on_response` - The closure that will be called when the `GameAction` request gets a
/// response.
///
/// ## Examples:
/// ```
/// let action = GameAction::Surrender;
///
/// let callback = |res: anyhow::Result<()>| {
///     match res {
///         Ok(_) => println!("Hell yea!!"),
///         Err(_) => println!("Hell no!!"),
///     };
/// }
///
/// send_game_action(action, callback);
/// ```
pub fn send_game_action<F>(action: GameAction, mut on_response: F)
where
    F: FnMut(anyhow::Result<()>) + Send + Sync + 'static,
{
    let closure = Arc::new(Mutex::new(move |resp: P2pResponse| {
        if let P2pResponsePacket::Error { kind: _ } = resp.packet {
            on_response(Err(anyhow::anyhow!("Recieved error")));
        } else {
            on_response(Ok(()));
        }
    }));

    let request = P2pRequest {
        session_id: executor::block_on(status::get_session_id()),
        transaction_id: executor::block_on(new_transaction_id()),
        packet: P2pRequestPacket::game_action(action),
    };
    executor::block_on(push_outgoing_queue(
        P2pPacket::Request(request),
        Some(closure),
    ));
}

/// Check if there is an established connection between the host and client.
pub fn is_connected() -> bool {
    executor::block_on(status::get_connection_status()).is_connected()
}

/// Gets the other users username.
pub fn get_other_username() -> Option<String> {
    executor::block_on(status::get_other_username())
}

/// Sets your username.
pub fn set_my_username(name: &str) {
    executor::block_on(status::set_my_username(name))
}

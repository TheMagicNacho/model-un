pub mod connection_pool;
pub mod counter;
pub mod game;
pub mod interface;
pub mod structs;

use std::collections::HashMap;
use std::sync::Arc;

use game::Game;
use structs::{GameState, RoomUpdate};
use tokio::sync::{RwLock, broadcast};
use warp::Filter;

use crate::connection_pool::ConnectionPool;
use crate::interface::GameWebSocket;

pub type SharedGameState = Arc<RwLock<HashMap<String, GameState>>>;

/// Build the WebSocket route used by both the binary and
/// integration tests.
pub fn build_ws_route() -> (
    impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone,
    broadcast::Sender<RoomUpdate>,
) {
    let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
    let tx_filter = warp::any().map({
        let tx = tx.clone();
        move || tx.clone()
    });

    let pool_filter = warp::any().map(ConnectionPool::new);

    let ws_route = warp::path("ws")
        .and(warp::path::param::<String>())
        .and(warp::ws())
        .and(tx_filter)
        .and(pool_filter)
        .and_then(GameWebSocket::handle_connection);

    (ws_route, tx)
}

/// Build all routes (index redirect, static files, ws).
pub fn build_routes() -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone
{
    let game_state = Game::instance();

    let index_route = warp::path::end().and_then(async move || {
        let room_name = game_state.random_name_generator().await;
        Ok::<_, warp::Rejection>(warp::redirect(
            warp::http::Uri::from_maybe_shared(format!("/index.html?room={room_name}")).unwrap(),
        ))
    });

    let (ws_route, _tx) = build_ws_route();

    let img_route = warp::path("img").and(
        warp::path("portraits.png")
            .and(warp::fs::file("./client/img/portraits.png"))
            .or(warp::path("atlas.png").and(warp::fs::file("./client/img/atlas.png"))),
    );

    let client_code = warp::path("game.js").and(warp::fs::file("./client/game.js"));

    let client_style = warp::path("style.css").and(warp::fs::file("./client/style.css"));

    let client_html = warp::path("index.html").and(warp::fs::file("./client/index.html"));

    warp::get().and(
        index_route
            .or(ws_route)
            .or(img_route)
            .or(client_code)
            .or(client_style)
            .or(client_html),
    )
}

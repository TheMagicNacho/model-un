#![feature(async_closure)]
mod connection_pool;
mod counter;
mod game;
mod interface;
mod structs;

use std::collections::HashMap;
use std::sync::Arc;

use game::Game;
use log::info;
use structs::{GameState, RoomUpdate};
use tokio::sync::{RwLock, broadcast};
use warp::Filter;

use crate::connection_pool::ConnectionPool;
use crate::interface::GameWebSocket;

type SharedGameState = Arc<RwLock<HashMap<String, GameState>>>;

static PORT: u16 = 3000;
static BIND_ADDRESS: [u8; 4] = [0, 0, 0, 0];

#[tokio::main]
async fn main()
{
  env_logger::init();
  let game_state = Game::instance();

  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let tx_filter = warp::any().map(move || tx.clone());

  // if the user goes to the root, generate a room name and redirect them to
  // index.html with the parameter of the room name
  let index_route = warp::path::end().and_then(async move || {
    // let room_name = generate_room_name();
    // let room_name = game_state.generate_new_room(None).await;
    let room_name = game_state.random_name_generator().await;
    Ok::<_, warp::Rejection>(warp::redirect(
      warp::http::Uri::from_maybe_shared(format!(
        "/index.html?room={}",
        room_name
      ))
      .unwrap(),
    ))
  });

  // the client is going to send a parameter after the /ws/ route. That
  // parameter is the room name. We need to filter out the room nae and group
  // all connections with the same room name together.
  let pool_filter = warp::any().map(ConnectionPool::new);

  let ws_route = warp::path("ws")
    .and(warp::path::param::<String>())
    .and(warp::ws())
    .and(tx_filter.clone())
    .and(pool_filter)
    .and_then(GameWebSocket::handle_connection);

  let img_route = warp::path("img").and(
    warp::path("portraits.png")
      .and(warp::fs::file("./client/img/portraits.png"))
      .or(
        warp::path("atlas.png").and(warp::fs::file("./client/img/atlas.png")),
      ),
  );

  let client_code =
    warp::path("game.js").and(warp::fs::file("./client/game.js"));

  let client_style =
    warp::path("style.css").and(warp::fs::file("./client/style.css"));

  let client_html =
    warp::path("index.html").and(warp::fs::file("./client/index.html"));

  let routes = warp::get().and(
    index_route
      .or(ws_route)
      .or(img_route)
      .or(client_code)
      .or(client_style)
      .or(client_html),
  );
  // .with(warp::cors().allow_any_origin());

  info!("Model UN Server Running.");
  warp::serve(routes).run((BIND_ADDRESS, PORT)).await;
}

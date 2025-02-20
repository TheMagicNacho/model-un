mod game;
mod interface;
mod structs;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use game::Game;
use log::info;
use structs::{GameState, RoomUpdate};
use tokio::sync::broadcast;
use warp::Filter;

use crate::interface::GameWebSocket;

type SharedGameState = Arc<Mutex<HashMap<String, GameState>>>;

static PORT: u16 = 3000;
static BIND_ADDRESS: [u8; 4] = [0, 0, 0, 0];

#[tokio::main]
async fn main()
{
  env_logger::init();
  let game_state = Game::instance();

  // let game_state = Arc::new(Mutex::new(HashMap::new()));

  let (tx, _rx) = broadcast::channel::<RoomUpdate>(32);
  // let game_state_filter = warp::any().map(move || game_state);
  // let game_state_filter = warp::any().map(move || game.get_global_state());
  let tx_filter = warp::any().map(move || tx.clone());

  // if the user goes to the root, generate a room name and redirect them to
  // index.html with the parameter of the room name
  let index_route = warp::path::end().map(move || {
    // let room_name = generate_room_name();
    let room_name = game_state.generate_new_room(None);
    warp::redirect(
      warp::http::Uri::from_maybe_shared(format!(
        "/index.html?room={}",
        room_name
      ))
      .unwrap(),
    )
  });
  // the client is going to send a parameter after the /ws/ route. That
  // parameter is the room name. We need to filter out the room nae and group
  // all connections with the same room name together.
  let ws_route = warp::path("ws")
    .and(warp::path::param::<String>())
    .and(warp::ws())
    .and(tx_filter.clone())
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

  let routes = index_route
    .or(ws_route)
    .or(img_route)
    .or(client_code)
    .or(client_style)
    .or(client_html)
    .with(warp::cors().allow_any_origin());

  info!("Model UN Server Running.");
  warp::serve(routes).run((BIND_ADDRESS, PORT)).await;
}

use log::info;
use model_un::build_routes;

static PORT: u16 = 3000;
static BIND_ADDRESS: [u8; 4] = [0, 0, 0, 0];

#[tokio::main]
async fn main() {
    env_logger::init();

    let routes = build_routes();

    info!("Model UN Server Running.");
    warp::serve(routes).run((BIND_ADDRESS, PORT)).await;
}

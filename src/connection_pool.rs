use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use warp::ws::Message;

pub struct ConnectionPool
{
  connections: RwLock<HashMap<String, Vec<mpsc::Sender<Message>>>>,
}

impl ConnectionPool
{
  pub fn new() -> Arc<Self>
  {
    Arc::new(Self {
      connections: RwLock::new(HashMap::new()),
    })
  }

  pub async fn add(
    &self,
    room: String,
    sender: mpsc::Sender<Message>,
  )
  {
    let mut connections = self.connections.write();
    connections.await.entry(room).or_default().push(sender);
  }

  pub async fn remove(
    &self,
    room: &str,
    sender: &mpsc::Sender<Message>,
  )
  {
    if let Some(senders) = self.connections.write().await.get_mut(room)
    {
      senders.retain(|s| !s.same_channel(sender));
      if senders.is_empty()
      {
        self.connections.write().await.remove(room);
      }
    }
  }
}

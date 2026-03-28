use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use warp::ws::Message;

pub struct ConnectionPool {
    connections: RwLock<HashMap<String, Vec<mpsc::Sender<Message>>>>,
}

impl ConnectionPool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            connections: RwLock::new(HashMap::new()),
        })
    }

    pub async fn add(&self, room: String, sender: mpsc::Sender<Message>) {
        self.connections
            .write()
            .await
            .entry(room)
            .or_default()
            .push(sender);
    }

    pub async fn remove(&self, room: &str, sender: &mpsc::Sender<Message>) {
        let mut guard = self.connections.write().await;
        let is_empty = if let Some(senders) = guard.get_mut(room) {
            senders.retain(|s| !s.same_channel(sender));
            senders.is_empty()
        } else {
            false
        };
        if is_empty {
            guard.remove(room);
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    /// Adding a connection stores it under the given room key.
    #[tokio::test]
    async fn test_add_connection_to_room() {
        let pool = ConnectionPool::new();
        let (tx, _rx) = mpsc::channel::<Message>(1);
        pool.add("room-add".to_string(), tx).await;
        let guard = pool.connections.read().await;
        assert!(guard.contains_key("room-add"));
        assert_eq!(guard["room-add"].len(), 1);
    }

    /// Multiple connections for the same room accumulate in the same entry.
    #[tokio::test]
    async fn test_add_multiple_connections_to_same_room() {
        let pool = ConnectionPool::new();
        let (tx1, _rx1) = mpsc::channel::<Message>(1);
        let (tx2, _rx2) = mpsc::channel::<Message>(1);
        pool.add("room-multi".to_string(), tx1).await;
        pool.add("room-multi".to_string(), tx2).await;
        let guard = pool.connections.read().await;
        assert_eq!(guard["room-multi"].len(), 2);
    }

    /// Removing the only connection deletes the room entry entirely.
    #[tokio::test]
    async fn test_remove_last_connection_cleans_up_room() {
        let pool = ConnectionPool::new();
        let (tx, _rx) = mpsc::channel::<Message>(1);
        pool.add("room-cleanup".to_string(), tx.clone()).await;
        pool.remove("room-cleanup", &tx).await;
        let guard = pool.connections.read().await;
        assert!(!guard.contains_key("room-cleanup"));
    }

    /// Removing one of several connections leaves the rest intact.
    #[tokio::test]
    async fn test_remove_one_of_multiple_connections() {
        let pool = ConnectionPool::new();
        let (tx1, _rx1) = mpsc::channel::<Message>(1);
        let (tx2, _rx2) = mpsc::channel::<Message>(1);
        pool.add("room-partial".to_string(), tx1.clone()).await;
        pool.add("room-partial".to_string(), tx2.clone()).await;
        pool.remove("room-partial", &tx1).await;
        let guard = pool.connections.read().await;
        assert_eq!(guard["room-partial"].len(), 1);
    }

    /// Calling remove for a room that does not exist is a no-op and does not
    /// panic.
    #[tokio::test]
    async fn test_remove_from_nonexistent_room_is_noop() {
        let pool = ConnectionPool::new();
        let (tx, _rx) = mpsc::channel::<Message>(1);
        pool.remove("nonexistent-room", &tx).await;
    }
}

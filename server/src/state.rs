use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use dashmap::{DashMap, Entry};
use tokio::sync::Mutex;

use crate::room::Room;

pub struct State {
    next_peer_id: Arc<AtomicU32>,
    next_room_id: AtomicU32,
    rooms: DashMap<String, Arc<Mutex<Room>>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            next_peer_id: Arc::new(AtomicU32::new(1)),
            next_room_id: AtomicU32::new(1),
            rooms: DashMap::new(),
        }
    }

    pub fn get_room(&self, code: String) -> Arc<Mutex<Room>> {
        let id = self.next_room_id.fetch_add(1, Ordering::Relaxed);
        match self.rooms.entry(code) {
            Entry::Occupied(entry) => Arc::clone(entry.get()),
            Entry::Vacant(entry) => {
                let room = Arc::new(Mutex::new(Room::new(id, Arc::clone(&self.next_peer_id))));
                entry.insert(Arc::clone(&room));
                room
            }
        }
    }

    pub fn remove_room(&self, code: &String) {
        self.rooms.remove(code);
    }
}

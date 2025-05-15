use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use dashmap::{DashMap, Entry};
use tokio::sync::Mutex;
use webrtc::{
    peer_connection::RTCPeerConnection,
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

use crate::signal::Sender;

pub struct PeerMedia {
    pub track: Arc<TrackLocalStaticRTP>,
    pub ssrc: u32,
}

pub struct Peer {
    pub id: u32,
    pub conn: RTCPeerConnection,
    pub signal_tx: Sender,
    pub name: Option<String>,
    pub video: Option<PeerMedia>,
    pub audio: Option<PeerMedia>,
}

pub struct Room {
    next_peer_id: Arc<AtomicU32>,
    pub id: u32,
    pub peers: Vec<Peer>,
}

impl Room {
    fn new(id: u32, next_peer_id: Arc<AtomicU32>) -> Self {
        Self {
            id,
            next_peer_id,
            peers: Vec::new(),
        }
    }

    fn get_peer_index(&self, id: u32) -> usize {
        self.peers.iter().position(|peer| peer.id == id).unwrap()
    }

    pub fn get_peer(&self, id: u32) -> &Peer {
        let index = self.get_peer_index(id);
        &self.peers[index]
    }

    pub fn get_peer_mut(&mut self, id: u32) -> &mut Peer {
        let index = self.get_peer_index(id);
        &mut self.peers[index]
    }

    pub fn add_peer(&mut self, conn: RTCPeerConnection, signal_tx: Sender) -> u32 {
        let id = self.next_peer_id.fetch_add(1, Ordering::Relaxed);
        self.peers.push(Peer {
            id,
            conn,
            signal_tx,
            name: None,
            video: None,
            audio: None,
        });
        id
    }
}

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
}

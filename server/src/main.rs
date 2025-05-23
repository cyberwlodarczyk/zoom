use crate::{
    signal::{PeerMessage, ServerMessage},
    state::State,
};
use axum::{
    Router,
    body::Body,
    extract::{
        self, Query,
        ws::{WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use std::{collections::HashMap, sync::Arc};
use tokio::net::TcpListener;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;

mod code;
mod peer;
mod room;
mod signal;
mod state;
mod track;

#[tokio::main]
async fn main() {
    let router = Router::new()
        .route("/code", get(code_handler))
        .route("/signal", get(signal_handler))
        .with_state(Arc::new(State::new()));
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, router).await.unwrap();
}

async fn code_handler() -> String {
    code::generate()
}

async fn signal_handler(
    extract::State(state): extract::State<Arc<State>>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let code = params.get("code");
    if let Some(code) = code {
        let code = code.clone();
        if code::is_valid(&code) {
            return ws.on_upgrade(async move |socket| {
                signal_handler_upgrade(state, socket, code).await;
            });
        }
    }
    return Response::builder().status(400).body(Body::empty()).unwrap();
}

async fn signal_handler_upgrade(state: Arc<State>, socket: WebSocket, code: String) {
    let (signal_tx, mut signal_rx) = signal::channel(socket);

    let room = state.get_room(code.clone());
    let mut room_guard = room.lock().await;
    let id = room_guard.add_peer(signal_tx.clone()).await;
    let peer = room_guard.get_peer(id);

    let (track_tx, mut track_rx) = track::channel(id);

    for kind in [RTPCodecType::Video, RTPCodecType::Audio] {
        peer.add_recvonly_transceiver(kind).await;
    }

    room_guard.add_other_peers_tracks(peer).await;

    let room1 = Arc::clone(&room);
    peer.on_connected(Box::new(move || {
        let room2 = Arc::clone(&room1);
        Box::pin(async move {
            let room_guard = room2.lock().await;
            let message = ServerMessage::Peers(room_guard.get_server_message_peers(id));
            drop(room_guard);

            let mut room_guard = room2.lock().await;
            let peer = room_guard.get_peer_mut(id);
            peer.send_offer().await;
            peer.send_message(message).await;
            if let Some(name) = peer.name.clone() {
                room_guard.send_joined_peer(id, name).await;
            }
        })
    }));

    let signal_tx1 = signal_tx.clone();
    peer.on_candidate(Box::new(move |candidate| {
        let mut signal_tx2 = signal_tx1.clone();
        Box::pin(async move {
            signal_tx2.send(ServerMessage::Candidate(candidate)).await;
        })
    }));

    peer.on_track(Box::new(move |track_remote| {
        track_tx.clone().send(track_remote);
        Box::pin(async {})
    }));

    let room1 = Arc::clone(&room);
    tokio::spawn(async move {
        while let Some(msg) = signal_rx.recv().await {
            let mut room_guard = room1.lock().await;
            let peer = room_guard.get_peer_mut(id);
            match msg {
                PeerMessage::Offer(sdp) => {
                    peer.recv_offer(sdp).await;
                }
                PeerMessage::Answer(sdp) => {
                    peer.recv_answer(sdp).await;
                }
                PeerMessage::Candidate(candidate) => {
                    peer.add_candidate(candidate).await;
                }
                PeerMessage::Name(name) => {
                    peer.set_name(name);
                }
                PeerMessage::Pli(id) => {
                    room_guard.send_pli(id).await;
                }
            }
        }
        let mut room_guard = room1.lock().await;
        room_guard.handle_peer_leave(id).await;
        if room_guard.peers.len() == 0 {
            state.remove_room(&code);
        }
    });

    let room1 = Arc::clone(&room);
    tokio::spawn(async move {
        while let Some(track) = track_rx.recv().await {
            let mut room_guard = room1.lock().await;
            let peer = room_guard.get_peer_mut(id);
            let track_local = Arc::clone(&track.inner.inner);
            peer.set_track(track);
            let send_offer = peer.is_audio_and_video();
            room_guard
                .add_peer_track_to_others(id, track_local, send_offer)
                .await;
        }
    });
}

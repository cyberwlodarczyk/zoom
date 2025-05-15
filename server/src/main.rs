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
use once_cell::sync::Lazy;
use signal::ServerMessagePeer;
use state::PeerMedia;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use webrtc::{
    api::{APIBuilder, media_engine::MediaEngine},
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication,
    rtp_transceiver::{
        RTCRtpTransceiverInit, rtp_codec::RTPCodecType,
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
    },
    track::track_local::{
        TrackLocal, TrackLocalWriter, track_local_static_rtp::TrackLocalStaticRTP,
    },
};

mod signal;
mod state;

static MEDIA_ENGINE_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn create_media_engine() -> MediaEngine {
    let mut engine = MediaEngine::default();
    let _lock = MEDIA_ENGINE_MUTEX.lock().unwrap();
    engine.register_default_codecs().unwrap();
    return engine;
}

async fn create_peer_connection() -> RTCPeerConnection {
    let api = APIBuilder::new()
        .with_media_engine(create_media_engine())
        .build();
    let config = RTCConfiguration::default();
    return api.new_peer_connection(config).await.unwrap();
}

fn is_valid_code(code: &str) -> bool {
    if code.len() != 11 {
        return false;
    }
    for (i, c) in code.bytes().enumerate() {
        if i == 3 || i == 7 {
            if c != b'-' {
                return false;
            }
        } else if c < b'a' || c > b'z' {
            return false;
        }
    }
    return true;
}

#[tokio::main]
async fn main() {
    let router = Router::new()
        .route("/signal", get(signal_handler))
        .with_state(Arc::new(State::new()));
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, router).await.unwrap();
}

async fn signal_handler(
    extract::State(state): extract::State<Arc<State>>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let code = params.get("code");
    if let Some(code) = code {
        let code = code.clone();
        if is_valid_code(&code) {
            return ws.on_upgrade(async move |socket| {
                signal_handler_upgrade(state, socket, code).await;
            });
        }
    }
    return Response::builder().status(400).body(Body::empty()).unwrap();
}

async fn signal_handler_upgrade(state: Arc<State>, socket: WebSocket, code: String) {
    let (mut signal_tx, mut signal_rx) = signal::channel(socket);
    let (local_track_tx, mut local_track_rx) = mpsc::channel::<(RTPCodecType, PeerMedia)>(2);
    let peer_conn = create_peer_connection().await;

    let room = state.get_room(code);
    let mut room_guard = room.lock().await;
    let room_id = room_guard.id;
    let id = room_guard.add_peer(peer_conn, signal_tx.clone());
    let peer = room_guard.get_peer(id);

    println!("[{room_id}][{id}] new peer");
    signal_tx.send(ServerMessage::Id(id)).await;

    println!("[{room_id}][{id}] adding recvonly transceivers");
    for kind in [RTPCodecType::Video, RTPCodecType::Audio] {
        peer.conn
            .add_transceiver_from_kind(
                kind,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .unwrap();
    }

    for other in &room_guard.peers {
        if other.id == id {
            continue;
        }
        println!("[{}][{}] adding peer {} tracks", room_id, id, other.id);
        for media in [&other.video, &other.audio] {
            if let Some(media) = media {
                peer.conn
                    .add_track(Arc::clone(&media.track) as Arc<dyn TrackLocal + Send + Sync>)
                    .await
                    .unwrap();
            }
        }
    }

    let room1 = Arc::clone(&room);
    peer.conn
        .on_peer_connection_state_change(Box::new(move |state| {
            if state != RTCPeerConnectionState::Connected {
                return Box::pin(async {});
            }
            println!("[{room_id}][{id}] connection established");
            let room2 = Arc::clone(&room1);
            Box::pin(async move {
                let room_guard = room2.lock().await;
                let peer = room_guard.get_peer(id);
                let name = peer.name.clone();
                let mut signal_tx = peer.signal_tx.clone();
                drop(room_guard);
                let room_guard = room2.lock().await;
                signal_tx
                    .send(ServerMessage::Peers(
                        room_guard
                            .peers
                            .iter()
                            .filter(|p| {
                                p.conn.connection_state() == RTCPeerConnectionState::Connected
                                    && p.id != id
                            })
                            .map(|p| ServerMessagePeer {
                                id: p.id,
                                name: p.name.clone().unwrap(),
                            })
                            .collect(),
                    ))
                    .await;
                drop(room_guard);
                let mut room_guard = room2.lock().await;
                for other in &mut room_guard.peers {
                    if other.id == id {
                        continue;
                    }
                    if let Some(name) = name.clone() {
                        other
                            .signal_tx
                            .send(ServerMessage::Peer(ServerMessagePeer { id, name }))
                            .await;
                    }
                }
            })
        }));

    let signal_tx1 = signal_tx.clone();
    let room1 = Arc::clone(&room);
    peer.conn.on_negotiation_needed(Box::new(move || {
        println!("[{room_id}][{id}] negotiation needed");
        let mut signal_tx2 = signal_tx1.clone();
        let room2 = Arc::clone(&room1);
        Box::pin(async move {
            let room_guard = room2.lock().await;
            let peer = room_guard.get_peer(id);
            if peer.conn.connection_state() != RTCPeerConnectionState::Connected {
                return;
            }
            let offer = peer.conn.create_offer(None).await.unwrap();
            peer.conn
                .set_local_description(offer.clone())
                .await
                .unwrap();
            signal_tx2.send(ServerMessage::Offer(offer.sdp)).await;
            println!("[{room_id}][{id}] offer sent")
        })
    }));

    let signal_tx1 = signal_tx.clone();
    peer.conn.on_ice_candidate(Box::new(move |candidate| {
        println!("[{room_id}][{id}] new local ice candidate");
        let mut signal_tx2 = signal_tx1.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                signal_tx2
                    .send(ServerMessage::Candidate(candidate.to_json().unwrap()))
                    .await;
            }
        })
    }));

    let local_track_tx1 = Arc::new(local_track_tx);
    peer.conn.on_track(Box::new(move |remote_track, _, _| {
        println!("[{room_id}][{id}] new remote track");
        let local_track_tx2 = Arc::clone(&local_track_tx1);
        tokio::spawn(async move {
            let local_track = Arc::new(TrackLocalStaticRTP::new(
                remote_track.codec().capability,
                format!("{}-{}", id.to_string(), remote_track.kind()),
                remote_track.stream_id(),
            ));
            local_track_tx2
                .send((
                    remote_track.kind(),
                    PeerMedia {
                        track: Arc::clone(&local_track),
                        ssrc: remote_track.ssrc(),
                    },
                ))
                .await
                .unwrap();
            while let Ok((rtp, _)) = remote_track.read_rtp().await {
                local_track.write_rtp(&rtp).await.unwrap();
            }
        });
        Box::pin(async {})
    }));

    drop(room_guard);

    let mut signal_tx1 = signal_tx.clone();
    let room1 = Arc::clone(&room);
    tokio::spawn(async move {
        while let Some(msg) = signal_rx.recv().await {
            let mut room_guard = room1.lock().await;
            let peer = room_guard.get_peer_mut(id);
            match msg {
                PeerMessage::Offer(sdp) => {
                    println!("[{room_id}][{id}] offer received");
                    peer.conn
                        .set_remote_description(RTCSessionDescription::offer(sdp).unwrap())
                        .await
                        .unwrap();
                    let answer = peer.conn.create_answer(None).await.unwrap();
                    peer.conn
                        .set_local_description(answer.clone())
                        .await
                        .unwrap();
                    signal_tx1.send(ServerMessage::Answer(answer.sdp)).await;
                    println!("[{room_id}][{id}] answer sent");
                }
                PeerMessage::Answer(sdp) => {
                    println!("[{room_id}][{id}] answer received");
                    peer.conn
                        .set_remote_description(RTCSessionDescription::answer(sdp).unwrap())
                        .await
                        .unwrap();
                }
                PeerMessage::Candidate(candidate) => {
                    println!("[{room_id}][{id}] new remote ice candidate");
                    peer.conn.add_ice_candidate(candidate).await.unwrap();
                }
                PeerMessage::Name(name) => peer.name = Some(name),
                PeerMessage::Pli(id) => {
                    println!("[{}][{}] pli requested for peer {}", room_id, peer.id, id);
                    for peer in &room_guard.peers {
                        if peer.id == id {
                            if let Some(video) = &peer.video {
                                peer.conn
                                    .write_rtcp(&[Box::new(PictureLossIndication {
                                        sender_ssrc: 0,
                                        media_ssrc: video.ssrc,
                                    })])
                                    .await
                                    .unwrap();
                            }
                        }
                    }
                }
            }
        }
    });

    while let Some((kind, media)) = local_track_rx.recv().await {
        let mut room_guard = room.lock().await;
        let peer = room_guard.get_peer_mut(id);
        let track = Arc::clone(&media.track);
        match kind {
            RTPCodecType::Audio => peer.audio = Some(media),
            RTPCodecType::Video => peer.video = Some(media),
            _ => {}
        }
        for other in &room_guard.peers {
            if other.id == id {
                continue;
            }
            println!(
                "[{}][{}] adding peer {} {} track",
                room_id, other.id, id, kind
            );
            other
                .conn
                .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
                .await
                .unwrap();
        }
    }
}

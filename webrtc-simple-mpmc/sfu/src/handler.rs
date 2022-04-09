use std::collections::{HashMap, HashSet};
use std::convert::From;
use std::sync::{Arc, Mutex};

use warp::ws::Message;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
// use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::Error;

use log::{error, info};

use crate::errors::ApplicationError;

const TRACK_NAME_PREF: &str = "sfu-track-";

#[derive(Debug)]
pub enum RTCPToPublisher {
    PLI,
}

#[derive(Debug)]
pub enum MessageToPublisher {
    RTCP(RTCPToPublisher),
}

#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
pub enum SubscriberMessageType {
    Prepare,
    Start,
    Offer,
    Answer,
    IceCandidate,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SubscriberMessage {
    pub msg_type: SubscriberMessageType,
    pub message: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct ClientIceCandidate {
    pub candidate: Option<String>,
    #[serde(rename = "spdMid")]
    pub sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex")]
    pub sdp_mline_index: Option<u16>,
    #[serde(rename = "usernameFragment")]
    pub username_fragment: Option<String>,
}

impl ClientIceCandidate {
    fn to_candidate_init(self) -> RTCIceCandidateInit {
        RTCIceCandidateInit {
            candidate: self.candidate.unwrap_or("".to_owned()),
            sdp_mid: self.sdp_mid.unwrap_or("".to_owned()),
            sdp_mline_index: self.sdp_mline_index.unwrap_or(0u16),
            username_fragment: self.username_fragment.unwrap_or("".to_owned()),
        }
    }

    fn from(another: RTCIceCandidateInit) -> ClientIceCandidate {
        ClientIceCandidate {
            candidate: Some(another.candidate),
            sdp_mid: Some(another.sdp_mid),
            sdp_mline_index: Some(another.sdp_mline_index),
            username_fragment: Some(another.username_fragment),
        }
    }
}

type ToPublisherChannel = tokio::sync::mpsc::UnboundedSender<MessageToPublisher>;
type ToSubscriberChannel = tokio::sync::mpsc::UnboundedSender<SubscriberMessage>;

pub struct PeerManager {
    tracks: HashMap<Uuid, Vec<Arc<TrackLocalStaticRTP>>>,
    to_publishers: HashMap<Uuid, ToPublisherChannel>,
    to_subscribers: HashMap<Uuid, ToSubscriberChannel>,
}

impl PeerManager {
    pub fn new() -> Self {
        PeerManager {
            tracks: HashMap::new(),
            to_publishers: HashMap::new(),
            to_subscribers: HashMap::new(),
        }
    }

    pub fn add_peer(
        &mut self,
        peer_id: &Uuid,
        to_pub_ch: ToPublisherChannel,
        to_sub_ch: ToSubscriberChannel,
    ) {
        self.to_publishers.insert(peer_id.clone(), to_pub_ch);
        self.to_subscribers.insert(peer_id.clone(), to_sub_ch);
    }
    pub fn add_track(&mut self, peer_id: &Uuid, track: Arc<TrackLocalStaticRTP>) {
        let tracks = self.tracks.entry(peer_id.clone()).or_insert(Vec::new());
        tracks.push(track);
    }

    pub fn has_both_audio_and_video(&self, peer_id: &Uuid) -> bool {
        self.tracks
            .get(&peer_id)
            .map(|t| t.len() == 2)
            .unwrap_or(false)
    }

    fn remove_peer(&mut self, peer_id: &Uuid) {
        self.tracks.remove(peer_id);
        self.to_publishers.remove(peer_id);
        self.to_subscribers.remove(peer_id);
    }

    pub fn send_to_subscribers(&self, message: SubscriberMessage) {
        for (sub_id, tx_ch) in self.to_subscribers.iter() {
            info!("Require renegotiation to subscriber {:?}", sub_id);

            if let Err(e) = tx_ch.send(SubscriberMessage {
                msg_type: message.msg_type,
                message: message.message.clone(),
            }) {
                error!("Error while sending a message to {:?} {:?}", sub_id, e);
            }
        }
    }

    fn publisher_tracks_info(
        &self,
        peer_id: &Uuid,
    ) -> (HashSet<String>, Vec<(Uuid, Arc<TrackLocalStaticRTP>)>) {
        let mut local_tracks = vec![];
        let mut local_track_ids = HashSet::new();
        for (pc_id, ts) in self.tracks.iter() {
            if peer_id == pc_id {
                continue;
            }

            for local_track in ts {
                local_tracks.push((pc_id.clone(), Arc::clone(&local_track)));
                local_track_ids.insert(local_track.id().to_owned());
            }
        }
        (local_track_ids, local_tracks)
    }

    fn send_to_publisher(&self, pc_id: &Uuid, message: MessageToPublisher) {
        if let Some(sender) = self.to_publishers.get(pc_id) {
            if let Err(e) = sender.send(message) {
                error!("Error while sending a message to {:?} {:?}", pc_id, e);
            }
        }
    }
}

pub type PeerManagerRef = Arc<Mutex<PeerManager>>;

pub fn on_track(
    peer_id: &Uuid,
    track: Option<Arc<TrackRemote>>,
    track_ssrc_tx: Arc<tokio::sync::mpsc::Sender<u32>>,
    local_track_chan_tx: Arc<tokio::sync::mpsc::Sender<Arc<TrackLocalStaticRTP>>>,
) {
    let peer_id = peer_id.clone();
    if let Some(track) = track {
        info!("on_track {:?} on {:?}.", track.kind(), peer_id);

        if track.kind() == RTPCodecType::Video {
            let media_ssrc = track.ssrc();
            let track_ssrc_tx = Arc::clone(&track_ssrc_tx);
            tokio::spawn(async move {
                if let Err(e) = track_ssrc_tx.send(media_ssrc).await {
                    error!("{:?} on {:?}.", e, peer_id);
                }
            });
        }

        let local_track_chan_tx2 = Arc::clone(&local_track_chan_tx);
        tokio::spawn(async move {
            let local_track = Arc::new(TrackLocalStaticRTP::new(
                track.codec().await.capability,
                format!(
                    "{}-{:?}-{:?}",
                    TRACK_NAME_PREF,
                    track.kind(),
                    Uuid::new_v4()
                ),
                format!("sfu-stream-{:?}", peer_id),
            ));

            let _ = local_track_chan_tx2.send(Arc::clone(&local_track)).await;

            while let Ok((rtp, _)) = track.read_rtp().await {
                if let Err(e) = local_track.write_rtp(&rtp).await {
                    if Error::ErrClosedPipe != e {
                        error!(
                            "output track write_rtp got error: {} and break on {:?}.",
                            e, peer_id
                        );
                        break;
                    } else {
                        error!("output track write_rtp got error: {} on {:?}.", e, peer_id);
                    }
                }
            }
        });
    }
}

pub fn on_peer_connection_state_change(
    state: RTCPeerConnectionState,
    peer_id: &Uuid,
    peer_manager: PeerManagerRef,
) {
    info!(
        "Peer connection state has changed to {} on {:?}.",
        state, peer_id
    );

    if state == RTCPeerConnectionState::Disconnected {
        let mut peer_manager = peer_manager.lock().unwrap();
        peer_manager.remove_peer(&peer_id);
        peer_manager.send_to_subscribers(SubscriberMessage {
            msg_type: SubscriberMessageType::Start,
            message: String::from(""),
        });
    }
}

pub fn on_negotiation_needed(
    peer_id: &Uuid,
    peer_connection: Arc<RTCPeerConnection>,
    tx_ws: tokio::sync::mpsc::UnboundedSender<warp::ws::Message>,
) {
    info!(
        "Negotiation has been needed on {:?} - {:?}.",
        peer_id,
        peer_connection.signaling_state()
    );

    let peer_id = peer_id.clone();
    tokio::spawn(async move {
        if let Err(e) = do_offer(peer_connection, tx_ws).await {
            error!("{:?} on {:?}.", e, peer_id);
        }
    });
}

pub fn on_ice_candidate(
    peer_id: &Uuid,
    candidate: RTCIceCandidate,
    tx_ws: tokio::sync::mpsc::UnboundedSender<warp::ws::Message>,
) {
    let tx_ws_facade_for_ice_candidate = tx_ws.clone();
    let peer_id = peer_id.clone();
    tokio::spawn(async move {
        let candidate_json = match candidate.to_json().await {
            Ok(c) => c,
            Err(e) => {
                error!("{:?} on {:?}.", e, peer_id);
                return;
            }
        };

        let candidate_str = match serde_json::to_string(&ClientIceCandidate::from(candidate_json)) {
            Ok(c) => c,
            Err(e) => {
                error!("{:?} on {:?}.", e, peer_id);
                return;
            }
        };

        let ret_message = match serde_json::to_string(&SubscriberMessage {
            msg_type: SubscriberMessageType::IceCandidate,
            message: candidate_str,
        }) {
            Ok(m) => m,
            Err(e) => {
                error!("{:?} on {:?}.", e, peer_id);
                return;
            }
        };

        if let Err(e) = tx_ws_facade_for_ice_candidate.send(Message::text(ret_message)) {
            error!("{:?} on {:?}.", e, peer_id);
        }
    });
}

pub async fn handle_ice_candidate_message(
    peer_id: &Uuid,
    msg: &SubscriberMessage,
    pc: Arc<RTCPeerConnection>,
) -> Result<(), ApplicationError> {
    if &msg.message == "null" {
        return Ok(());
    }

    info!(
        "An ICE candidate has been received on {:?} {}.",
        peer_id, &msg.message
    );

    let ice_candidate =
        serde_json::from_str::<ClientIceCandidate>(&msg.message)?.to_candidate_init();
    pc.add_ice_candidate(ice_candidate).await?;

    Ok(())
}

pub async fn handle_answer_message(
    peer_id: &Uuid,
    msg: &SubscriberMessage,
    pc: Arc<RTCPeerConnection>,
) -> Result<(), ApplicationError> {
    info!("Receive answer on {:?}.", peer_id);

    let answer = serde_json::from_str::<RTCSessionDescription>(&msg.message)?;
    pc.set_remote_description(answer).await?;

    Ok(())
}

pub async fn handle_start_message(
    peer_id: &Uuid,
    pc: Arc<RTCPeerConnection>,
    peer_manager: PeerManagerRef,
    tx_ws: tokio::sync::mpsc::UnboundedSender<warp::ws::Message>,
) -> Result<(), ApplicationError> {
    info!("Prepare tracks on {:?}.", peer_id);
    let peer_id = peer_id.clone();
    let local_track_ids;
    let local_tracks;

    {
        let peer_manager = peer_manager.lock().unwrap();
        let (ids, tracks) = peer_manager.publisher_tracks_info(&peer_id);
        local_track_ids = ids;
        local_tracks = tracks;
    }

    let mut existing_track_ids = HashSet::new();
    let senders = pc.get_senders().await;

    for sender in senders {
        if let Some(t) = sender.track().await {
            let track_id = t.id().to_owned();

            if track_id.find(TRACK_NAME_PREF).unwrap_or(1) != 0 {
                continue;
            }

            if !local_track_ids.contains(&track_id) {
                info!("Remove the track {:?} from {:?}", track_id, peer_id);
                if let Err(e) = pc.remove_track(&sender).await {
                    error!("Error while removing track {:?} {:?}.", track_id, e);
                }
            }
            existing_track_ids.insert(track_id);
        }
    }
    info!(
        "The number of publisher's tracks is {}. Existing track track_ids are {:?} on {:?}.",
        local_tracks.len(),
        existing_track_ids,
        peer_id
    );

    if local_tracks.len() == 0 {
        info!("No publisher for {:?}", peer_id);
        return Ok(());
    }

    for (publisher_peer_id, local_track) in local_tracks {
        let track_id = local_track.id();
        if existing_track_ids.contains(track_id) {
            info!(
                "The specified track already exists {:?} on {:?}.",
                track_id, peer_id
            );
            continue;
        }
        let track_id = track_id.to_owned();
        match pc
            .add_track(local_track as Arc<dyn TrackLocal + Send + Sync>)
            .await
        {
            Ok(rtp_sender) => {
                let peer_manager_for_rtcp = peer_manager.clone();
                tokio::spawn(async move {
                    let mut rtcp_buf = vec![0u8; 1500];
                    while let Ok((n, _)) = rtp_sender.read(&mut rtcp_buf).await {
                        let mut buf = &rtcp_buf[..n];
                        let peer_manager = peer_manager_for_rtcp.lock().unwrap();
                        // https://stackoverflow.com/questions/33687447/how-to-get-a-reference-to-a-concrete-type-from-a-trait-object
                        if let Ok(packets) = webrtc::rtcp::packet::unmarshal(&mut buf) {
                            for packet in packets {
                                if let Some(pli_packet) =
                                    packet.as_any().downcast_ref::<PictureLossIndication>()
                                {
                                    info!("{:?} on {:?}", pli_packet, peer_id);
                                    peer_manager.send_to_publisher(
                                        &publisher_peer_id,
                                        MessageToPublisher::RTCP(RTCPToPublisher::PLI),
                                    );
                                }
                            }
                        }
                    }
                });
            }
            Err(e) => error!("{:?} on {:?}", e, peer_id),
        }

        info!("Add a track {:?} to {:?}.", track_id, peer_id);
    }
    do_offer(pc, tx_ws).await?;

    Ok(())
}

pub async fn do_offer(
    peer_connection: Arc<RTCPeerConnection>,
    tx_ws: tokio::sync::mpsc::UnboundedSender<warp::ws::Message>,
) -> Result<(), ApplicationError> {
    let offer = peer_connection.create_offer(None).await?;

    // TODO For some reason, this promise resolves before ice_gathering_state become complete.
    // let mut gather_complete = peer_connection.gathering_complete_promise().await;

    peer_connection.set_local_description(offer).await?;

    // let _ = gather_complete.recv().await;

    if let Some(local_description) = peer_connection.local_description().await {
        let sdp_str = serde_json::to_string(&local_description)?;

        let ret_message = serde_json::to_string(&SubscriberMessage {
            msg_type: SubscriberMessageType::Offer,
            message: sdp_str,
        })?;

        tx_ws.send(Message::text(ret_message))?
    }
    Ok(())
}

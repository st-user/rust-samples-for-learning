use std::collections::{HashMap, HashSet};
use std::convert::From;
use std::sync::{Arc, Mutex};

use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;
use warp::{http::StatusCode, reject, ws::Message, Filter, Rejection, Reply};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
// use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::Error;

use log::{error, info, warn};

mod logger;

#[derive(Deserialize, Serialize, Debug)]
struct OfferBody {
    sdp: String,
    _type: String,
}

#[derive(Debug)]
enum RTCPToPublisher {
    PLI,
}

#[derive(Debug)]
enum MessageToPublisher {
    RTCP(RTCPToPublisher),
}

#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
enum SubscriberMessageType {
    Start,
    Offer,
    Answer,
    Prepare,
}

#[derive(Deserialize, Serialize, Debug)]
struct SubscriberMessage {
    msg_type: SubscriberMessageType,
    message: String,
}

type WsSendError = tokio::sync::mpsc::error::SendError<warp::ws::Message>;

#[derive(Debug)]
enum ApplicationError {
    Any(()),
    Json(serde_json::Error),
    Web(warp::Error),
    WebRTC(webrtc::Error),
    WsSend(WsSendError),
}

impl From<()> for ApplicationError {
    fn from(item: ()) -> Self {
        ApplicationError::Any(item)
    }
}

impl From<serde_json::Error> for ApplicationError {
    fn from(item: serde_json::Error) -> Self {
        ApplicationError::Json(item)
    }
}

impl From<warp::Error> for ApplicationError {
    fn from(item: warp::Error) -> Self {
        ApplicationError::Web(item)
    }
}

impl From<webrtc::Error> for ApplicationError {
    fn from(item: webrtc::Error) -> Self {
        ApplicationError::WebRTC(item)
    }
}

impl From<WsSendError> for ApplicationError {
    fn from(item: WsSendError) -> Self {
        ApplicationError::WsSend(item)
    }
}

impl reject::Reject for ApplicationError {}

#[derive(Serialize)]
struct ErrorMessage {
    pub code: u16,
    pub message: String,
}

type ToPublisherChannel = tokio::sync::mpsc::UnboundedSender<MessageToPublisher>;
type ToSubscriberChannel = tokio::sync::mpsc::UnboundedSender<SubscriberMessage>;

struct PeerManager {
    tracks: HashMap<Uuid, Vec<Arc<TrackLocalStaticRTP>>>,
    to_publishers: HashMap<Uuid, ToPublisherChannel>,
    to_subscribers: HashMap<Uuid, ToSubscriberChannel>,
}

impl PeerManager {
    fn new() -> Self {
        PeerManager {
            tracks: HashMap::new(),
            to_publishers: HashMap::new(),
            to_subscribers: HashMap::new(),
        }
    }

    fn add_peer(
        &mut self,
        peer_id: &Uuid,
        to_pub_ch: ToPublisherChannel,
        to_sub_ch: ToSubscriberChannel,
    ) {
        self.to_publishers.insert(peer_id.clone(), to_pub_ch);
        self.to_subscribers.insert(peer_id.clone(), to_sub_ch);
    }

    fn remove_peer(&mut self, peer_id: &Uuid) {
        self.tracks.remove(peer_id);
        self.to_publishers.remove(peer_id);
        self.to_subscribers.remove(peer_id);
    }

    fn add_track(&mut self, peer_id: &Uuid, track: Arc<TrackLocalStaticRTP>) {
        let tracks = self.tracks.entry(peer_id.clone()).or_insert(Vec::new());
        tracks.push(track);
    }

    fn has_both_audio_and_video(&self, peer_id: &Uuid) -> bool {
        self.tracks
            .get(&peer_id)
            .map(|t| t.len() == 2)
            .unwrap_or(false)
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

    fn send_to_subscribers(&self, message: SubscriberMessage) {
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
}

type PeerManagerRef = Arc<Mutex<PeerManager>>;

#[tokio::main]
async fn main() {
    logger::init_logger();

    let ws_context = warp::path("ws-app");
    let peer_manager = Arc::new(Mutex::new(PeerManager::new()));

    let subscribe = ws_context
        .and(warp::path("subscribe"))
        .and(warp::ws())
        .and(with_peer_manager(peer_manager.clone()))
        .map(|ws: warp::ws::Ws, peer_manager: PeerManagerRef| {
            ws.on_upgrade(|websocket| handle_peer(websocket, peer_manager))
        });

    let route = subscribe.recover(handle_rejection);

    warp::serve(route).run(([127, 0, 0, 1], 9001)).await;
}

fn with_peer_manager(
    peer_manager: PeerManagerRef,
) -> impl Filter<Extract = (PeerManagerRef,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || peer_manager.clone())
}

async fn handle_peer(ws: warp::ws::WebSocket, peer_manager: PeerManagerRef) {
    if let Err(e) = handle_peer_delegate(ws, peer_manager).await {
        error!("Error on handle_subscribe {:?}.", e);
    }
}

async fn handle_peer_delegate(
    ws: warp::ws::WebSocket,
    peer_manager: PeerManagerRef,
) -> Result<(), ApplicationError> {
    let peer_id = Uuid::new_v4();

    let (mut tx_ws, mut rx_ws) = ws.split();
    let (tx_ws_facade, rx_ws_facade) = unbounded_channel();
    let mut rx_ws_facade: UnboundedReceiverStream<warp::ws::Message> =
        UnboundedReceiverStream::new(rx_ws_facade);

    let (tx_main_to_subscriber, rx_main_to_subscriber) = unbounded_channel();
    let mut rx_main_to_subscriber: UnboundedReceiverStream<SubscriberMessage> =
        UnboundedReceiverStream::new(rx_main_to_subscriber);

    let (tx_main_to_publisher, rx_main_to_publisher) = unbounded_channel();
    let mut rx_main_to_publisher: UnboundedReceiverStream<MessageToPublisher> =
        UnboundedReceiverStream::new(rx_main_to_publisher);

    {
        let mut peer_manager = peer_manager.lock().unwrap();
        peer_manager.add_peer(
            &peer_id,
            tx_main_to_publisher,
            tx_main_to_subscriber.clone(),
        );
    }

    //
    // Send messages through websocket connection to the peer.
    //
    tokio::spawn(async move {
        while let Some(msg) = rx_ws_facade.next().await {
            if let Err(e) = tx_ws.send(msg).await {
                error!("{:?} on {:?}.", e, peer_id);
            }
        }
    });

    let peer_connection = Arc::new(new_base_peer_connection().await?);

    peer_connection
        .add_transceiver_from_kind(RTPCodecType::Video, &[])
        .await?;
    peer_connection
        .add_transceiver_from_kind(RTPCodecType::Audio, &[])
        .await?;

    let (local_track_chan_tx, mut local_track_chan_rx) =
        tokio::sync::mpsc::channel::<Arc<TrackLocalStaticRTP>>(2);

    let (track_ssrc_tx, mut track_ssrc_rx) =
        tokio::sync::mpsc::channel::<webrtc::rtp_transceiver::SSRC>(1);

    let local_track_chan_tx = Arc::new(local_track_chan_tx);
    let track_ssrc_tx = Arc::new(track_ssrc_tx);

    //
    // In order to publish a video and an audio, this pc should handle tracks from the client.
    //
    peer_connection
        .on_track(Box::new(
            move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
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
                            format!("t-{:?}-{:?}", track.kind(), Uuid::new_v4()),
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
                                    error!(
                                        "output track write_rtp got error: {} on {:?}.",
                                        e, peer_id
                                    );
                                }
                            }
                        }
                    });
                }

                Box::pin(async {})
            },
        ))
        .await;

    let peer_manager_for_state_change = peer_manager.clone();
    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            info!(
                "Peer connection state has changed to {} on {:?}.",
                s, peer_id
            );

            if s == RTCPeerConnectionState::Disconnected {
                let mut peer_manager = peer_manager_for_state_change.lock().unwrap();
                peer_manager.remove_peer(&peer_id);
                peer_manager.send_to_subscribers(SubscriberMessage {
                    msg_type: SubscriberMessageType::Start,
                    message: String::from(""),
                });
            }

            Box::pin(async {})
        }))
        .await;

    let peer_manager_for_track_add = peer_manager.clone();
    tokio::spawn(async move {
        loop {
            if let Some(track) = local_track_chan_rx.recv().await {
                let mut peer_manager = peer_manager_for_track_add.lock().unwrap();
                peer_manager.add_track(&peer_id, track);

                if peer_manager.has_both_audio_and_video(&peer_id) {
                    peer_manager.send_to_subscribers(SubscriberMessage {
                        msg_type: SubscriberMessageType::Start,
                        message: String::from(""),
                    });
                    info!("Both audio and video track are added to {:?}.", peer_id);
                    break;
                }
            }
        }
    });

    //
    // Forwards RTCP packets to the sender of the media stream.
    //
    let rtcp_observer_pc = peer_connection.clone();
    tokio::spawn(async move {
        if let Some(ssrc) = track_ssrc_rx.recv().await {
            info!("SSRC {:?} detected on {:?}.", ssrc, peer_id);

            while let Some(msg) = rx_main_to_publisher.next().await {
                match msg {
                    MessageToPublisher::RTCP(packet_type) => match packet_type {
                        RTCPToPublisher::PLI => {
                            if let Err(e) = rtcp_observer_pc
                                .write_rtcp(&[Box::new(PictureLossIndication {
                                    sender_ssrc: 0,
                                    media_ssrc: ssrc,
                                })])
                                .await
                            {
                                error!("{:?} on {:?}.", e, peer_id);
                            }
                        }
                    },
                }
            }
        }
    });

    //
    // Reference: https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Perfect_negotiation
    //
    let making_offer = Arc::new(Mutex::new(false));

    //
    // Detect 'negotiation needed' events to send an offer.
    //
    let pc_for_renegotiation = peer_connection.clone();
    let tx_ws_facade_for_renegotiation = tx_ws_facade.clone();
    let making_offer_for_renegotiation = making_offer.clone();
    peer_connection
        .on_negotiation_needed(Box::new(move || {
            let pc_for_renegotiation = pc_for_renegotiation.clone();
            let tx_ws_facade_for_renegotiation = tx_ws_facade_for_renegotiation.clone();

            info!(
                "Negotiation has been needed on {:?} - {:?}.",
                peer_id,
                pc_for_renegotiation.signaling_state()
            );

            let making_offer = Arc::clone(&making_offer_for_renegotiation);
            tokio::spawn(async move {
                if let Err(e) = do_offer(
                    &peer_id,
                    pc_for_renegotiation,
                    tx_ws_facade_for_renegotiation,
                    making_offer,
                )
                .await
                {
                    error!("{:?} on {:?}.", e, peer_id);
                }
            });

            Box::pin(async {})
        }))
        .await;

    //
    // The main event loop that handles messages for negotiation.
    //

    tokio::spawn(async move {
        while let Some(msg) = rx_main_to_subscriber.next().await {
            let pc_for_prepare = peer_connection.clone();
            let tx_ws_facade_for_prepare = tx_ws_facade.clone();
            let making_offer_for_prepare = making_offer.clone();
            match msg.msg_type {
                SubscriberMessageType::Prepare => {
                    info!("Preparation is requested on {:?}.", peer_id);

                    if let Err(e) = do_offer(
                        &peer_id,
                        pc_for_prepare,
                        tx_ws_facade_for_prepare,
                        making_offer_for_prepare,
                    )
                    .await
                    {
                        error!("{:?} on {:?}.", e, peer_id);
                    }
                }
                SubscriberMessageType::Answer => {
                    info!("Receive answer on {:?}.", peer_id);

                    let answer = match serde_json::from_str::<RTCSessionDescription>(&msg.message) {
                        Ok(a) => a,
                        Err(e) => {
                            error!("{:?} on {:?}", e, peer_id);
                            continue;
                        }
                    };
                    if let Err(e) = peer_connection.set_remote_description(answer).await {
                        error!("{:?} on {:?}", e, peer_id);
                        continue;
                    }
                }
                SubscriberMessageType::Start => {
                    info!("Prepare tracks on {:?}.", peer_id);

                    let local_track_ids;
                    let local_tracks;

                    {
                        let peer_manager = peer_manager.lock().unwrap();
                        let (ids, tracks) = peer_manager.publisher_tracks_info(&peer_id);
                        local_track_ids = ids;
                        local_tracks = tracks;
                    }

                    let mut existing_track_ids = HashSet::new();
                    let senders = peer_connection.get_senders().await;

                    for sender in senders {
                        if let Some(t) = sender.track().await {
                            let track_id = t.id().to_owned();
                            if !local_track_ids.contains(&track_id) {
                                info!("Remove the track {:?} from {:?}", track_id, peer_id);
                                if let Err(e) = peer_connection.remove_track(&sender).await {
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
                        continue;
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
                        match peer_connection
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
                                        if let Ok(packets) =
                                            webrtc::rtcp::packet::unmarshal(&mut buf)
                                        {
                                            for packet in packets {
                                                if let Some(pli_packet) = packet
                                                    .as_any()
                                                    .downcast_ref::<PictureLossIndication>(
                                                ) {
                                                    info!("{:?} on {:?}", pli_packet, peer_id);
                                                    peer_manager.send_to_publisher(
                                                        &publisher_peer_id,
                                                        MessageToPublisher::RTCP(
                                                            RTCPToPublisher::PLI,
                                                        ),
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

                    if let Err(e) = do_offer(
                        &peer_id,
                        pc_for_prepare,
                        tx_ws_facade_for_prepare,
                        making_offer_for_prepare,
                    )
                    .await
                    {
                        error!("{:?} on {:?}.", e, peer_id);
                    }
                }
                SubscriberMessageType::Offer => {
                    error!(
                        "Receiving offers is currently not supported ({:?}).",
                        peer_id
                    );
                    continue;
                }
            }
        }
    });

    while let Some(msg) = rx_ws.next().await {
        match msg.map_err(ApplicationError::Web).and_then(|msg| {
            msg.to_str().map_err(ApplicationError::Any).and_then(|s| {
                serde_json::from_str::<SubscriberMessage>(&s).map_err(ApplicationError::Json)
            })
        }) {
            Ok(msg) => {
                if let Err(e) = tx_main_to_subscriber.send(msg) {
                    error!("{:?} on {:?}.", e, peer_id)
                }
            }
            Err(e) => error!("{:?} on {:?}.", e, peer_id),
        }
    }

    Ok(())
}

async fn do_offer(
    peer_id: &Uuid,
    peer_connection: Arc<RTCPeerConnection>,
    tx_ws: tokio::sync::mpsc::UnboundedSender<warp::ws::Message>,
    making_offer: Arc<Mutex<bool>>,
) -> Result<(), ApplicationError> {
    {
        let mut is_making_offer = making_offer.lock().unwrap();
        if *is_making_offer {
            warn!("An offer is being processed on {:?}", peer_id);
            return Ok(());
        }
        *is_making_offer = true;
    }

    let offer = match peer_connection.create_offer(None).await {
        Ok(offer) => offer,
        Err(e) => {
            let mut is_making_offer = making_offer.lock().unwrap();
            *is_making_offer = false;
            return Err(ApplicationError::WebRTC(e));
        }
    };

    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    if let Err(e) = peer_connection.set_local_description(offer).await {
        let mut is_making_offer = making_offer.lock().unwrap();
        *is_making_offer = false;
        return Err(ApplicationError::WebRTC(e));
    }

    let _ = gather_complete.recv().await;

    if let Some(local_description) = peer_connection.local_description().await {
        let sdp_str = match serde_json::to_string(&local_description) {
            Ok(sdp_str) => sdp_str,
            Err(e) => {
                let mut is_making_offer = making_offer.lock().unwrap();
                *is_making_offer = false;
                return Err(ApplicationError::Json(e));
            }
        };

        let ret_message = match serde_json::to_string(&SubscriberMessage {
            msg_type: SubscriberMessageType::Offer,
            message: sdp_str,
        }) {
            Ok(ret_message) => ret_message,
            Err(e) => {
                let mut is_making_offer = making_offer.lock().unwrap();
                *is_making_offer = false;
                return Err(ApplicationError::Json(e));
            }
        };

        if let Err(e) = tx_ws.send(Message::text(ret_message)) {
            let mut is_making_offer = making_offer.lock().unwrap();
            *is_making_offer = false;
            return Err(ApplicationError::WsSend(e));
        }
    }
    let mut is_making_offer = making_offer.lock().unwrap();
    *is_making_offer = false;
    Ok(())
}

async fn new_base_peer_connection() -> Result<RTCPeerConnection, webrtc::Error> {
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    api.new_peer_connection(config).await
}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    error!("handle_rejection {:?}", err);
    Ok(warp::reply::with_status(
        "Internal Server Error",
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

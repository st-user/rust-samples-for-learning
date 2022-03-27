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
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;
// use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::RTCPeerConnection;

use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::Error;

// Both audio and video
// https://github.com/webrtc-rs/examples/blob/5a0e2861c66a45fca93aadf9e70a5b045b26dc9e/examples/save-to-disk-h264/save-to-disk-h264.rs
//

#[derive(Debug)]
struct WebRTCRuntimeError {
    _cause: webrtc::Error,
}

impl reject::Reject for WebRTCRuntimeError {}

impl From<webrtc::Error> for WebRTCRuntimeError {
    fn from(item: webrtc::Error) -> Self {
        WebRTCRuntimeError { _cause: item }
    }
}

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
}

#[derive(Deserialize, Serialize, Debug)]
struct SubscriberMessage {
    msg_type: SubscriberMessageType,
    message: String,
}

#[derive(Debug)]
enum MessagingError {
    Any(()),
    Json(serde_json::Error),
    Web(warp::Error),
}

impl From<()> for MessagingError {
    fn from(item: ()) -> Self {
        MessagingError::Any(item)
    }
}

impl From<serde_json::Error> for MessagingError {
    fn from(item: serde_json::Error) -> Self {
        MessagingError::Json(item)
    }
}

impl From<warp::Error> for MessagingError {
    fn from(item: warp::Error) -> Self {
        MessagingError::Web(item)
    }
}

#[derive(Serialize)]
struct ErrorMessage {
    pub code: u16,
    pub message: String,
}

type ToPublisherChannel = tokio::sync::mpsc::UnboundedSender<MessageToPublisher>;

struct PublisherManager {
    tracks: HashMap<Uuid, Vec<Arc<TrackLocalStaticRTP>>>,
    senders: HashMap<Uuid, ToPublisherChannel>,
}

impl PublisherManager {
    fn new() -> Self {
        PublisherManager {
            tracks: HashMap::new(),
            senders: HashMap::new(),
        }
    }

    fn add_publisher(&mut self, publisher_id: &Uuid, ch: ToPublisherChannel) {
        self.senders.insert(publisher_id.clone(), ch);
    }

    fn remove_publisher(&mut self, publisher_id: &Uuid) {
        self.tracks.remove(publisher_id);
        self.senders.remove(publisher_id);
    }

    fn add_track(&mut self, publisher_id: &Uuid, track: Arc<TrackLocalStaticRTP>) {
        let tracks = self
            .tracks
            .entry(publisher_id.clone())
            .or_insert(Vec::new());
        tracks.push(track);
    }

    fn has_both_audio_and_video(&self, publisher_id: &Uuid) -> bool {
        self.tracks
            .get(&publisher_id)
            .map(|t| t.len() == 2)
            .unwrap_or(false)
    }

    fn publisher_tracks_info(&self) -> (HashSet<String>, Vec<(Uuid, Arc<TrackLocalStaticRTP>)>) {
        let mut local_tracks = vec![];
        let mut local_track_ids = HashSet::new();
        for (pc_id, ts) in self.tracks.iter() {
            for local_track in ts {
                local_tracks.push((pc_id.clone(), Arc::clone(&local_track)));
                local_track_ids.insert(local_track.id().to_owned());
            }
        }
        (local_track_ids, local_tracks)
    }

    fn send(&self, pc_id: &Uuid, message: MessageToPublisher) {
        if let Some(sender) = self.senders.get(pc_id) {
            if let Err(e) = sender.send(message) {
                println_err(format!(
                    "Error while sending a message to {:?} {:?}",
                    pc_id, e
                ));
            }
        }
    }
}

type ToSubscriberChannel = tokio::sync::mpsc::UnboundedSender<SubscriberMessage>;

struct SubscriberManager {
    senders: HashMap<Uuid, ToSubscriberChannel>,
}

impl SubscriberManager {
    fn new() -> Self {
        SubscriberManager {
            senders: HashMap::new(),
        }
    }

    fn remove_subscriber(&mut self, subscriber_id: &Uuid) {
        self.senders.remove(subscriber_id);
    }

    fn add_subscriber(&mut self, subscriber_id: &Uuid, ch: ToSubscriberChannel) {
        self.senders.insert(subscriber_id.clone(), ch);
    }

    fn send(&self, message: SubscriberMessage) {
        for (sub_id, tx_ch) in self.senders.iter() {
            println!("Require renegotiation to subscriber {:?}", sub_id);

            if let Err(e) = tx_ch.send(SubscriberMessage {
                msg_type: message.msg_type,
                message: message.message.clone(),
            }) {
                println_err(format!(
                    "Error while sending a message to {:?} {:?}",
                    sub_id, e
                ));
            }
        }
    }
}

type PublisherManagerRef = Arc<Mutex<PublisherManager>>;
type SubscriberManagerRef = Arc<Mutex<SubscriberManager>>;

#[tokio::main]
async fn main() {
    let context = warp::path("app");
    let ws_context = warp::path("ws-app");
    let publisher_manager = Arc::new(Mutex::new(PublisherManager::new()));
    let subscriber_manager = Arc::new(Mutex::new(SubscriberManager::new()));

    let offer = context
        .and(warp::path("offer"))
        .and(warp::body::json::<RTCSessionDescription>())
        .and(with_publisher_manager(publisher_manager.clone()))
        .and(with_subscriber_manager(subscriber_manager.clone()))
        .and_then(handle_offer);

    let subscribe = ws_context
        .and(warp::path("subscribe"))
        .and(warp::ws())
        .and(with_publisher_manager(publisher_manager.clone()))
        .and(with_subscriber_manager(subscriber_manager.clone()))
        .map(
            |ws: warp::ws::Ws,
             publisher_manager: PublisherManagerRef,
             subscriber_manager: SubscriberManagerRef| {
                ws.on_upgrade(|websocket| {
                    handle_subscribe(websocket, publisher_manager, subscriber_manager)
                })
            },
        );

    let route = offer.or(subscribe).recover(handle_rejection);

    warp::serve(route).run(([127, 0, 0, 1], 9001)).await;
}

fn with_publisher_manager(
    publisher_manager: PublisherManagerRef,
) -> impl Filter<Extract = (PublisherManagerRef,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || publisher_manager.clone())
}

fn with_subscriber_manager(
    subscriber_manager: SubscriberManagerRef,
) -> impl Filter<Extract = (SubscriberManagerRef,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || subscriber_manager.clone())
}

async fn handle_offer(
    offer: RTCSessionDescription,
    publisher_manager: PublisherManagerRef,
    subscriber_manager: SubscriberManagerRef,
) -> Result<impl Reply, Rejection> {
    handle_offer_delegate(offer, publisher_manager, subscriber_manager)
        .await
        .map_err(|e| {
            println_err(format!("Error on handle_offer {:?}.", e));
            reject::custom(e)
        })
}

/// Handles an offer from a publisher.
///
async fn handle_offer_delegate(
    offer: RTCSessionDescription,
    publisher_manager: PublisherManagerRef,
    subscriber_manager: SubscriberManagerRef,
) -> Result<impl Reply, WebRTCRuntimeError> {
    let pc_id = Uuid::new_v4();
    let (tx_ch, rx_ch) = unbounded_channel();
    {
        let mut publisher_manager = publisher_manager.lock().unwrap();
        publisher_manager.add_publisher(&pc_id, tx_ch)
    }

    let mut rx_ch: UnboundedReceiverStream<MessageToPublisher> =
        UnboundedReceiverStream::new(rx_ch);

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

    peer_connection
        .on_track(Box::new(
            move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
                if let Some(track) = track {
                    println!("on_track {:?} on {:?}.", track.kind(), pc_id);

                    if track.kind() == RTPCodecType::Video {
                        let media_ssrc = track.ssrc();
                        let track_ssrc_tx = Arc::clone(&track_ssrc_tx);
                        tokio::spawn(async move {
                            if let Err(e) = track_ssrc_tx.send(media_ssrc).await {
                                println_err(format!("{:?} on {:?}.", e, pc_id));
                            }
                        });
                    }

                    let local_track_chan_tx2 = Arc::clone(&local_track_chan_tx);
                    tokio::spawn(async move {
                        let local_track = Arc::new(TrackLocalStaticRTP::new(
                            track.codec().await.capability,
                            format!("t-{:?}-{:?}", track.kind(), Uuid::new_v4()),
                            format!("s-{:?}", pc_id),
                        ));

                        let _ = local_track_chan_tx2.send(Arc::clone(&local_track)).await;

                        while let Ok((rtp, _)) = track.read_rtp().await {
                            if let Err(e) = local_track.write_rtp(&rtp).await {
                                if Error::ErrClosedPipe != e {
                                    println_err(format!(
                                        "output track write_rtp got error: {} and break on {:?}.",
                                        e, pc_id
                                    ));
                                    break;
                                } else {
                                    println_err(format!(
                                        "output track write_rtp got error: {} on {:?}.",
                                        e, pc_id
                                    ));
                                }
                            }
                        }
                    });
                }

                Box::pin(async {})
            },
        ))
        .await;

    let publisher_manager_for_state_change = publisher_manager.clone();
    let subscriber_manager_for_state_change = subscriber_manager.clone();
    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!("Peer connection state has changed to {} on {:?}.", s, pc_id);

            if s == RTCPeerConnectionState::Disconnected {
                {
                    let mut publisher_manager = publisher_manager_for_state_change.lock().unwrap();
                    publisher_manager.remove_publisher(&pc_id);
                }
                {
                    let subscriber_manager = subscriber_manager_for_state_change.lock().unwrap();
                    subscriber_manager.send(SubscriberMessage {
                        msg_type: SubscriberMessageType::Start,
                        message: String::from(""),
                    });
                }
            }

            Box::pin(async {})
        }))
        .await;

    peer_connection.set_remote_description(offer).await?;
    let answer = peer_connection.create_answer(None).await?;
    let mut gather_complete = peer_connection.gathering_complete_promise().await;
    peer_connection.set_local_description(answer).await?;

    let publisher_manager_for_track = publisher_manager.clone();
    let _ = gather_complete.recv().await;
    tokio::spawn(async move {
        loop {
            if let Some(track) = local_track_chan_rx.recv().await {
                let mut publisher_manager = publisher_manager_for_track.lock().unwrap();
                publisher_manager.add_track(&pc_id, track);
            }
            {
                let publisher_manager = publisher_manager_for_track.lock().unwrap();
                if publisher_manager.has_both_audio_and_video(&pc_id) {
                    let subscriber_manager = subscriber_manager.lock().unwrap();
                    subscriber_manager.send(SubscriberMessage {
                        msg_type: SubscriberMessageType::Start,
                        message: String::from(""),
                    });
                    println!("Both audio and video track are added to {:?}.", pc_id);
                    break;
                }
            }
        }
    });
    let rtcp_observer_pc = peer_connection.clone();

    tokio::spawn(async move {
        if let Some(ssrc) = track_ssrc_rx.recv().await {
            println!("SSRC {:?} detected on {:?}.", ssrc, pc_id);

            while let Some(msg) = rx_ch.next().await {
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
                                println_err(format!("{:?} on {:?}.", e, pc_id));
                            }
                        }
                    },
                }
            }
        }
    });
    if let Some(local_description) = peer_connection.local_description().await {
        Ok(ok_with_json(&local_description))
    } else {
        println_err(format!("generate local description failed on {:?}.", pc_id));
        Ok(internal_server_error_json())
    }
}

async fn handle_subscribe(
    ws: warp::ws::WebSocket,
    publisher_manager: PublisherManagerRef,
    subscriber_manager: SubscriberManagerRef,
) {
    if let Err(e) = handle_subscribe_delegate(ws, publisher_manager, subscriber_manager).await {
        println_err(format!("Error on handle_subscribe {:?}.", e));
    }
}

async fn handle_subscribe_delegate(
    ws: warp::ws::WebSocket,
    publisher_manager: PublisherManagerRef,
    subscriber_manager: SubscriberManagerRef,
) -> Result<(), WebRTCRuntimeError> {
    let subscriber_id = Uuid::new_v4();

    let (mut tx_ws, mut rx_ws) = ws.split();

    let (tx_ch, rx_ch) = unbounded_channel();
    {
        let mut subscriber_manager = subscriber_manager.lock().unwrap();
        subscriber_manager.add_subscriber(&subscriber_id, tx_ch.clone());
    }

    let mut rx_ch: UnboundedReceiverStream<SubscriberMessage> = UnboundedReceiverStream::new(rx_ch);

    let peer_connection = Arc::new(new_base_peer_connection().await?);

    let subscriber_manager_for_state_change = subscriber_manager.clone();
    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!(
                "Peer Connection State has changed to {} on {:?}.",
                s, subscriber_id
            );

            if s == RTCPeerConnectionState::Disconnected {
                let mut subscriber_manager = subscriber_manager_for_state_change.lock().unwrap();
                subscriber_manager.remove_subscriber(&subscriber_id);
            }

            Box::pin(async {})
        }))
        .await;
    tokio::spawn(async move {
        while let Some(msg) = rx_ch.next().await {
            match msg.msg_type {
                SubscriberMessageType::Answer => {
                    println!("Receive answer on {:?}.", subscriber_id);

                    let answer = match serde_json::from_str::<RTCSessionDescription>(&msg.message) {
                        Ok(a) => a,
                        Err(e) => {
                            println_err(format!("{:?} on {:?}", e, subscriber_id));
                            continue;
                        }
                    };
                    if let Err(e) = peer_connection.set_remote_description(answer).await {
                        println_err(format!("{:?} on {:?}", e, subscriber_id));
                        continue;
                    }
                }
                SubscriberMessageType::Start => {
                    println!("Create offer on {:?}.", subscriber_id);

                    let local_track_ids;
                    let local_tracks;

                    {
                        let publisher_manager = publisher_manager.lock().unwrap();
                        let (ids, tracks) = publisher_manager.publisher_tracks_info();
                        local_track_ids = ids;
                        local_tracks = tracks;
                    }

                    let mut existing_track_ids = HashSet::new();
                    let senders = peer_connection.get_senders().await;
                    for sender in senders {
                        if let Some(t) = sender.track().await {
                            let track_id = t.id().to_owned();
                            if !local_track_ids.contains(&track_id) {
                                println!("Remove track {:?} from {:?}", track_id, subscriber_id);
                                if let Err(e) = peer_connection.remove_track(&sender).await {
                                    println_err(format!(
                                        "Error while removing track {:?} {:?}.",
                                        track_id, e
                                    ));
                                }
                            }
                            existing_track_ids.insert(track_id);
                        }
                    }
                    println!(
                        "The number of publisher's tracks is {}. Existing track track_ids {:?} on {:?}.",
                        local_tracks.len(),
                        existing_track_ids,
                        subscriber_id
                    );

                    if local_tracks.len() == 0 {
                        println!("No publisher for {:?}", subscriber_id);
                        continue;
                    }

                    for (publisher_pc_id, local_track) in local_tracks {
                        let track_id = local_track.id();
                        if existing_track_ids.contains(track_id) {
                            println!(
                                "Track already exists {:?} on {:?}.",
                                track_id, subscriber_id
                            );
                            continue;
                        }
                        println!("Add track {:?} to {:?}.", track_id, subscriber_id);

                        if let Ok(rtp_sender) = peer_connection
                            .add_track(local_track as Arc<dyn TrackLocal + Send + Sync>)
                            .await
                        {
                            let publisher_manager_for_rtcp = publisher_manager.clone();
                            tokio::spawn(async move {
                                let mut rtcp_buf = vec![0u8; 1500];
                                while let Ok((n, _)) = rtp_sender.read(&mut rtcp_buf).await {
                                    let mut buf = &rtcp_buf[..n];
                                    // https://stackoverflow.com/questions/33687447/how-to-get-a-reference-to-a-concrete-type-from-a-trait-object
                                    if let Ok(packets) = webrtc::rtcp::packet::unmarshal(&mut buf) {
                                        for packet in packets {
                                            if let Some(pli_packet) = packet
                                                .as_any()
                                                .downcast_ref::<PictureLossIndication>(
                                            ) {
                                                println!("{:?} on {:?}", pli_packet, subscriber_id);
                                                let publisher_manager =
                                                    publisher_manager_for_rtcp.lock().unwrap();
                                                publisher_manager.send(
                                                    &publisher_pc_id,
                                                    MessageToPublisher::RTCP(RTCPToPublisher::PLI),
                                                );
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    }

                    let offer = match peer_connection.create_offer(None).await {
                        Ok(offer) => offer,
                        Err(e) => {
                            println_err(format!("{:?} on {:?}.", e, subscriber_id));
                            continue;
                        }
                    };

                    let mut gather_complete = peer_connection.gathering_complete_promise().await;

                    if let Err(e) = peer_connection.set_local_description(offer).await {
                        println_err(format!("{:?} on {:?}.", e, subscriber_id));
                        continue;
                    }
                    let _ = gather_complete.recv().await;
                    if let Some(local_description) = peer_connection.local_description().await {
                        let sdp_str = match serde_json::to_string(&local_description) {
                            Ok(s) => s,
                            Err(e) => {
                                println_err(format!("{:?} on {:?}.", e, subscriber_id));
                                continue;
                            }
                        };
                        let ret_message = match serde_json::to_string(&SubscriberMessage {
                            msg_type: SubscriberMessageType::Offer,
                            message: sdp_str,
                        }) {
                            Ok(s) => s,
                            Err(e) => {
                                println_err(format!("{:?} on {:?}.", e, subscriber_id));
                                continue;
                            }
                        };

                        if let Err(e) = tx_ws.send(Message::text(ret_message)).await {
                            println_err(format!("{:?} on {:?}.", e, subscriber_id));
                        }
                    } else {
                        println_err(format!(
                            "generate local_description failed on {:?}.",
                            subscriber_id
                        ));
                    }
                }
                SubscriberMessageType::Offer => {
                    println_err(format!(
                        "Receiving offers is currently not supported ({:?}).",
                        subscriber_id
                    ));
                    continue;
                }
            }
        }
    });

    while let Some(msg) = rx_ws.next().await {
        match msg.map_err(MessagingError::Web).and_then(|msg| {
            msg.to_str().map_err(MessagingError::Any).and_then(|s| {
                serde_json::from_str::<SubscriberMessage>(&s).map_err(MessagingError::Json)
            })
        }) {
            Ok(msg) => {
                if let Err(e) = tx_ch.send(msg) {
                    println_err(format!("{:?} on {:?}.", e, subscriber_id))
                }
            }
            Err(e) => println_err(format!("{:?} on {:?}.", e, subscriber_id)),
        }
    }

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

fn ok_with_json<T>(data: &T) -> warp::reply::WithStatus<warp::reply::Json>
where
    T: Serialize,
{
    warp::reply::with_status(warp::reply::json(data), StatusCode::OK)
}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    println_err(format!("handle_rejection {:?}", err));
    Ok(warp::reply::with_status(
        "Internal Server Error",
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

fn internal_server_error_json() -> warp::reply::WithStatus<warp::reply::Json> {
    warp::reply::with_status(
        warp::reply::json(&ErrorMessage {
            code: 500,
            message: String::from("Internal Server Error"),
        }),
        StatusCode::INTERNAL_SERVER_ERROR,
    )
}

fn println_err(e: String) {
    eprintln!("\x1b[91m{}\x1b[0m", e);
}

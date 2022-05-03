use std::sync::{Arc, Mutex};
use warp::{http::StatusCode, reject, reply, reply::Reply, Filter, Rejection};

use std::collections::HashMap;
use std::collections::VecDeque;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug)]
struct InvalidParameter {
    message: String,
}

impl reject::Reject for InvalidParameter {}

struct MessageBuffer {
    buf: VecDeque<String>,
    notification_chan: Option<mpsc::Sender<()>>,
}

type MessageHub = Arc<Mutex<HashMap<Uuid, MessageBuffer>>>;

#[tokio::main]
async fn main() {
    let message_hub: MessageHub = Arc::new(Mutex::new(HashMap::new()));

    let join = warp::path("join")
        .and(with_message_hub(message_hub.clone()))
        .and_then(join);

    let chat = warp::path("chat")
        .and(warp::query::<HashMap<String, String>>())
        .and(with_message_hub(message_hub.clone()))
        .and_then(chat);

    let send = warp::path("send")
        .and(warp::post())
        .and(warp::query::<HashMap<String, String>>())
        .and(with_message_hub(message_hub.clone()))
        .and(warp::body::bytes())
        .and_then(send);

    warp::serve(join.or(chat.or(send)).recover(handle_rejection))
        .run(([127, 0, 0, 1], 8080))
        .await;
}

async fn join(message_hub: MessageHub) -> Result<impl Reply, Rejection> {
    let client_id = Uuid::new_v4();

    let mut message_hub = message_hub.lock().unwrap();
    {
        message_hub.insert(
            client_id.clone(),
            MessageBuffer {
                buf: VecDeque::new(),
                notification_chan: None,
            },
        );
    }

    Ok(client_id.to_string())
}

async fn chat(
    queries: HashMap<String, String>,
    message_hub: MessageHub,
) -> Result<impl Reply, Rejection> {
    let client_id = match get_client_id(queries) {
        Ok(id) => id,
        Err(e) => return Err(e),
    };

    let (tx_for_notification, mut rx_for_notification) = mpsc::channel(1);
    {
        let mut message_hub = message_hub.lock().unwrap();
        if let Some(message_buf) = message_hub.get_mut(&client_id) {
            if let Some(msg) = message_buf.buf.pop_front() {
                message_buf.notification_chan = None;
                return Ok(msg);
            }

            message_buf.notification_chan = Some(tx_for_notification);
        } else {
            return Err(reject::custom(InvalidParameter {
                message: "A client mapped to the client_id doesn't exist.".to_owned(),
            }));
        }
    }

    let (tx_for_timer, rx_for_timer) = oneshot::channel();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        if let Err(_) = tx_for_timer.send(()) {
            // println!("Error on stopping timer {:?}", e)
            // A message has already been sent.
        }
    });

    let msg = tokio::select! {
        _ = rx_for_notification.recv() => {
            let mut message_hub = message_hub.lock().unwrap();
            if let Some(message_buf) = message_hub.get_mut(&client_id) {
                let ret = message_buf.buf.pop_front();
                message_buf.notification_chan = None;
                ret
            } else {
                None
            }
        },
        _ = rx_for_timer => {
            let mut message_hub = message_hub.lock().unwrap();
            if let Some(message_buf) = message_hub.get_mut(&client_id) {
                message_buf.notification_chan = None;
            }
            None
        }
    };

    Ok(msg.unwrap_or("".to_owned()))
}

async fn send(
    queries: HashMap<String, String>,
    message_hub: MessageHub,
    body: bytes::Bytes,
) -> Result<impl Reply, Rejection> {
    let client_id = match get_client_id(queries) {
        Ok(id) => id,
        Err(e) => return Err(e),
    };

    let mut body = String::from_utf8_lossy(&body[..]);
    let body = body.to_mut().to_owned();

    let mut senders = vec![];
    {
        let mut message_hub = message_hub.lock().unwrap();
        for (id, message_buf) in message_hub.iter_mut() {
            if id == &client_id {
                continue;
            }

            message_buf.buf.push_back(body.to_owned());
            if let Some(ch) = &message_buf.notification_chan {
                senders.push(ch.clone());
            }
        }
    }

    for sender in senders {
        if let Err(e) = sender.send(()).await {
            println!("{:?}", e);
        }
    }

    Ok("The message has been sent.".to_owned())
}

fn get_client_id(queries: HashMap<String, String>) -> Result<Uuid, Rejection> {
    let client_id = if let Some(client_id) = queries.get("client_id") {
        client_id
    } else {
        return Err(reject::custom(InvalidParameter {
            message: "client_id isn't specified.".to_owned(),
        }));
    };

    let client_id = if let Ok(client_id) = Uuid::parse_str(client_id) {
        client_id
    } else {
        return Err(reject::custom(InvalidParameter {
            message: "Invalid client_id format.".to_owned(),
        }));
    };

    Ok(client_id)
}

fn with_message_hub(
    message_hub: MessageHub,
) -> impl Filter<Extract = (MessageHub,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || message_hub.clone())
}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    if err.is_not_found() {
        Ok(reply::with_status(
            "NOT_FOUND".to_owned(),
            StatusCode::NOT_FOUND,
        ))
    } else if let Some(e) = err.find::<InvalidParameter>() {
        Ok(reply::with_status(
            format!("BAD_REQUEST ({})", e.message),
            StatusCode::BAD_REQUEST,
        ))
    } else {
        eprintln!("unhandled rejection: {:?}", err);
        Ok(reply::with_status(
            "INTERNAL_SERVER_ERROR".to_owned(),
            StatusCode::INTERNAL_SERVER_ERROR,
        ))
    }
}

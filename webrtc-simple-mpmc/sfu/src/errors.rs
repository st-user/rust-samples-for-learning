
use warp::{reject, ws::Message};
use serde::Serialize;

type WsSendError = tokio::sync::mpsc::error::SendError<Message>;

#[derive(Debug)]
pub enum ApplicationError {
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
pub struct ErrorMessage {
    pub code: u16,
    pub message: String,
}

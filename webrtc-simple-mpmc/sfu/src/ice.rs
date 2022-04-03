use base64::encode;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha1::Sha1;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type HmacSha1 = Hmac<Sha1>;

use webrtc::ice_transport::ice_credential_type::RTCIceCredentialType;
use webrtc::ice_transport::ice_server::RTCIceServer;

use log::info;
use std::env;

const TURN_AUTH_EXPIRATION_HOURS_DEFAULT: u64 = 3;

#[derive(Serialize)]
pub struct ClientRTCIceServer {
	urls: Vec<String>,
	username: Option<String>,
	credential: Option<String>,
	#[serde(rename = "credentialType")]
	credential_type: String,
}

pub fn create_ice_server_config_for_browser(name: &str) -> Vec<ClientRTCIceServer> {
	let ice_servers = create_ice_server_config(name);
	let mut ret = vec![];

	for ice_server in ice_servers {
		let username = if ice_server.username.len() == 0 {
			None
		} else {
			Some(ice_server.username)
		};

		let credential = if ice_server.credential.len() == 0 {
			None
		} else {
			Some(ice_server.credential)
		};

		let credential_type = match ice_server.credential_type {
			RTCIceCredentialType::Oauth => "oauth".to_owned(),
			_ => "password".to_owned(),
		};

		ret.push(ClientRTCIceServer {
			urls: ice_server.urls,
			username,
			credential,
			credential_type,
		});
	}

	ret
}

pub fn create_ice_server_config(name: &str) -> Vec<RTCIceServer> {
	let mut ret = vec![];

	let stun_url = match env::var("STUN_URL") {
		Ok(s) => s,
		Err(_) => {
			info!("STUN_URL is not specified so use the default value.");
			"stun.l.google.com:19302".to_owned()
		}
	};
	ret.push(RTCIceServer {
		urls: vec![format!("stun:{}", stun_url)],
		..Default::default()
	});

	let expires_after = env::var("TURN_AUTH_EXPIRATION_HOURS")
		.map(|h| h.parse().unwrap_or(TURN_AUTH_EXPIRATION_HOURS_DEFAULT))
		.unwrap_or(TURN_AUTH_EXPIRATION_HOURS_DEFAULT);

	if let (Ok(turn_url), Ok(turn_auth)) = (env::var("TURN_URL"), env::var("TURN_AUTH")) {
		let (username, credential) =
			generate_coturn_credential(&turn_auth, name, Duration::from_secs(expires_after * 3600));
		ret.push(RTCIceServer {
			urls: vec![format!("turn:{}", turn_url)],
			username,
			credential,
			credential_type: RTCIceCredentialType::Password,
			..Default::default()
		});
	} else {
		info!("TURN_URL and TURN_AUTH are not specified.");
	}
	ret
}

fn _generate_coturn_credential(
	secret: &str,
	name: &str,
	current_time_from_epoch: Duration,
	expires_after: Duration,
) -> (String, String) {
	let timestamp = current_time_from_epoch + expires_after;
	let timestamp = timestamp.as_secs();

	let username = format!("{}:{}", timestamp, name);
	let mut mac = HmacSha1::new_from_slice(secret.as_bytes()).unwrap();
	mac.update(username.as_bytes());
	let result = mac.finalize();
	let code_bytes = result.into_bytes();

	let credential = encode(code_bytes);

	(username, credential)
}

fn generate_coturn_credential(
	secret: &str,
	name: &str,
	expires_after: Duration,
) -> (String, String) {
	let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

	_generate_coturn_credential(secret, name, current_time, expires_after)
}

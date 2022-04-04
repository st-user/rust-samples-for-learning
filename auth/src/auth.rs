use crate::errors::ApplicationError;
use actix_web::dev::ServiceRequest;
use alcoholic_jwt::{token_kid, validate, Validation, JWKS};
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
	sub: String,
	company: String,
	exp: usize,
}

use actix_web_httpauth::extractors::bearer::{BearerAuth, Config};
use actix_web_httpauth::extractors::AuthenticationError;

use log::error;
use std::env;

pub fn check_env() {
	env::var("AUTHORITY").expect("AUTHORITY must be specified.");
	env::var("AUDIENCE").expect("AUDIENCE must be specified.");
}

pub async fn validator(
	req: ServiceRequest,
	credentials: BearerAuth,
) -> Result<ServiceRequest, actix_web::Error> {
	let config = req
		.app_data::<Config>()
		.map(|data| data.clone())
		.unwrap_or_else(Default::default);
	match validate_token(credentials.token()).await {
		Ok(res) => {
			if res {
				Ok(req)
			} else {
				Err(AuthenticationError::from(config).into())
			}
		}
		Err(e) => {
			error!("{:?}", e);
			Err(AuthenticationError::from(config).into())
		}
	}
}

async fn validate_token(token: &str) -> Result<bool, ApplicationError> {
	let authority = env::var("AUTHORITY").unwrap();

	let jwks = match fetch_jwks(&format!("{}{}", authority, ".well-known/jwks.json")).await {
		Ok(jwks) => jwks,
		Err(e) => {
			error!("{:?}", e);
			return Err(ApplicationError::JWKSFetchError);
		}
	};

	let audience = env::var("AUDIENCE").unwrap();

	let validations = vec![
		Validation::Issuer(authority),
		Validation::SubjectPresent,
		Validation::NotExpired,
		Validation::Audience(audience.to_owned()),
	];

	let kid = match token_kid(&token) {
		Ok(res) => {
			if let Some(res) = res {
				res
			} else {
				error!("Failed to decode kid.");
				return Err(ApplicationError::JWKSFetchError);
			}
		}
		Err(e) => {
			error!("{:?}", e);
			return Err(ApplicationError::JWKSFetchError);
		}
	};
	let jwk = if let Some(jwk) = jwks.find(&kid) {
		jwk
	} else {
		error!("Specified key not found in set");
		return Err(ApplicationError::JWKSFetchError);
	};

	Ok(validate(token, jwk, validations).is_ok())
}

async fn fetch_jwks(uri: &str) -> Result<JWKS, Box<dyn Error>> {
	Ok(reqwest::get(uri).await?.json::<JWKS>().await?)
}

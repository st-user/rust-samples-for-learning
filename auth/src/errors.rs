use actix_web::{
    error,
    http::{header::ContentType, StatusCode},
    HttpResponse,
};

use log::error;

#[derive(Debug)]
pub enum ApplicationError {
    DBPool(mobc::Error<tokio_postgres::Error>),
    DB(tokio_postgres::Error),
    InputCheck(String),
    Message(String),
    MessageAndStatus(String, u16),
    JWKSFetchError,
}

impl From<mobc::Error<tokio_postgres::Error>> for ApplicationError {
    fn from(item: mobc::Error<tokio_postgres::Error>) -> ApplicationError {
        ApplicationError::DBPool(item)
    }
}

impl From<tokio_postgres::Error> for ApplicationError {
    fn from(item: tokio_postgres::Error) -> ApplicationError {
        ApplicationError::DB(item)
    }
}

impl std::fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:?})", self)
    }
}

impl error::ResponseError for ApplicationError {
    fn error_response(&self) -> HttpResponse {
        error!("{:?}", self);

        let message = match self {
            ApplicationError::InputCheck(m) => m,
            ApplicationError::MessageAndStatus(m, _) => m,
            _ => "Internal Server Error",
        };

        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(format!(
                "{{
                \"message\": \"{}\"
            }}",
                message
            ))
    }

    fn status_code(&self) -> StatusCode {
        match self {
            ApplicationError::InputCheck(_) => StatusCode::BAD_REQUEST,
            ApplicationError::MessageAndStatus(_, s) => {
                StatusCode::from_u16(*s).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

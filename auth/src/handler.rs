use actix_web::{web, Responder};

use serde::{Deserialize, Serialize};
use std::convert::From;
use tokio_postgres::Row;

use base64::encode;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;

use log::error;

use crate::errors::ApplicationError;

#[derive(Serialize)]
struct Room {
    room_id: i64,
    room_name: String,
    secret_token: String,
}

#[derive(Serialize)]
struct Member {
    member_id: i64,
    room_id: i64,
    member_name: String,
    secret_token: String,
}

#[derive(Deserialize)]
pub struct RoomBody {
    room_name: String,
}

#[derive(Deserialize)]
pub struct MemberBody {
    room_id: i64,
    room_secret_token: String,
    member_name: String,
}

const NAME_MAX_LENGTH: usize = 100;

type DBPool = mobc::Pool<mobc_postgres::PgConnectionManager<tokio_postgres::NoTls>>;

pub async fn room(db_pool: web::Data<DBPool>, room_body: web::Json<RoomBody>) -> impl Responder {
    
    check_length(&room_body.room_name, "room_name", NAME_MAX_LENGTH)?;

    room_delegate(db_pool, &room_body.room_name)
        .await
        .map(web::Json)
}

pub async fn member(
    db_pool: web::Data<DBPool>,
    member_body: web::Json<MemberBody>,
) -> impl Responder {
    check_length(&member_body.member_name, "member_name", NAME_MAX_LENGTH)?;

    member_delegate(db_pool, member_body.into_inner())
        .await
        .map(web::Json)
}

async fn room_delegate(
    db_pool: web::Data<DBPool>,
    room_name: &String,
) -> Result<Room, ApplicationError> {
    let client = db_pool.get().await?;

    let sql = "
        INSERT
            INTO 
                myappsch.rooms (
                    room_name, secret_token
                )
            VALUES
                (
                    $1, $2
                )
            RETURNING *
    ";

    let result = client
        .query(sql, &[&room_name, &generate_secret_token()])
        .await?;

    if let Some(row) = result.get(0) {
        Ok(row_to_room(row))
    } else {
        Err(ApplicationError::Message(String::from(
            "Unexpectedly 'RETURNING *' didn't return a row.",
        )))
    }
}

fn row_to_room(row: &Row) -> Room {
    Room {
        room_id: row.get(0),
        room_name: row.get(1),
        secret_token: row.get(2),
    }
}

async fn member_delegate(
    db_pool: web::Data<DBPool>,
    member_body: MemberBody,
) -> Result<Member, ApplicationError> {
    let mut client = db_pool.get().await?;
    let trans = client.transaction().await?;

    let sql = "
        SELECT
            *
            FROM
                myappsch.rooms
            WHERE
                room_id = $1 and secret_token = $2
            FOR UPDATE
    ";

    let _ = match trans
        .query_one(sql, &[&member_body.room_id, &member_body.room_secret_token])
        .await
    {
        Ok(room) => room,
        Err(e) => {
            error!("{:?}", e);
            return Err(ApplicationError::MessageAndStatus(
                "Room doesn't exist.".to_owned(),
                404,
            ));
        }
    };

    let sql = "
        INSERT
            INTO
                myappsch.members (
                    room_id, member_name, secret_token
                ) VALUES (
                    $1, $2, $3
                )
            RETURNING *
    ";

    match trans
        .query_one(
            sql,
            &[
                &member_body.room_id,
                &member_body.member_name,
                &generate_secret_token(),
            ],
        )
        .await
    {
        Ok(member) => {
            trans.commit().await?;
            Ok(row_to_member(&member))
        }
        Err(e) => {
            error!("{:?}", e);
            Err(ApplicationError::Message(String::from(
                "Unexpectedly 'RETURNING *' didn't return a row.",
            )))
        }
    }
}

fn row_to_member(row: &Row) -> Member {
    Member {
        member_id: row.get(0),
        room_id: row.get(1),
        member_name: row.get(2),
        secret_token: row.get(3),
    }
}

fn generate_secret_token() -> String {
    let mut bs = [0u8; 32];
    let mut rng = ChaCha20Rng::from_entropy();
    rng.fill_bytes(&mut bs);
    encode(bs)
}

fn check_length(name: &str, field_name: &str, max_length: usize) -> Result<(), ApplicationError> {
    let error_message = if name.len() == 0 {
        format!("{} is empty.", field_name)
    } else if name.len() > max_length {
        format!(
            "The length of {} must be no more than {} characters.",
            field_name, max_length
        )
    } else {
        return Ok(());
    };
    Err(ApplicationError::InputCheck(error_message))
}

use actix_web::{web, App, HttpServer};

use mobc::Pool;
use mobc_postgres::PgConnectionManager;
use std::str::FromStr;
use tokio_postgres::{Config, NoTls};

use actix_web_httpauth::middleware::HttpAuthentication;

use dotenv::dotenv;
use std::env;

mod auth;
mod errors;
mod handler;
mod logger;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    logger::init_logger();

    let db_url = env::var("DB_URL").expect("DB_URL must be specified.");
    auth::check_env();

    let config = Config::from_str(&db_url).unwrap();
    let connection_manager = PgConnectionManager::new(config, NoTls);

    let pool = Pool::builder().max_open(20).build(connection_manager);

    HttpServer::new(move || {
        let auth = HttpAuthentication::bearer(auth::validator);
        App::new()
            .service(
                web::scope("/room")
                    .wrap(auth)
                    .app_data(web::Data::new(pool.clone()))
                    .route("", web::post().to(handler::room)),
            )
            .service(
                web::scope("/member")
                    .app_data(web::Data::new(pool.clone()))
                    .route("", web::post().to(handler::member)),
            )
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

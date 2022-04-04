# API endpoints with JWT authentication

Simple API endpoints with JWT authentication.

# Installation

```
cd auth
mv sample.env .env
```

Set `AUTHORITY` and `AUDIENCE` according to your settings.

# Run

```
cd auth/postgres
docker build -t auth_postgres .
docker run -it -d -p 5555:5432 --name auth_postgres auth_postgres
cd ../
cargo run

```

# endpoints

TBD

# References

 - [Build an API in Rust with JWT Authentication](https://auth0.com/blog/build-an-api-in-rust-with-jwt-authentication-using-actix-web/)
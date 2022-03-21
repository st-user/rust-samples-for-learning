# crud

Simple web applications demonstrate CRUD(Create, Read, Update, Delete) operations.

## Simple app

`src/bin/simple.rs` doesn't require external resources(e.g. relational database). 
This application uses in-memory `HashMap` as a storage.

### Run

```
cargo run --bin simple
```

## Postgres app

`src/bin/with_postgres.rs` accesses postgres with [tokio-postgres](https://crates.io/crates/tokio-postgres).

### Run

`src/bin/with_postgres.rs` requires postgres running on the localhost.

``` bash
# We uses docker in this example.
docker run -it -d -p 5555:5432 --name my-postgres -e POSTGRES_PASSWORD=postgres postgres

cargo run --bin with_postgres
```

## Endpoints

``` bash
curl -X GET http://localhost:8080/employees

# [{"id":1,"name":"Bob","role":"Admin"}]
```

``` bash
ID=1
curl -X GET http://localhost:8080/employees/${ID}

# {"id":1,"name":"Bob","role":"Admin"}
```

``` bash
curl -X POST http://localhost:8080/employees \
     -H 'Content-Type: application/json' \
     -d '{ "name": "Bob", "role": "Admin" }'

# {"id":1,"name":"Bob","role":"Admin"}
```

``` bash
ID=1
curl -X PUT http://localhost:8080/employees/${ID} \
     -H 'Content-Type: application/json' \
     -d '{ "name": "Bob", "role": "SuperAdmin" }'

# {"id":1,"name":"Bob","role":"SuperAdmin"}
```

``` bash
ID=1
curl -X DELETE http://localhost:8080/employees/${ID}

# {"id":1,"name":"Bob","role":"Admin"}
```

## References

 - [Create an async CRUD web service in Rust with warp - LogRocket](https://blog.logrocket.com/create-an-async-crud-web-service-in-rust-with-warp/)



# chat

Chat applications utilizing the asynchronous features in Rust.


## TCP

`src/bin/chat_tcp.rs` utilizes a raw TCP transport provided by [tokio](https://github.com/tokio-rs/tokio).

### Run

```
cargo run --bin chat_tcp
```

### Usage

We uses `telnet` in the following example.

``` bash
$ telnet localhost 8080

# First, enter a room with '@' keyword.
@hello_room
Hello World! # Once you have entered a room, you can send messages.
Hey!
@another_room # You can change the room with `@` keyword.

```


## Websocket

`src/bin/chat_ws.rs` utilizes Websocket protocol with [warp](https://github.com/seanmonstar/warp).

### Run

```
cargo run --bin chat_ws
```

### Usage

We use uses [wscat](https://www.npmjs.com/package/wscat) in the following example .

``` bash
$ wscat -c ws://localhost:8080/chat

# First, enter a room with '@' keyword.
@hello_room
Hello World! # Once you have entered a room, you can send messages.
Hey!
@another_room # You can change the room with `@` keyword.

```

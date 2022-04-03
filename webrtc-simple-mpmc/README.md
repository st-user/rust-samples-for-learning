# webrtc-simple-mpmc

A WebRTC video streaming application utilizing [WebRTC.rs](https://github.com/webrtc-rs/webrtc).

Each producer(publisher) can broadcast its video/audio to multiple consumers(subscribers).


## Installation

``` bash
cd client
npm install
```

## Run the application

### server(Rust)

``` bash
cd sfu
mv sample.env .env
cargo run

```

### client(javascript)

``` bash
cd client
npm run start
```

After you run both server and client successfully, An browser window should open and access `http://localhost:9000`.
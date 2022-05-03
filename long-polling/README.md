# long-polling

A simple chat application using long-polling.

## Run

``` bash
cargo run
```

## Subscribe messages

``` bash
chmod +x client.sh
./client.sh
```


## Send messages

``` bash
CLIENT_ID=`curl http://localhost:8080/join 2>/dev/null`
curl -X POST -d 'Hello World!' "http://localhost:8080/send?client_id=${CLIENT_ID}"
```
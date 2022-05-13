use std::collections::HashMap;
use std::cmp::Eq;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::Mutex;

use std::marker::Send;
use std::marker::Sync;
use std::future::Future;
use std::pin::Pin;

type HandlerPtr<T> = Box<
    dyn (FnMut(T) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>)
        + Send
        + Sync
>;

type HandlerRef<T> = Arc<Mutex<HandlerPtr<T>>>;

pub struct EventEmitterInternal<E: Hash + Eq, T> {
    handlers: Mutex<HashMap<E, Vec<HandlerRef<T>>>>
}

pub struct EventEmitter<E: Hash + Eq, T> {
    internal: EventEmitterInternal<E, T>
}

impl<E: Hash + Eq, T: Clone> EventEmitter<E, T> {
    pub fn new() -> Self {
        Self {
            internal: EventEmitterInternal {
                handlers: Mutex::new(HashMap::new())
            }
        }
    }

    pub async fn on(&self, event: E, handler: HandlerPtr<T>)
    {   
        let mut handlers = self.internal.handlers.lock().await;

        let event_handlers = handlers.entry(event).or_insert_with(|| 
            vec![]
        );

        event_handlers.push(Arc::new(Mutex::new(handler)));
    }

    pub async fn emit(&self, event: &E, payload: T) {

        let handlers = self.internal.handlers.lock().await;

        if let Some(handlers) = handlers.get(event) {
            for handler in handlers.into_iter() {
                let mut f = handler.lock().await;
                f(payload.clone()).await;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    
    let emitter = Arc::new(EventEmitter::new());

    let em1 = emitter.clone();
    let h1 = tokio::spawn(async move {

        let events = vec!["click".to_owned(), "change".to_owned()];
        for e in events {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            em1.emit(&e, "Hello World!".to_owned()).await;
        }

    });

    let em2 = emitter.clone();
    let h2 = tokio::spawn(async move {
        em2.on("click".to_owned(), Box::new(move |name: String| {

            println!("Click 1 {}", name);
    
            Box::pin(async {})
        })).await;

        em2.on("click".to_owned(), Box::new(move |name: String| {

            println!("Click 2 {}", name);
    
            Box::pin(async {})
        })).await;
    });

    let em3 = emitter.clone();
    let h3 = tokio::spawn(async move {
        em3.on("change".to_owned(), Box::new(move |name: String| {

            println!("Change {}", name);
    
            Box::pin(async {})
        })).await;
    });
    
    h1.await.unwrap();
    h2.await.unwrap();
    h3.await.unwrap();
}

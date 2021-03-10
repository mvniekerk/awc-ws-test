use std::time::Duration;
use actix::io::SinkWrite;
use actix::*;
use actix_codec::Framed;
use awc::{
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
    BoxedSocket,
};
use bytes::Bytes;
use futures::stream::{SplitSink, StreamExt};

struct WsClient(
    SinkWrite<Message, SplitSink<Framed<BoxedSocket, Codec>, Message>>,
);

#[derive(Message)]
#[rtype(result = "()")]
struct ClientCommand(String);

impl Actor for WsClient {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // start heartbeats otherwise server will disconnect after 10 seconds
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
        println!("Disconnected");

        // Stop application on disconnect
        System::current().stop();
    }
}

impl WsClient {
    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(1, 0), |act, ctx| {
            act.0.write(Message::Ping(Bytes::from_static(b"")));
            act.hb(ctx);
        });
    }
}

/// Handle stdin commands
impl Handler<ClientCommand> for WsClient {
    type Result = ();

    fn handle(&mut self, msg: ClientCommand, _ctx: &mut Context<Self>) {
        self.0.write(Message::Text(msg.0.into()));
    }
}

/// Handle server websocket messages
impl StreamHandler<Result<Frame, WsProtocolError>> for WsClient {
    fn handle(&mut self, msg: Result<Frame, WsProtocolError>, _: &mut Context<Self>) {
        if let Ok(Frame::Text(txt)) = msg {
            if let Ok(txt) = String::from_utf8(txt.to_vec()) {
                println!("Got text {:}", txt);
            }
        }
    }

    fn started(&mut self, _ctx: &mut Context<Self>) {
        println!("Connected");
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        println!("Server disconnected");
        ctx.stop()
    }
}

impl actix::io::WriteHandler<WsProtocolError> for WsClient {}


async fn do_some_arbitrary_things() {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        println!("Hello from tokio task");
    }
}

#[tokio::main]
pub async fn main() {
    println!("Hello, world!");
    std::thread::spawn(|| { start_ws() });

    let jh = tokio::spawn(do_some_arbitrary_things());
    jh.await;
}

pub fn start_ws() {
    let jwt = std::env::var("JWT").expect("JWT must be set");
    let url = std::env::var("URL").expect("URL must be set");

    actix_rt::System::new().block_on(async {
        loop {
            println!("WS URL: {:} {:}", url, jwt);

            let ssl = {
                let mut ssl =
                    openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls()).unwrap();
                let _ = ssl.set_alpn_protos(b"\x08http/1.1");
                ssl.build()
            };

            let connector = awc::Connector::new()
                .timeout(std::time::Duration::from_secs_f64(300f64))
                .ssl(ssl);

            let cb = awc::ClientBuilder::new()
                .timeout(std::time::Duration::from_secs_f64(60f64))
                .connector(connector)
                .finish()
                .ws(&url)
                .bearer_auth(&jwt)
                .connect()
                .await
                .map_err(|e| {
                    eprintln!("######### WS Error: {:?}", e);
                    panic!(format!("Error: {:?}", e));
                    ()
                })
                .map(|(_response, framed)| {
                    let (sink, stream) = framed.split();
                    let addr = WsClient::create(|ctx| {
                        WsClient::add_stream(stream, ctx);
                        WsClient(
                            SinkWrite::new(sink, ctx),
                        )
                    });
                    addr.do_send(ClientCommand("START-SEND-EVENTS".to_string()));
                });

            if cb.is_err() {
                eprintln!("Error on CB");
                break;
            }
        }
    });

}

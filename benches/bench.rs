
extern crate futures;
extern crate hyper;
extern crate tokio;
extern crate bytes;

#[macro_use]
extern crate bencher;

#[macro_use]
extern crate lazy_static;

use std::net::SocketAddr;
use bencher::Bencher;

use futures::{Future, Stream};
use tokio::runtime::current_thread::Runtime;
use hyper::{Body, Method, Request, Response, Server};
use hyper::client::HttpConnector;

use bytes::{Bytes, BytesMut};

lazy_static! {
    static ref buffer : Bytes = { let mut b = BytesMut::with_capacity(1024 * 1024 * 100);
                                  b.resize(1024 * 1024 * 100, b'z'); 
                                  b.freeze() };
}

fn http1_get_parallel(b: &mut Bencher) {
    opts()
        .parallel(1)
        .bench(b)
}

benchmark_main!(benches);
benchmark_group!(benches, http1_get_parallel);

// ==== Benchmark Options =====

struct Opts {
    http2: bool,
    parallel_cnt: u32,
    request_method: Method,
    request_body: Option<&'static [u8]>,
    response_body: Bytes,
}

fn opts() -> Opts {
    Opts {
        http2: false,
        parallel_cnt: 1,
        request_method: Method::GET,
        request_body: None,
        response_body: buffer.clone(),
    }
}

impl Opts {

    fn parallel(mut self, cnt: u32) -> Self {
        assert!(cnt > 0, "parallel count must be larger than 0");
        self.parallel_cnt = cnt;
        self
    }

    fn bench(self, b: &mut Bencher) {
        let mut rt = Runtime::new().unwrap();
        let addr = spawn_hello(&mut rt, self.response_body.clone());

        let connector = HttpConnector::new(1);
        let client = hyper::Client::builder()
            .http2_only(self.http2)
            .build::<_, Body>(connector);


        b.bytes = buffer.len() as u64;
        if self.parallel_cnt == 1 {
            let url: hyper::Uri = format!("http://{}/hello", addr).parse().unwrap();

            let make_request = || {
                let body = self
                    .request_body
                    .map(Body::from)
                    .unwrap_or_else(|| Body::empty());
                let mut req = Request::new(body);
                *req.method_mut() = self.request_method.clone();
                req
            };
            b.iter(move || {
                let mut req = make_request();
                *req.uri_mut() = url.clone();
                rt.block_on(client.request(req).and_then(|res| {
                    res.into_body()
                       .map(|chunk| {
                            chunk.as_ref().to_vec()
                        })
                       .for_each(|v| { bencher::black_box(v); Ok(())})
                })).expect("client wait");
            });
        } else {
            let url: hyper::Uri = format!("http://{}/hello", addr).parse().unwrap();

            let make_request = || {
                let body = self
                    .request_body
                    .map(Body::from)
                    .unwrap_or_else(|| Body::empty());
                let mut req = Request::new(body);
                *req.method_mut() = self.request_method.clone();
                req
            };
            b.iter(|| {
                let futs = (0..self.parallel_cnt)
                    .into_iter()
                    .map(|_| {
                        let mut req = make_request();
                        *req.uri_mut() = url.clone();
                        client.request(req).and_then(|res| {
                            res.into_body()
                               .map(|chunk| {
                                    chunk.as_ref().to_vec()
                                })
                               .for_each(|v| { bencher::black_box(v); Ok(())})
                        }).map_err(|e| panic!("client error: {}", e))
                    });
                let _ = rt.block_on(::futures::future::join_all(futs));
            });
        }
    }
}

fn spawn_hello(rt: &mut Runtime, body: Bytes) -> SocketAddr {
    use hyper::service::{service_fn};
    let addr = "127.0.0.1:0".parse().unwrap();
    let srv = Server::bind(&addr)
        .serve(move || {
            let bdy = body.clone();
            service_fn(move |req: Request<Body>| {
                let b = bdy.clone();
                req
                    .into_body()
                    .for_each(|_chunk| {
                        Ok(())
                    })
                    .map(move |_| {
                        Response::new(Body::from(b))
                    })
            })
        });
    let addr = srv.local_addr();
    let fut = srv.map_err(|err| panic!("server error: {}", err));
    rt.spawn(fut);
    return addr
}



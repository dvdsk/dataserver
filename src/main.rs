//mod websocket;
mod httpserver;
use httpserver::pause;

fn main() {
    //https://www.deviousd.duckdns.org:8080/index.html
    let handle = httpserver::start();
    pause();
    httpserver::stop(handle);
}

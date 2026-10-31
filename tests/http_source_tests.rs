use bfs::{acquisition::SourceConfig, http_source::parse_links};
#[test]
fn links_are_resolved_deduplicated_and_titled() {
    let config = SourceConfig {
        page: "http://localhost/index".into(),
        ..Default::default()
    };
    let result = parse_links(&config, &config.page, r#"<a href='/a.ZIP#one'>Linux &amp; tools</a><a href='/a.ZIP#two'>Duplicate</a><a href='other.txt'>No</a><a href='http://elsewhere/b.zip'>Other</a>"#).unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].title, "Linux & tools");
    assert_eq!(result.candidates[0].url, "http://localhost/a.ZIP");
}

#[test]
fn discovery_reads_local_http_page_and_limits_redirect_requests() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::atomic::AtomicBool,
    };
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let page = format!("http://{}/", server.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let mut request = [0; 2048];
        stream.read(&mut request).unwrap();
        let body = "<a href='/a.zip'>Fixture</a>";
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let source = SourceConfig {
        page,
        ..Default::default()
    };
    let result =
        bfs::http_source::discover(&source, &Default::default(), &AtomicBool::new(false)).unwrap();
    assert_eq!(result.candidates[0].title, "Fixture");
    handle.join().unwrap();
}

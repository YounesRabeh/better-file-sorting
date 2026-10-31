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
        assert!(stream.read(&mut request).unwrap() > 0);
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

#[test]
fn redirect_hops_consume_request_budget() {
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
        assert!(stream.read(&mut request).unwrap() > 0);
        write!(stream,"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let config = bfs::acquisition::DownloadConfig {
        max_requests: Some(1),
        ..Default::default()
    };
    let http = bfs::http_source::HttpSource::new(
        &SourceConfig {
            page: page.clone(),
            ..Default::default()
        },
        &config,
    )
    .unwrap();
    assert!(
        http.get(&page, &AtomicBool::new(false))
            .unwrap_err()
            .to_string()
            .contains("request limit")
    );
    handle.join().unwrap();
}

#[test]
fn rate_limited_origin_is_not_requested_again() {
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
        assert!(stream.read(&mut request).unwrap() > 0);
        write!(stream,"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 60\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let http = bfs::http_source::HttpSource::new(
        &SourceConfig {
            page: page.clone(),
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    let flag = AtomicBool::new(false);
    assert!(
        http.get(&page, &flag)
            .unwrap_err()
            .to_string()
            .contains("Retry-After: 60")
    );
    handle.join().unwrap();
    assert!(
        http.get(&page, &flag)
            .unwrap_err()
            .to_string()
            .contains("origin rate limited")
    );
}

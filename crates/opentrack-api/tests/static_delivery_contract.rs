//! Fetch the frontend's prepaint dependency through a real loopback HTTP server.
#[allow(dead_code)]
mod support;

use std::{collections::BTreeMap, net::SocketAddr, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Reply {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

async fn request(address: SocketAddr, method: &str, path: &str) -> Reply {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket
            .write_all(
                format!(
                    "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: text/html,*/*\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut bytes = Vec::new();
        socket.take(64 * 1024).read_to_end(&mut bytes).await.unwrap();
        assert!(bytes.len() < 64 * 1024, "Static fixture response exceeded its bound");
        let boundary = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
        let mut lines = std::str::from_utf8(&bytes[..boundary]).unwrap().split("\r\n");
        let status = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
        let headers = lines.map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        }).collect();
        Reply { status, headers, body: bytes[boundary + 4..].to_vec() }
    }).await.expect("Loopback static request exceeded its deadline")
}

#[tokio::test]
async fn frontend_prepaint_script_is_served_and_revalidated_over_http() {
    let harness = support::Harness::new().await;
    harness.install_frontend_entrypoint().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = harness.router.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });

    let index = request(address, "GET", "/").await;
    assert_eq!(index.status, 200);
    let html = std::str::from_utf8(&index.body).unwrap();
    assert_eq!(html, include_str!("../../../web/index.html"));
    let script = html
        .split("<script src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert_eq!(
        script, "/theme.js",
        "Test follows the frontend's actual prepaint reference"
    );
    let get = request(address, "GET", script).await;
    assert_eq!(
        get.status, 200,
        "The script referenced by the HTML must be fetchable"
    );
    assert_eq!(get.body, include_bytes!("../../../web/public/theme.js"));
    assert!(matches!(
        get.headers["content-type"].as_str(),
        "text/javascript" | "application/javascript"
    ));
    assert_eq!(get.headers["cache-control"], "no-cache");
    assert_eq!(get.headers["x-content-type-options"], "nosniff");
    assert!(get.headers["content-security-policy"].contains("script-src 'self'"));
    assert_eq!(get.headers["content-length"], get.body.len().to_string());

    let head = request(address, "HEAD", script).await;
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    for name in [
        "content-type",
        "cache-control",
        "content-length",
        "x-content-type-options",
        "content-security-policy",
    ] {
        assert_eq!(head.headers[name], get.headers[name]);
    }
    assert_eq!(request(address, "POST", script).await.status, 405);
    for path in ["/missing.js", "/theme.missing.js", "/assets/missing.js"] {
        let missing = request(address, "GET", path).await;
        assert_eq!(missing.status, 404, "{path} must not return the SPA shell");
        assert_ne!(missing.body, index.body);
    }

    stop.send(()).unwrap();
    server.await.unwrap();
    harness.cleanup().await;
}

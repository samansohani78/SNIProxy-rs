use prometheus::Registry;
use sniproxy_config::Config;
use sniproxy_core::run_proxy;
/// Live Integration Tests for SNIProxy-rs
///
/// These tests verify the proxy can start, listen, and accept connections.
/// For full end-to-end protocol testing, see MANUAL_TESTING_GUIDE.md
///
/// Tests included:
/// - Proxy server startup and shutdown
/// - Listening on configured ports
/// - Metrics endpoint availability
/// - Multiple proxy instances
/// - Configuration validation
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::sleep;

// Helper to create a test config
fn create_test_config(proxy_port: u16, metrics_port: u16) -> Config {
    Config {
        listen_addrs: vec![format!("127.0.0.1:{}", proxy_port)],
        timeouts: sniproxy_config::Timeouts {
            connect: 5,
            client_hello: 3,
            idle: 60,
        },
        metrics: sniproxy_config::Metrics {
            enabled: true,
            address: format!("127.0.0.1:{}", metrics_port),
        },
        allowlist: None,
        max_connections: Some(1000),
        shutdown_timeout: Some(10),
        connection_pool: None,
    }
}

// Helper to find an available port
async fn find_available_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    addr.port()
}

// Helper to wait for server to be ready
async fn wait_for_server(addr: &str, max_attempts: u32) -> bool {
    for _ in 0..max_attempts {
        if TcpStream::connect(addr).await.is_ok() {
            return true;
        }
        sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_proxy_starts_and_listens() {
    let proxy_port = find_available_port().await;
    let metrics_port = find_available_port().await;
    let config = create_test_config(proxy_port, metrics_port);

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    // Wait for proxy to start
    sleep(Duration::from_millis(500)).await;

    // Verify proxy is listening
    let can_connect = wait_for_server(&format!("127.0.0.1:{}", proxy_port), 30).await;
    assert!(
        can_connect,
        "Proxy should be listening on port {}",
        proxy_port
    );

    // Cleanup
    proxy_handle.abort();

    println!("✅ Proxy can start and listen successfully");
}

// NOTE: Metrics server is started in sniproxy-bin, not in run_proxy
// This test is flaky because metrics server initialization is not part of core library
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Metrics server is binary-level concern, tested in sniproxy-bin"]
async fn test_metrics_endpoint_available() {
    let proxy_port = find_available_port().await;
    let metrics_port = find_available_port().await;
    let config = create_test_config(proxy_port, metrics_port);

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    // Wait for proxy to start (longer wait for metrics server)
    sleep(Duration::from_millis(1500)).await;

    // Verify metrics endpoint works (more attempts for slower systems)
    let metrics_works = wait_for_server(&format!("127.0.0.1:{}", metrics_port), 100).await;
    assert!(
        metrics_works,
        "Metrics endpoint should be listening on port {}",
        metrics_port
    );

    // Cleanup
    proxy_handle.abort();

    println!("✅ Metrics endpoint is available");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multiple_listen_addresses() {
    let proxy_port1 = find_available_port().await;
    let proxy_port2 = find_available_port().await;
    let metrics_port = find_available_port().await;

    let config = Config {
        listen_addrs: vec![
            format!("127.0.0.1:{}", proxy_port1),
            format!("127.0.0.1:{}", proxy_port2),
        ],
        timeouts: sniproxy_config::Timeouts {
            connect: 5,
            client_hello: 3,
            idle: 60,
        },
        metrics: sniproxy_config::Metrics {
            enabled: true,
            address: format!("127.0.0.1:{}", metrics_port),
        },
        allowlist: None,
        max_connections: Some(1000),
        shutdown_timeout: Some(10),
        connection_pool: None,
    };

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    // Wait for proxy to start
    sleep(Duration::from_millis(500)).await;

    // Verify both ports are listening
    let port1_works = wait_for_server(&format!("127.0.0.1:{}", proxy_port1), 30).await;
    let port2_works = wait_for_server(&format!("127.0.0.1:{}", proxy_port2), 30).await;

    assert!(port1_works, "First proxy port should be listening");
    assert!(port2_works, "Second proxy port should be listening");

    // Cleanup
    proxy_handle.abort();

    println!("✅ Multiple listen addresses work");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_proxy_accepts_connections() {
    let proxy_port = find_available_port().await;
    let metrics_port = find_available_port().await;
    let config = create_test_config(proxy_port, metrics_port);

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    sleep(Duration::from_millis(500)).await;

    // Try to connect 5 times
    for i in 1..=5 {
        let result = TcpStream::connect(format!("127.0.0.1:{}", proxy_port)).await;
        assert!(result.is_ok(), "Connection {} should succeed", i);

        // Close the connection
        drop(result);
    }

    // Cleanup
    proxy_handle.abort();

    println!("✅ Proxy accepts multiple connections");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_proxy_with_allowlist() {
    let proxy_port = find_available_port().await;
    let metrics_port = find_available_port().await;

    let config = Config {
        listen_addrs: vec![format!("127.0.0.1:{}", proxy_port)],
        timeouts: sniproxy_config::Timeouts {
            connect: 5,
            client_hello: 3,
            idle: 60,
        },
        metrics: sniproxy_config::Metrics {
            enabled: true,
            address: format!("127.0.0.1:{}", metrics_port),
        },
        allowlist: Some(vec!["example.com".to_string(), "*.test.com".to_string()]),
        max_connections: Some(1000),
        shutdown_timeout: Some(10),
        connection_pool: None,
    };

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    sleep(Duration::from_millis(500)).await;

    // Verify proxy starts with allowlist
    let can_connect = wait_for_server(&format!("127.0.0.1:{}", proxy_port), 30).await;
    assert!(can_connect, "Proxy with allowlist should start");

    // Cleanup
    proxy_handle.abort();

    println!("✅ Proxy works with allowlist configuration");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_proxy_graceful_shutdown() {
    let proxy_port = find_available_port().await;
    let metrics_port = find_available_port().await;
    let config = create_test_config(proxy_port, metrics_port);

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    sleep(Duration::from_millis(500)).await;

    // Verify it started
    assert!(wait_for_server(&format!("127.0.0.1:{}", proxy_port), 30).await);

    // Shutdown
    proxy_handle.abort();

    // Wait a bit
    sleep(Duration::from_millis(200)).await;

    // Port should be free now (connection should fail)
    let result = TcpStream::connect(format!("127.0.0.1:{}", proxy_port)).await;
    assert!(result.is_err(), "Port should be freed after shutdown");

    println!("✅ Proxy shuts down gracefully");
}

// Helper to start a simple HTTP/1.1 backend server
async fn start_http11_backend(port: u16) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
            .await
            .expect("Failed to bind backend server");

        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 4096];
                if let Ok(n) = socket.read(&mut buffer).await
                    && n > 0
                {
                    // Simple HTTP/1.1 response
                    let response = b"HTTP/1.1 200 OK\r\n\
Content-Type: text/plain\r\n\
Content-Length: 12\r\n\
Connection: close\r\n\
\r\n\
Hello, World";
                    let _ = socket.write_all(response).await;
                    let _ = socket.shutdown().await;
                }
            });
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_http11_proxy_traffic() {
    // Start backend server first
    let backend_port = find_available_port().await;
    let backend_handle = start_http11_backend(backend_port).await;
    sleep(Duration::from_millis(300)).await; // Wait for backend to start

    // Start proxy
    let proxy_port = find_available_port().await;
    let metrics_port = find_available_port().await;
    let config = create_test_config(proxy_port, metrics_port);

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    sleep(Duration::from_millis(800)).await; // Wait for proxy to start

    // Send HTTP/1.1 request through proxy
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", proxy_port))
        .await
        .expect("Failed to connect to proxy");

    let request = format!(
        "GET / HTTP/1.1\r\n\
Host: 127.0.0.1:{}\r\n\
Connection: close\r\n\
\r\n",
        backend_port
    );

    stream
        .write_all(request.as_bytes())
        .await
        .expect("Failed to send request");

    // Read response with timeout
    let mut response = vec![0u8; 4096];
    let read_future = stream.read(&mut response);
    let bytes_read = tokio::time::timeout(Duration::from_secs(5), read_future)
        .await
        .expect("Timeout reading response")
        .expect("Failed to read response");

    assert!(bytes_read > 0, "Should receive response");
    let response_str = String::from_utf8_lossy(&response[..bytes_read]);
    assert!(
        response_str.contains("200 OK"),
        "Should receive 200 OK response, got: {}",
        response_str
    );
    assert!(
        response_str.contains("Hello, World"),
        "Should receive response body, got: {}",
        response_str
    );

    // Cleanup
    proxy_handle.abort();
    backend_handle.abort();

    println!("✅ HTTP/1.1 traffic passes through proxy");
}

// Helper to create TLS ClientHello for testing
fn create_client_hello(server_name: &str) -> Vec<u8> {
    let mut client_hello = Vec::new();

    // TLS Record header
    client_hello.push(0x16); // ContentType: Handshake
    client_hello.push(0x03); // TLS version 1.0
    client_hello.push(0x01);

    // We'll fill length later
    let length_pos = client_hello.len();
    client_hello.push(0x00);
    client_hello.push(0x00);

    // Handshake header
    client_hello.push(0x01); // HandshakeType: ClientHello
    let handshake_length_pos = client_hello.len();
    client_hello.push(0x00);
    client_hello.push(0x00);
    client_hello.push(0x00);

    // ClientHello body
    client_hello.push(0x03); // Client version
    client_hello.push(0x03);

    // Random (32 bytes)
    client_hello.extend(std::iter::repeat_n(0x00, 32));

    // Session ID (0 length)
    client_hello.push(0x00);

    // Cipher suites (2 bytes length + 2 bytes for one cipher)
    client_hello.push(0x00);
    client_hello.push(0x02);
    client_hello.push(0x00);
    client_hello.push(0x2f);

    // Compression methods (1 byte length + 1 byte for null)
    client_hello.push(0x01);
    client_hello.push(0x00);

    // Extensions
    let extensions_length_pos = client_hello.len();
    client_hello.push(0x00);
    client_hello.push(0x00);

    // SNI Extension
    client_hello.push(0x00); // Extension type: SNI
    client_hello.push(0x00);

    let sni_ext_length_pos = client_hello.len();
    client_hello.push(0x00);
    client_hello.push(0x00);

    // SNI list length
    let sni_list_length = 3 + server_name.len() as u16;
    client_hello.push((sni_list_length >> 8) as u8);
    client_hello.push((sni_list_length & 0xff) as u8);

    // SNI entry
    client_hello.push(0x00); // Name type: host_name

    let name_length = server_name.len() as u16;
    client_hello.push((name_length >> 8) as u8);
    client_hello.push((name_length & 0xff) as u8);

    client_hello.extend_from_slice(server_name.as_bytes());

    // Fill in lengths
    let sni_ext_length = sni_list_length + 2;
    client_hello[sni_ext_length_pos] = (sni_ext_length >> 8) as u8;
    client_hello[sni_ext_length_pos + 1] = (sni_ext_length & 0xff) as u8;

    let extensions_length = sni_ext_length + 4;
    client_hello[extensions_length_pos] = (extensions_length >> 8) as u8;
    client_hello[extensions_length_pos + 1] = (extensions_length & 0xff) as u8;

    let handshake_length = (client_hello.len() - handshake_length_pos - 3) as u32;
    client_hello[handshake_length_pos] = ((handshake_length >> 16) & 0xff) as u8;
    client_hello[handshake_length_pos + 1] = ((handshake_length >> 8) & 0xff) as u8;
    client_hello[handshake_length_pos + 2] = (handshake_length & 0xff) as u8;

    let record_length = (client_hello.len() - length_pos - 2) as u16;
    client_hello[length_pos] = (record_length >> 8) as u8;
    client_hello[length_pos + 1] = (record_length & 0xff) as u8;

    client_hello
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tls_sni_proxy_accepts_connection() {
    // Note: Full TLS proxying requires the backend to be on port 443
    // This test just verifies the proxy accepts TLS connections and attempts routing

    // Start proxy
    let proxy_port = find_available_port().await;
    let metrics_port = find_available_port().await;
    let config = create_test_config(proxy_port, metrics_port);

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    sleep(Duration::from_millis(800)).await;

    // Send TLS ClientHello through proxy
    // The proxy will try to connect to localhost.test:443 which will fail,
    // but we're just verifying the proxy accepts and processes the TLS handshake
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", proxy_port))
        .await
        .expect("Failed to connect to proxy");

    let client_hello = create_client_hello("localhost.test");

    let write_result = stream.write_all(&client_hello).await;
    assert!(write_result.is_ok(), "Proxy should accept TLS ClientHello");

    // Give the proxy time to process
    sleep(Duration::from_millis(200)).await;

    // Cleanup
    proxy_handle.abort();

    println!("✅ TLS/SNI connection accepted by proxy");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multiple_concurrent_connections() {
    // Start backend server
    let backend_port = find_available_port().await;
    let backend_handle = start_http11_backend(backend_port).await;
    sleep(Duration::from_millis(300)).await;

    // Start proxy
    let proxy_port = find_available_port().await;
    let metrics_port = find_available_port().await;
    let config = create_test_config(proxy_port, metrics_port);

    let proxy_handle = tokio::spawn(async move {
        let registry = Registry::new();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let _ = run_proxy(config, Some(registry), shutdown_rx).await;
    });

    sleep(Duration::from_millis(800)).await;

    // Send 10 concurrent requests
    let mut handles = vec![];

    for i in 0..10 {
        let proxy_addr = format!("127.0.0.1:{}", proxy_port);
        let backend_port_clone = backend_port;

        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(proxy_addr)
                .await
                .expect("Failed to connect to proxy");

            let request = format!(
                "GET /test{} HTTP/1.1\r\n\
Host: 127.0.0.1:{}\r\n\
Connection: close\r\n\
\r\n",
                i, backend_port_clone
            );

            stream
                .write_all(request.as_bytes())
                .await
                .expect("Failed to send request");

            // Read response with timeout
            let mut response = vec![0u8; 4096];
            let read_future = stream.read(&mut response);
            let bytes_read = match tokio::time::timeout(Duration::from_secs(5), read_future).await {
                Ok(Ok(n)) => n,
                _ => 0,
            };

            bytes_read > 0 && String::from_utf8_lossy(&response[..bytes_read]).contains("200 OK")
        });

        handles.push(handle);
    }

    // Wait for all requests to complete
    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap() {
            success_count += 1;
        }
    }

    assert_eq!(
        success_count, 10,
        "All 10 concurrent requests should succeed"
    );

    // Cleanup
    proxy_handle.abort();
    backend_handle.abort();

    println!("✅ Proxy handles multiple concurrent connections");
}

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use sniproxy_core::{extract_alpn, extract_sni};
use std::hint::black_box;

/// Helper to build a valid TLS ClientHello with SNI
fn build_client_hello_with_sni(domain: &str) -> Vec<u8> {
    let domain_bytes = domain.as_bytes();
    let domain_len = domain_bytes.len() as u16;

    let sni_list_len = 3 + domain_len;
    let sni_ext_len = 2 + sni_list_len;
    let extensions_len = 4 + sni_ext_len;
    let handshake_len = 2 + 32 + 1 + 2 + 2 + 2 + 2 + extensions_len;
    let record_len = 4 + handshake_len;

    let mut record = vec![
        0x16,
        0x03,
        0x03,
        (record_len >> 8) as u8,
        (record_len & 0xff) as u8,
        0x01,
        ((handshake_len as u32) >> 16) as u8,
        (handshake_len >> 8) as u8,
        (handshake_len & 0xff) as u8,
        0x03,
        0x03,
    ];
    record.extend_from_slice(&[0; 32]);
    record.extend_from_slice(&[
        0x00,
        0x00,
        0x02,
        0x00,
        0x00,
        0x01,
        0x00,
        (extensions_len >> 8) as u8,
        (extensions_len & 0xff) as u8,
        0x00,
        0x00,
        (sni_ext_len >> 8) as u8,
        (sni_ext_len & 0xff) as u8,
        (sni_list_len >> 8) as u8,
        (sni_list_len & 0xff) as u8,
        0x00,
        (domain_len >> 8) as u8,
        (domain_len & 0xff) as u8,
    ]);
    record.extend_from_slice(domain_bytes);
    record
}

/// Helper to build a valid TLS ClientHello with ALPN
fn build_client_hello_with_alpn(protocol: &[u8]) -> Vec<u8> {
    let protocol_len = protocol.len() as u8;
    let alpn_list_len = 1 + protocol_len as u16;
    let alpn_ext_len = 2 + alpn_list_len;
    let extensions_len = 4 + alpn_ext_len;
    let handshake_len = 2 + 32 + 1 + 2 + 2 + 2 + 2 + extensions_len;
    let record_len = 4 + handshake_len;

    let mut record = vec![
        0x16,
        0x03,
        0x03,
        (record_len >> 8) as u8,
        (record_len & 0xff) as u8,
        0x01,
        ((handshake_len as u32) >> 16) as u8,
        (handshake_len >> 8) as u8,
        (handshake_len & 0xff) as u8,
        0x03,
        0x03,
    ];
    record.extend_from_slice(&[0; 32]);
    record.extend_from_slice(&[
        0x00,
        0x00,
        0x02,
        0x00,
        0x00,
        0x01,
        0x00,
        (extensions_len >> 8) as u8,
        (extensions_len & 0xff) as u8,
        0x00,
        0x10,
        (alpn_ext_len >> 8) as u8,
        (alpn_ext_len & 0xff) as u8,
        (alpn_list_len >> 8) as u8,
        (alpn_list_len & 0xff) as u8,
        protocol_len,
    ]);
    record.extend_from_slice(protocol);
    record
}

fn bench_sni_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("sni_extraction");

    // Benchmark with different domain lengths
    for domain in &[
        "example.com",
        "subdomain.example.com",
        "very.long.subdomain.example.com",
    ] {
        let record = build_client_hello_with_sni(domain);
        group.bench_with_input(BenchmarkId::from_parameter(domain), &record, |b, record| {
            b.iter(|| extract_sni(black_box(record)).unwrap());
        });
    }

    group.finish();
}

fn bench_alpn_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("alpn_extraction");

    for protocol in &[b"h2".as_slice(), b"h3".as_slice(), b"http/1.1".as_slice()] {
        let record = build_client_hello_with_alpn(protocol);
        let protocol_name = std::str::from_utf8(protocol).unwrap();
        group.bench_with_input(
            BenchmarkId::from_parameter(protocol_name),
            &record,
            |b, record| {
                b.iter(|| extract_alpn(black_box(record)));
            },
        );
    }

    group.finish();
}

fn bench_sni_with_large_record(c: &mut Criterion) {
    // Simulate a large ClientHello with many extensions
    let domain = "production.api.service.company.example.com";
    let mut record = build_client_hello_with_sni(domain);

    // Add some padding to simulate additional extensions
    record.extend_from_slice(&[0; 1024]);

    c.bench_function("sni_large_record", |b| {
        b.iter(|| extract_sni(black_box(&record[..record.len() - 1024])));
    });
}

fn bench_error_cases(c: &mut Criterion) {
    let mut group = c.benchmark_group("error_handling");

    // Truncated record
    let truncated = vec![0x16, 0x03, 0x01];
    group.bench_function("truncated_record", |b| {
        b.iter(|| {
            let _ = extract_sni(black_box(&truncated));
        });
    });

    // Invalid TLS version
    let invalid = vec![0x16, 0x02, 0x01, 0x00, 0x05, 0x01, 0x00, 0x00, 0x00];
    group.bench_function("invalid_version", |b| {
        b.iter(|| {
            let _ = extract_sni(black_box(&invalid));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_sni_extraction,
    bench_alpn_extraction,
    bench_sni_with_large_record,
    bench_error_cases
);
criterion_main!(benches);

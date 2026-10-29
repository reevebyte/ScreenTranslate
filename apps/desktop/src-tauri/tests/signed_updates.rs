// Synthetic payload and optional locally signed release; no installer is executed.
mod signed_download_tests {
    use std::time::Duration;
    use tauri_plugin_updater::UpdaterExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn verifies_signed_loopback_payload_and_rejects_tampering() {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert("updater".into(),serde_json::json!({
            "pubkey":include_str!("../test-fixtures/updater-test.pub").trim(),"dangerousInsecureTransportProtocol":true
        }));
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let mut cases = vec![(
            include_str!("../test-fixtures/updater-test.pub")
                .trim()
                .to_string(),
            include_str!("../test-fixtures/updater-payload.txt.sig")
                .trim()
                .to_string(),
            include_bytes!("../test-fixtures/updater-payload.txt").to_vec(),
            "99.0.0",
        )];
        if let Ok(path) = std::env::var("SCREENTRANS_VERIFY_INSTALLER") {
            let payload = std::fs::read(&path).unwrap();
            let signature = std::fs::read_to_string(format!("{path}.sig")).unwrap();
            cases.push((
                include_str!("../updater.pub").trim().to_string(),
                signature.trim().to_string(),
                payload,
                env!("CARGO_PKG_VERSION"),
            ));
        }
        for (public, signature, payload, version) in cases {
            for tamper in [false, true] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let origin = format!("http://{}", listener.local_addr().unwrap());
                let manifest = serde_json::json!({"version":version,"platforms":{"fixture":{
                    "url":format!("{origin}/fixture"),"signature":signature
                }}})
                .to_string();
                let server_payload = payload.clone();
                let server = tokio::spawn(async move {
                    loop {
                        let (mut stream, _) = listener.accept().await.unwrap();
                        let mut request = Vec::new();
                        while !request.ends_with(b"\r\n\r\n") {
                            let mut byte = [0_u8];
                            if stream.read(&mut byte).await.unwrap() == 0 {
                                break;
                            }
                            request.push(byte[0]);
                            assert!(request.len() < 8192);
                        }
                        let body: &[u8] = if request.starts_with(b"GET /latest.json ") {
                            manifest.as_bytes()
                        } else if tamper {
                            b"tampered bytes"
                        } else {
                            &server_payload
                        };
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        stream.write_all(header.as_bytes()).await.unwrap();
                        stream.write_all(body).await.unwrap();
                    }
                });
                let update = app
                    .updater_builder()
                    .target("fixture")
                    .pubkey(public.clone())
                    .endpoints(vec![format!("{origin}/latest.json").parse().unwrap()])
                    .unwrap()
                    .no_proxy()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap()
                    .check()
                    .await
                    .unwrap()
                    .unwrap();
                let downloaded = update.download(|_, _| {}, || {}).await;
                if tamper {
                    assert!(downloaded.is_err());
                } else {
                    assert_eq!(downloaded.unwrap(), payload.as_slice());
                    assert!(update.install(b"tampered after verification").is_err());
                }
                server.abort();
            }
        }
    }
}

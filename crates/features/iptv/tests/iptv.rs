#![allow(clippy::unwrap_used, clippy::expect_used)]

use cinma_iptv::{IptvClient, m3u::parse_m3u, xmltv::parse_xmltv};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn parses_m3u_and_xmltv() {
    let playlist = "\
#EXTM3U
#EXTINF:-1 tvg-id=\"bbc.uk\" tvg-logo=\"https://logo/bbc.png\" group-title=\"News\",BBC World
https://stream.example/bbc.m3u8
#EXTINF:-1 group-title=\"Sports\",Local Sport
https://stream.example/sport.ts
";
    let channels = parse_m3u(playlist);
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].name, "BBC World");
    assert_eq!(channels[0].group_title.as_deref(), Some("News"));
    assert_eq!(channels[0].epg_channel_id.as_deref(), Some("bbc.uk"));
    assert_eq!(channels[1].stream_url, "https://stream.example/sport.ts");

    let epg_xml = r#"<tv>
      <programme start="20261117220000 +0300" stop="20261117230000 +0300" channel="bbc.uk">
        <title lang="en">World News</title>
        <desc>Hourly bulletin</desc>
      </programme>
    </tv>"#;
    let epg = parse_xmltv(epg_xml);
    assert_eq!(epg.len(), 1);
    assert_eq!(epg[0].channel_id, "bbc.uk");
    assert_eq!(epg[0].title, "World News");
    assert_eq!(epg[0].stop_ts - epg[0].start_ts, 3600);
}

#[tokio::test]
async fn xtream_mock_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let body = if req.contains("get_live_categories") {
                    r#"[{"category_id":"10","category_name":"Movies"}]"#
                } else {
                    r#"[{"stream_id":55,"name":"Cinema HD","stream_icon":"https://icon/c.png","epg_channel_id":"cinema.hd","category_id":"10"}]"#
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });

    let client = IptvClient::new().unwrap();
    let chans = client
        .fetch_xtream_live(&format!("http://{addr}"), "u", "p")
        .await
        .unwrap();
    assert_eq!(chans.len(), 1);
    assert_eq!(chans[0].group_title.as_deref(), Some("Movies"));
    assert_eq!(chans[0].stream_url, format!("http://{addr}/live/u/p/55.ts"));
}

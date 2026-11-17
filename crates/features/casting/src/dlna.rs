//! DLNA UPnP AVTransport:1 SOAP control.

use crate::CastError;

pub struct DlnaClient {
    http: reqwest::Client,
    control_url: String,
}

impl DlnaClient {
    pub fn new(control_url: String) -> Result<Self, CastError> {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(3))
            .pool_max_idle_per_host(0)
            .build()?;
        Ok(Self { http, control_url })
    }

    pub async fn set_av_transport_uri(
        &self,
        media_url: &str,
        title: &str,
    ) -> Result<(), CastError> {
        let esc_url = xml_escape(media_url);
        let esc_title = xml_escape(title);
        let meta = format!(
            "&lt;DIDL-Lite xmlns=&quot;urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/&quot; \
             xmlns:dc=&quot;http://purl.org/dc/elements/1.1/&quot; \
             xmlns:upnp=&quot;urn:schemas-upnp-org:metadata-1-0/upnp/&quot;&gt;\
             &lt;item id=&quot;0&quot; parentID=&quot;-1&quot; restricted=&quot;1&quot;&gt;\
             &lt;dc:title&gt;{esc_title}&lt;/dc:title&gt;\
             &lt;upnp:class&gt;object.item.videoItem&lt;/upnp:class&gt;\
             &lt;res protocolInfo=&quot;http-get:*:video/x-matroska:*&quot;&gt;{esc_url}&lt;/res&gt;\
             &lt;/item&gt;&lt;/DIDL-Lite&gt;"
        );
        let body = format!(
            "<u:SetAVTransportURI xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">\
             <InstanceID>0</InstanceID>\
             <CurrentURI>{esc_url}</CurrentURI>\
             <CurrentURIMetaData>{meta}</CurrentURIMetaData>\
             </u:SetAVTransportURI>"
        );
        self.soap("SetAVTransportURI", &body).await
    }

    pub async fn play(&self) -> Result<(), CastError> {
        self.soap(
            "Play",
            "<u:Play xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">\
             <InstanceID>0</InstanceID><Speed>1</Speed></u:Play>",
        )
        .await
    }

    pub async fn pause(&self) -> Result<(), CastError> {
        self.soap(
            "Pause",
            "<u:Pause xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">\
             <InstanceID>0</InstanceID></u:Pause>",
        )
        .await
    }

    pub async fn stop(&self) -> Result<(), CastError> {
        self.soap(
            "Stop",
            "<u:Stop xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">\
             <InstanceID>0</InstanceID></u:Stop>",
        )
        .await
    }

    pub async fn seek_seconds(&self, seconds: u64) -> Result<(), CastError> {
        let h = seconds / 3600;
        let m = (seconds / 60) % 60;
        let s = seconds % 60;
        let target = format!("{h:02}:{m:02}:{s:02}");
        let body = format!(
            "<u:Seek xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">\
             <InstanceID>0</InstanceID><Unit>REL_TIME</Unit><Target>{target}</Target></u:Seek>"
        );
        self.soap("Seek", &body).await
    }

    async fn soap(&self, action: &str, inner_xml: &str) -> Result<(), CastError> {
        let envelope = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
             <s:Body>{inner_xml}</s:Body></s:Envelope>"
        );
        let resp = self
            .http
            .post(&self.control_url)
            .header("Content-Type", "text/xml; charset=\"utf-8\"")
            .header("Connection", "close")
            .header(
                "SOAPACTION",
                format!("\"urn:schemas-upnp-org:service:AVTransport:1#{action}\""),
            )
            .body(envelope)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(CastError::Invalid(format!(
                "DLNA {action} returned HTTP {}",
                resp.status()
            )));
        }
        Ok(())
    }
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

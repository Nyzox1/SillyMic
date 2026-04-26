use std::sync::Arc;

use anyhow::Context;
use tokio::sync::mpsc;
use tracing::warn;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_OPUS},
        APIBuilder,
    },
    ice_transport::ice_server::RTCIceServer,
    interceptor::registry::Registry,
    peer_connection::{configuration::RTCConfiguration, RTCPeerConnection},
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
};

use crate::signal::SignalMessage;

pub struct WebRtcEngine {
    api: Arc<webrtc::api::API>,
    track: Arc<TrackLocalStaticSample>,
}

impl WebRtcEngine {
    pub fn new() -> anyhow::Result<Self> {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .context("Could not register default WebRTC codecs")?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .context("Could not register default WebRTC interceptors")?;

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                clock_rate: 48_000,
                channels: 1,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                rtcp_feedback: vec![],
            },
            "audio".to_string(),
            "sillymic".to_string(),
        ));

        Ok(Self {
            api: Arc::new(api),
            track,
        })
    }

    pub fn track(&self) -> Arc<TrackLocalStaticSample> {
        Arc::clone(&self.track)
    }

    pub async fn create_peer(
        &self,
        signal_tx: mpsc::UnboundedSender<SignalMessage>,
    ) -> anyhow::Result<Arc<RTCPeerConnection>> {
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let peer = Arc::new(
            self.api
                .new_peer_connection(config)
                .await
                .context("Could not create peer connection")?,
        );

        let rtp_sender = peer
            .add_track(Arc::clone(&self.track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .context("Could not attach local audio track")?;

        tokio::spawn(async move {
            let mut rtcp_buf = vec![0_u8; 1500];
            while rtp_sender.read(&mut rtcp_buf).await.is_ok() {}
        });

        peer.on_ice_candidate(Box::new(move |candidate| {
            let signal_tx = signal_tx.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    match candidate.to_json() {
                        Ok(json) => {
                            let _ = signal_tx.send(SignalMessage::IceCandidate {
                                candidate: json.candidate,
                                sdp_mid: json.sdp_mid,
                                sdp_mline_index: json.sdp_mline_index,
                            });
                        }
                        Err(err) => warn!("Could not serialize ICE candidate: {err}"),
                    }
                }
            })
        }));

        Ok(peer)
    }
}

# SillyMic v1

Bridge micro PC vers iPhone sur le même réseau local, sans matériel additionnel.

## Stack

- **PC host**: Rust CLI (`cpal` + `audiopus` + `webrtc` + `axum`)
- **Mobile**: React Native bare + TypeScript + `react-native-webrtc`
- **Signaling**: WebSocket JSON (`/signal`)
- **Session API**: HTTP local (`/health`, `/session/create`, `/session/status`)

## Dossiers

- `backend/`: serveur audio + CLI
- `mobile/`: app iOS receiver
- `.github/workflows/ios-unsigned.yml`: fallback CI pour IPA non signée

## Backend quickstart

```bash
cargo run -p sillymic-host -- devices
cargo run -p sillymic-host -- doctor --port 41777
cargo run -p sillymic-host -- host --port 41777 --pin 123456
```

### CLI

- `sillymic host --port 41777 --pin 6digits --input <device-id-or-name>`
- `sillymic devices`
- `sillymic doctor --port 41777 --input <optional>`

### API locale

- `POST /session/create` body optional: `{"pin":"123456"}`
- `GET /session/status?id=<uuid>`
- `GET /health`
- `GET ws /signal`

### Signal messages

- `hello`: `{ deviceName, appVersion, sessionCode }`
- `offer`: `{ sdp }`
- `answer`: `{ sdp }`
- `ice_candidate`: `{ candidate, sdpMid, sdpMLineIndex }`
- `ready`: `{ sessionId }`
- `error`: `{ code, message }`

## iPhone app quickstart

See [mobile/README.md](mobile/README.md).

## Security and robustness (v1)

- LAN-only deployment (bind local host service only; no WAN relay setup included)
- PIN/session code required
- Session TTL 60s for non-streaming states
- Strict signal payload size cap
- Single mobile connection at a time


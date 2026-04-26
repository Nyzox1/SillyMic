# SillyMic Mobile (iOS-first, React Native bare)

This folder contains the receiver app that connects to the Rust host over LAN and plays the incoming microphone stream.

## 1) Install dependencies

```bash
npm install
cd ios && pod install && cd ..
```

## 2) iOS required config

The required iOS keys are already configured in `ios/SillyMicNative/Info.plist`:

- `NSLocalNetworkUsageDescription`
- `NSBonjourServices`
- `UIBackgroundModes` with `audio`

## 3) Run against host

1. Start host on your PC:

```bash
cargo run -p sillymic-host -- host --pin 123456 --port 41777
```

2. On iPhone app:
- Enter PC LAN IP (for example `192.168.1.10`)
- Enter the same 6-digit PIN
- Tap `Connect`

## 4) EAS build from Windows

```bash
npm i -g eas-cli
eas login
eas build -p ios --profile development
```

Download IPA and install with Sideloadly.

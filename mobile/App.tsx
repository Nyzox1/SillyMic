import React, {useEffect, useMemo, useRef, useState} from 'react';
import {
  SafeAreaView,
  StatusBar,
  StyleSheet,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from 'react-native';

import {BridgeClient} from './src/bridgeClient';
import {ConnectionStatus} from './src/types';

const APP_VERSION = '0.1.0';

export default function App() {
  const [pcIp, setPcIp] = useState('192.168.1.10');
  const [sessionCode, setSessionCode] = useState('');
  const [status, setStatus] = useState<ConnectionStatus>('idle');
  const [sessionId, setSessionId] = useState('');
  const [error, setError] = useState('');
  const [level, setLevel] = useState(0);

  const clientRef = useRef<BridgeClient | null>(null);

  useEffect(() => {
    clientRef.current = new BridgeClient({
      onStatus: setStatus,
      onError: setError,
      onSessionReady: setSessionId,
      onLevel: setLevel,
    });
    return () => {
      clientRef.current?.disconnect();
    };
  }, []);

  const connectDisabled = useMemo(
    () => !pcIp.trim() || !sessionCode.trim() || status === 'connecting',
    [pcIp, sessionCode, status],
  );

  const connect = () => {
    setError('');
    setSessionId('');
    clientRef.current?.connect({
      pcIp: pcIp.trim(),
      sessionCode: sessionCode.trim(),
      deviceName: 'iPhone',
      appVersion: APP_VERSION,
    });
  };

  const disconnect = () => {
    clientRef.current?.disconnect();
    setSessionId('');
    setLevel(0);
  };

  return (
    <SafeAreaView style={styles.safeArea}>
      <StatusBar barStyle="light-content" />
      <View style={styles.container}>
        <Text style={styles.title}>SillyMic Receiver</Text>
        <Text style={styles.subtitle}>
          Bridge micro PC {'->'} iPhone (LAN only)
        </Text>

        <View style={styles.card}>
          <Text style={styles.label}>PC IP Address</Text>
          <TextInput
            style={styles.input}
            value={pcIp}
            onChangeText={setPcIp}
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="numbers-and-punctuation"
            placeholder="192.168.x.x"
            placeholderTextColor="#95a3b8"
          />

          <Text style={styles.label}>Session Code (PIN)</Text>
          <TextInput
            style={styles.input}
            value={sessionCode}
            onChangeText={setSessionCode}
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="number-pad"
            maxLength={6}
            placeholder="6-digit PIN"
            placeholderTextColor="#95a3b8"
          />

          <View style={styles.statusRow}>
            <Text style={styles.statusLabel}>Status</Text>
            <View style={[styles.badge, badgeStyle(status)]}>
              <Text style={styles.badgeText}>{status.toUpperCase()}</Text>
            </View>
          </View>

          <View style={styles.meterWrap}>
            <View style={styles.meterTrack}>
              <View style={[styles.meterFill, {width: `${Math.round(level * 100)}%`}]} />
            </View>
            <Text style={styles.meterText}>Input Level: {Math.round(level * 100)}%</Text>
          </View>

          {sessionId ? <Text style={styles.sessionId}>Session: {sessionId}</Text> : null}
          {error ? <Text style={styles.error}>{error}</Text> : null}

          <View style={styles.actions}>
            <TouchableOpacity
              style={[styles.button, styles.primary, connectDisabled && styles.disabled]}
              onPress={connect}
              disabled={connectDisabled}>
              <Text style={styles.buttonText}>Connect</Text>
            </TouchableOpacity>
            <TouchableOpacity
              style={[styles.button, styles.secondary]}
              onPress={disconnect}>
              <Text style={styles.buttonText}>Disconnect</Text>
            </TouchableOpacity>
          </View>
        </View>
      </View>
    </SafeAreaView>
  );
}

function badgeStyle(status: ConnectionStatus) {
  switch (status) {
    case 'streaming':
      return {backgroundColor: '#0f9d58'};
    case 'connecting':
      return {backgroundColor: '#e09f3e'};
    case 'error':
      return {backgroundColor: '#cf3c51'};
    default:
      return {backgroundColor: '#4d6079'};
  }
}

const styles = StyleSheet.create({
  safeArea: {
    flex: 1,
    backgroundColor: '#0b1220',
  },
  container: {
    flex: 1,
    padding: 20,
    backgroundColor: '#0b1220',
  },
  title: {
    color: '#f3f6ff',
    fontSize: 30,
    fontWeight: '700',
  },
  subtitle: {
    color: '#9eb3cf',
    marginTop: 6,
    marginBottom: 20,
    fontSize: 14,
  },
  card: {
    borderRadius: 20,
    padding: 18,
    backgroundColor: '#132034',
    borderWidth: 1,
    borderColor: '#243a5a',
    gap: 10,
  },
  label: {
    color: '#c8d9f0',
    fontSize: 13,
    fontWeight: '600',
  },
  input: {
    height: 46,
    borderRadius: 12,
    backgroundColor: '#0d1728',
    borderWidth: 1,
    borderColor: '#29456c',
    paddingHorizontal: 12,
    color: '#eff6ff',
  },
  statusRow: {
    marginTop: 4,
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  statusLabel: {
    color: '#bdd4f2',
    fontWeight: '600',
  },
  badge: {
    borderRadius: 999,
    paddingHorizontal: 10,
    paddingVertical: 5,
  },
  badgeText: {
    color: '#ffffff',
    fontSize: 12,
    fontWeight: '700',
  },
  meterWrap: {
    marginTop: 6,
  },
  meterTrack: {
    height: 12,
    borderRadius: 999,
    backgroundColor: '#0c1625',
    overflow: 'hidden',
    borderWidth: 1,
    borderColor: '#2b4469',
  },
  meterFill: {
    height: '100%',
    backgroundColor: '#2dd4bf',
  },
  meterText: {
    color: '#b5cae7',
    marginTop: 6,
    fontSize: 12,
  },
  sessionId: {
    marginTop: 6,
    color: '#a5c0e3',
    fontSize: 12,
  },
  error: {
    marginTop: 6,
    color: '#ff9fb0',
    fontSize: 12,
  },
  actions: {
    flexDirection: 'row',
    gap: 10,
    marginTop: 10,
  },
  button: {
    flex: 1,
    height: 44,
    borderRadius: 12,
    alignItems: 'center',
    justifyContent: 'center',
  },
  primary: {
    backgroundColor: '#2563eb',
  },
  secondary: {
    backgroundColor: '#3b4960',
  },
  disabled: {
    opacity: 0.4,
  },
  buttonText: {
    color: '#fff',
    fontSize: 15,
    fontWeight: '700',
  },
});


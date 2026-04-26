export type ConnectionStatus = 'idle' | 'connecting' | 'streaming' | 'error';

export type SignalMessage =
  | {
      type: 'hello';
      deviceName: string;
      appVersion: string;
      sessionCode: string;
    }
  | {
      type: 'offer';
      sdp: string;
    }
  | {
      type: 'answer';
      sdp: string;
    }
  | {
      type: 'ice_candidate';
      candidate: string;
      sdpMid?: string | null;
      sdpMLineIndex?: number | null;
    }
  | {
      type: 'ready';
      sessionId: string;
    }
  | {
      type: 'error';
      code: string;
      message: string;
    };

export interface BridgeConfig {
  pcIp: string;
  sessionCode: string;
  deviceName: string;
  appVersion: string;
}

export interface BridgeEvents {
  onStatus: (status: ConnectionStatus) => void;
  onError: (message: string) => void;
  onSessionReady: (sessionId: string) => void;
  onLevel: (level: number) => void;
}


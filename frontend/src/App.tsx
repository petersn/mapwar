import React from 'react';
import './App.css';
import { WebSocketRequest, WebSocketResponse } from './BackendTypes';

class GameConnection {
  ws: WebSocket | null;

  constructor() {
    this.ws = null;
    this.reconnect();
  }

  reconnect() {
    if (this.ws != null)
      this.ws.close();
    const ws = new WebSocket('ws://localhost:12001/api/game-connection');
    this.ws = ws
    ws.onopen = () => {
      console.log('Connected to game server');
      this.sendMessage({
        kind: 'ping',
        //kind: 'auth',
        //username: 'test',
        //account_token: 'test',
      });
    }
    ws.onclose = () => {
      console.log('Disconnected from game server');
      setTimeout(() => this.reconnect(), 1000);
    }
    ws.onmessage = (msg) => this.onMessage(JSON.parse(msg.data));
    ws.onerror = (err) => {
      console.log('Error from game server', err);
      ws.close();
    }
  }

  sendMessage(msg: WebSocketRequest) {
    if (this.ws == null) {
      console.log('Cannot send message to game server, not connected');
      return;
    }
    this.ws.send(JSON.stringify(msg));
  }

  onMessage(msg: WebSocketResponse) {
    console.log('Received message from game server', msg);
  }
}

class App extends React.Component<{}, {
  name: string;
}> {
  gc: GameConnection;

  constructor(props: {}) {
    super(props);
    this.state = {
      name: 'World'
    };
    this.gc = new GameConnection();
  }

  render() {
    return (
      <div>Hello, world?</div>
    );
  }
}

export default App;

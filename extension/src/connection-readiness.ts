// A Native Messaging port is created synchronously, but it is not safe for application messages
// until the host's handshake ACK has been fully validated. Keeping those states separate prevents
// a cold service-worker start from sending group/config operations before the handshake.
export class ConnectionReadiness {
  private portOpen = false;
  private handshakeAccepted = false;

  get connected(): boolean { return this.portOpen; }
  get ready(): boolean { return this.portOpen && this.handshakeAccepted; }

  opened(): void {
    this.portOpen = true;
    this.handshakeAccepted = false;
  }

  accepted(): void {
    if (!this.portOpen) throw new Error("native host disconnected during handshake");
    this.handshakeAccepted = true;
  }

  closed(): void {
    this.portOpen = false;
    this.handshakeAccepted = false;
  }
}

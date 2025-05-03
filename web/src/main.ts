type Message = { candidate: RTCIceCandidateInit } | { sdp: string };

const rtc = new RTCPeerConnection();
const ws = new WebSocket("http://localhost:3000/ws");

const send = (message: Message) => {
  ws.send(JSON.stringify(message));
};

ws.addEventListener("open", async () => {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  stream.getTracks().forEach((track) => rtc.addTrack(track, stream));
  rtc.addEventListener("connectionstatechange", () => {
    if (rtc.connectionState === "connected") {
      console.log("connection established");
    }
  });
  rtc.addEventListener("icecandidate", (event) => {
    if (event.candidate) {
      const { candidate, sdpMid, sdpMLineIndex, usernameFragment } =
        event.candidate;
      send({
        candidate: { candidate, sdpMid, sdpMLineIndex, usernameFragment },
      });
    }
  });
  const offer = await rtc.createOffer();
  if (offer.sdp) {
    await rtc.setLocalDescription(offer);
    send({ sdp: offer.sdp });
  }
});

ws.addEventListener("message", async (event) => {
  const message = JSON.parse(event.data) as Message;
  if ("candidate" in message) {
    await rtc.addIceCandidate(message.candidate);
  } else {
    await rtc.setRemoteDescription({ type: "answer", sdp: message.sdp });
  }
});

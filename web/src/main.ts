type ServerMessagePeer = {
  id: number;
  name: string;
};

type ServerMessage =
  | { candidate: RTCIceCandidateInit }
  | { offer: string }
  | { answer: string }
  | { id: number }
  | { peers: ServerMessagePeer[] }
  | { peer: ServerMessagePeer };

type PeerMessage =
  | { candidate: RTCIceCandidateInit }
  | { offer: string }
  | { answer: string }
  | { name: string }
  | { pli: boolean };

const app = document.getElementById("app")!;
const rtc = new RTCPeerConnection();
const signal = new WebSocket("http://localhost:3000/signal");

const addStream = (stream: MediaStream, tagName: "audio" | "video") => {
  const el = document.createElement(tagName);
  el.srcObject = stream;
  el.autoplay = true;
  app.appendChild(el);
};

const send = (message: PeerMessage) => {
  signal.send(JSON.stringify(message));
};

signal.addEventListener("open", async () => {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: true,
    video: true,
  });
  stream.getTracks().forEach((track) => rtc.addTrack(track, stream));
  addStream(stream, "video");
  addStream(stream, "audio");
  rtc.addEventListener("connectionstatechange", () => {
    if (rtc.connectionState === "connected") {
      console.log("connection established");
    }
  });
  rtc.addEventListener("negotiationneeded", () => {
    console.log("negotiation needed");
  });
  rtc.addEventListener("icecandidate", (event) => {
    console.log("new local ice candidate");
    if (event.candidate) {
      const { candidate, sdpMid, sdpMLineIndex, usernameFragment } =
        event.candidate;
      send({
        candidate: { candidate, sdpMid, sdpMLineIndex, usernameFragment },
      });
    }
  });
  rtc.addEventListener("track", (event) => {
    console.log("new remote track");
    const [stream] = event.streams;
    addStream(stream, event.track.kind as "audio" | "video");
    console.log("requesting pli");
    send({ pli: true });
  });
  const offer = await rtc.createOffer();
  await rtc.setLocalDescription(offer);
  if (offer.sdp) {
    send({ offer: offer.sdp });
    console.log("offer sent");
    console.log(offer.sdp);
    send({ name: `${Date.now()}` });
    console.log("name sent");
  }
});

signal.addEventListener("message", async (event) => {
  const message = JSON.parse(event.data) as ServerMessage;
  if ("candidate" in message) {
    console.log("new remote ice candidate");
    await rtc.addIceCandidate(message.candidate);
  } else if ("offer" in message) {
    console.log("offer received");
    console.log(message.offer);
    await rtc.setRemoteDescription({ type: "offer", sdp: message.offer });
    const answer = await rtc.createAnswer();
    await rtc.setLocalDescription(answer);
    if (answer.sdp) {
      send({ answer: answer.sdp });
      console.log("answer sent");
      console.log(answer.sdp);
    }
  } else if ("answer" in message) {
    console.log("answer received");
    console.log(message.answer);
    await rtc.setRemoteDescription({ type: "answer", sdp: message.answer });
  } else if ("id" in message) {
    console.log("id received");
    console.log(message.id);
  } else if ("peers" in message) {
    console.log("peers received");
    console.log(message.peers);
  } else if ("peer" in message) {
    console.log("peer received");
    console.log(message.peer);
  }
});

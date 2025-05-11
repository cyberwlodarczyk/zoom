type ServerMessage =
  | { candidate: RTCIceCandidateInit }
  | { offer: { sdp: string } }
  | { answer: { sdp: string } };

type PeerMessage =
  | { candidate: RTCIceCandidateInit }
  | { offer: { sdp: string } }
  | { answer: { sdp: string } }
  | { name: string }
  | { pli: boolean };

const app = document.getElementById("app")!;
const rtc = new RTCPeerConnection();
const signal = new WebSocket("http://localhost:3000/signal");

const addVideo = (stream: MediaStream) => {
  const video = document.createElement("video");
  video.srcObject = stream;
  video.autoplay = true;
  app.appendChild(video);
};

const send = (message: PeerMessage) => {
  signal.send(JSON.stringify(message));
};

signal.addEventListener("open", async () => {
  const stream = await navigator.mediaDevices.getUserMedia({ video: true });
  const [track] = stream.getTracks();
  rtc.addTrack(track, stream);
  addVideo(stream);
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
    addVideo(stream);
    console.log("requesting pli");
    send({ pli: true });
  });
  const offer = await rtc.createOffer();
  await rtc.setLocalDescription(offer);
  if (offer.sdp) {
    send({ offer: { sdp: offer.sdp } });
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
  } else if ("answer" in message) {
    console.log("answer received");
    console.log(message.answer.sdp);
    await rtc.setRemoteDescription({ type: "answer", sdp: message.answer.sdp });
  } else {
    console.log("offer received");
    console.log(message.offer.sdp);
    await rtc.setRemoteDescription({ type: "offer", sdp: message.offer.sdp });
    const answer = await rtc.createAnswer();
    await rtc.setLocalDescription(answer);
    if (answer.sdp) {
      send({ answer: { sdp: answer.sdp } });
      console.log("answer sent");
      console.log(answer.sdp);
    }
  }
});

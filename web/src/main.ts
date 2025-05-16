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
  | { pli: number };

type MediaKind = "audio" | "video";

const app = document.getElementById("app")!;

function createElement<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]>
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  Object.assign(el, props);
  return el;
}

function addMedia(stream: MediaStream, kind: MediaKind) {
  const media = createElement(kind, { srcObject: stream, autoplay: true });
  app.append(media);
}

function addName(text: string) {
  const name = createElement("p", { textContent: text });
  app.append(name);
}

function getPeerIdFromTrackId(id: string) {
  return parseInt(id.split(" ")[0]);
}

function join(name: string, code: string) {
  const rtc = new RTCPeerConnection();
  const signal = new WebSocket(`http://localhost:3000/signal?code=${code}`);
  const peers: ServerMessagePeer[] = [];
  function send(message: PeerMessage) {
    signal.send(JSON.stringify(message));
  }
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
    const peer = peers.find(
      (peer) => peer.id === getPeerIdFromTrackId(event.track.id)
    );
    addMedia(stream, event.track.kind as "audio" | "video");
    if (peer && event.track.kind === "video") {
      addName(peer.name);
    }
  });
  setInterval(async () => {
    const stats = await rtc.getStats();
    stats.forEach((report: RTCInboundRtpStreamStats) => {
      if (
        report.type === "inbound-rtp" &&
        report.kind === "video" &&
        report.framesDecoded === 0
      ) {
        send({ pli: getPeerIdFromTrackId(report.trackIdentifier) });
      }
    });
  }, 100);
  signal.addEventListener("open", async () => {
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: true,
      video: true,
    });
    stream.getTracks().forEach((track) => rtc.addTrack(track, stream));
    addMedia(stream, "video");
    addName(name);
    const offer = await rtc.createOffer();
    await rtc.setLocalDescription(offer);
    if (offer.sdp) {
      send({ offer: offer.sdp });
      console.log("offer sent");
      console.log(offer.sdp);
      send({ name });
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
      peers.push(...message.peers);
    } else if ("peer" in message) {
      console.log("peer received");
      console.log(message.peer);
      peers.push(message.peer);
    }
  });
}

const nameLabel = createElement("label", {
  htmlFor: "name",
  textContent: "Name",
});
const nameInput = createElement("input", {
  type: "text",
  id: "name",
  name: "name",
  required: true,
  placeholder: "John",
});
const codeLabel = createElement("label", {
  htmlFor: "code",
  textContent: "Code",
});
const codeInput = createElement("input", {
  type: "text",
  id: "code",
  name: "code",
  required: true,
  placeholder: "abc-def-ghi",
});
const button = createElement("button", { type: "submit", textContent: "Join" });
const form = createElement("form", {});
form.append(nameLabel, nameInput, codeLabel, codeInput, button);
form.addEventListener("submit", (event) => {
  event.preventDefault();
  join(nameInput.value, codeInput.value);
});
app.append(form);

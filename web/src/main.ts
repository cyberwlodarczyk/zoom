import { Mutex } from "async-mutex";

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
  | { peerJoined: ServerMessagePeer }
  | { peerLeft: number };

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

function createJoinForm(onSubmit: (name: string, code: string) => void) {
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
  const submitButton = createElement("button", {
    type: "submit",
    textContent: "Join",
  });
  const joinForm = createElement("form", {});
  joinForm.append(nameLabel, nameInput, codeLabel, codeInput, submitButton);
  joinForm.addEventListener("submit", (event) => {
    event.preventDefault();
    onSubmit(nameInput.value, codeInput.value);
  });
  return joinForm;
}

const joinForm = createJoinForm(join);
const mediaContainer = createElement("div", {});
const leaveButton = createElement("button", { textContent: "Leave" });
app.append(joinForm);

function addMedia(
  stream: MediaStream,
  kind: MediaKind,
  id: number,
  muted: boolean
) {
  const media = createElement(kind, {
    srcObject: stream,
    autoplay: true,
    muted,
  });
  media.dataset.id = id.toString();
  mediaContainer.append(media);
}

function addName(text: string, id: number) {
  const name = createElement("p", {
    textContent: text,
  });
  name.dataset.id = id.toString();
  mediaContainer.append(name);
}

function removeMediaAndName(id: number) {
  for (const child of [...mediaContainer.children]) {
    if (child instanceof HTMLElement && child.dataset.id === id.toString()) {
      mediaContainer.removeChild(child);
    }
  }
}

function getPeerIdFromTrackId(id: string) {
  return parseInt(id.split(" ")[0]);
}

async function join(name: string, code: string) {
  app.removeChild(joinForm);
  app.append(leaveButton, mediaContainer);
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: true,
    video: true,
  });
  const mutex = new Mutex();
  const rtc = new RTCPeerConnection();
  const signal = new WebSocket(`http://localhost:3000/signal?code=${code}`);
  let id: number | null = null;
  let peers: ServerMessagePeer[] = [];
  let pendingCandidates: RTCIceCandidateInit[] = [];
  function send(message: PeerMessage) {
    signal.send(JSON.stringify(message));
  }
  async function setRemoteDescription(description: RTCSessionDescriptionInit) {
    await rtc.setRemoteDescription(description);
    for (const candidate of pendingCandidates) {
      await rtc.addIceCandidate(candidate);
    }
    pendingCandidates = [];
  }
  leaveButton.addEventListener("click", () => {
    signal.close();
  });
  rtc.addEventListener("connectionstatechange", () => {
    if (rtc.connectionState === "connected") {
      console.log("connection established");
    }
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
    const peerId = getPeerIdFromTrackId(event.track.id);
    const peer = peers.find((peer) => peer.id === peerId);
    addMedia(stream, event.track.kind as "audio" | "video", peerId, false);
    if (peer && event.track.kind === "video") {
      addName(peer.name, peerId);
    }
  });
  signal.addEventListener("open", async () => {
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
    stream.getTracks().forEach((track) => rtc.addTrack(track, stream));
    send({ name });
    console.log("name sent");
    await mutex.runExclusive(async () => {
      const offer = await rtc.createOffer();
      await rtc.setLocalDescription(offer);
      send({ offer: offer.sdp! });
      console.log("offer sent");
      console.log(offer.sdp);
    });
  });
  signal.addEventListener("message", async (event) => {
    const message = JSON.parse(event.data) as ServerMessage;
    if ("candidate" in message) {
      console.log("new remote ice candidate");
      await mutex.runExclusive(async () => {
        if (rtc.remoteDescription) {
          await rtc.addIceCandidate(message.candidate);
        } else {
          pendingCandidates.push(message.candidate);
        }
      });
    } else if ("offer" in message) {
      await mutex.runExclusive(async () => {
        console.log("offer received");
        console.log(message.offer);
        if (rtc.signalingState === "have-local-offer") {
          console.log("rollback");
          await rtc.setLocalDescription({ type: "rollback" });
        }
        await setRemoteDescription({ type: "offer", sdp: message.offer });
        const answer = await rtc.createAnswer();
        await rtc.setLocalDescription(answer);
        send({ answer: answer.sdp! });
        console.log("answer sent");
        console.log(answer.sdp);
      });
    } else if ("answer" in message) {
      await mutex.runExclusive(async () => {
        console.log("answer received");
        console.log(message.answer);
        await setRemoteDescription({ type: "answer", sdp: message.answer });
      });
    } else if ("id" in message) {
      console.log("id received");
      console.log(message.id);
      if (!id) {
        id = message.id;
        addMedia(stream, "video", id, true);
        addName(name, id);
      }
    } else if ("peers" in message) {
      console.log("peers received");
      console.log(message.peers);
      peers.push(...message.peers);
    } else if ("peerJoined" in message) {
      console.log("peer joined");
      console.log(message.peerJoined);
      peers.push(message.peerJoined);
    } else if ("peerLeft" in message) {
      console.log("peer left");
      console.log(message.peerLeft);
      peers = peers.filter((peer) => peer.id !== message.peerLeft);
      removeMediaAndName(message.peerLeft);
    }
  });
  signal.addEventListener("close", () => {
    console.log("connection closed");
    rtc.close();
    stream.getTracks().forEach((track) => track.stop());
    app.removeChild(leaveButton);
    app.removeChild(mediaContainer);
    mediaContainer.innerHTML = "";
    app.appendChild(joinForm);
  });
}

const ws = new WebSocket("http://localhost:3000/ws");

ws.addEventListener("open", () => {
  console.log("connection established");
  let x = 1;
  let id = setInterval(() => {
    const msg = `${x}`;
    ws.send(msg);
    console.log(`message sent: ${msg}`);
    if (x === 5) {
      clearInterval(id);
      ws.close();
    }
    x++;
  }, 500);
});

ws.addEventListener("message", (event) => {
  console.log(`message received: ${event.data}`);
});

ws.addEventListener("close", () => {
  console.log("connection closed");
});

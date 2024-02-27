import { invoke } from "@tauri-apps/api/tauri";
import "./App.css";
import { useState } from "react";

function App() {
  const [response, setResponse] = useState("");
  async function start_proxy() {
    try {
      const res = await invoke<string>("start_proxy");
      setResponse(res);
    } catch (error) {
      setResponse((error as Error).message);
    }
  }

  async function stop_proxy() {
    try {
      const res = await invoke<string>("stop_proxy");
      setResponse(res);
    } catch (error) {
      setResponse((error as Error).message);
    }
  }

  return (
    <div className="container">
      <h1>Welcome to Tauri!</h1>
      {response && <p>{response}</p>}
      <button onClick={start_proxy}>启动 Proxy</button>
      <button onClick={stop_proxy}>停止 Proxy</button>
    </div>
  );
}

export default App;

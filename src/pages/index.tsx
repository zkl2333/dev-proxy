import { useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { Button } from "@/components/ui/button";

function Index() {
  const [response, setResponse] = useState("");
  async function start_proxy() {
    try {
      const res = await invoke<string>("start_proxy");
      setResponse(res);
    } catch (error) {
      setResponse(error as string);
    }
  }

  async function stop_proxy() {
    try {
      const res = await invoke<string>("stop_proxy");
      setResponse(res);
    } catch (error) {
      setResponse(error as string);
    }
  }

  return (
    <div className="space-y-2">
      <div className="flex gap-4">
        <Button size="sm" onClick={start_proxy}>
          启动 Proxy
        </Button>
        <Button size="sm" variant="destructive" onClick={stop_proxy}>
          停止 Proxy
        </Button>
      </div>
      <div>{response && <p>{response}</p>}</div>
    </div>
  );
}

export default Index;

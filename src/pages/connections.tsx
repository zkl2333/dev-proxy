import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { ScrollArea } from "@/components/ui/scroll-area";

interface Session {
  id: string;
  state: string;
  protocol: string;
  target_addr: any;
}

const uesInterval = (callback: () => void, delay: number) => {
  const savedCallback = useRef<() => void>();

  useEffect(() => {
    savedCallback.current = callback;
  }, [callback]);

  useEffect(() => {
    const tick = () => {
      savedCallback.current!();
    };
    if (delay !== null) {
      const id = setInterval(tick, delay);
      return () => clearInterval(id);
    }
  }, [delay]);
};

const Connections = () => {
  const [sessions, setSessions] = useState<Session[]>([]);

  async function get_proxy_connections() {
    try {
      const res = await invoke<Session[]>("get_proxy_connections");
      console.log(res);
      setSessions(res);
    } catch (error) {
      console.log(error);
    }
  }

  useEffect(() => {
    get_proxy_connections();
  }, []);

  uesInterval(() => {
    get_proxy_connections();
  }, 500);

  return (
    <div className="h-full flex flex-col overflow-hidden">
      <h1 className="p-4 text-2xl font-bold">连接 ({sessions.length})</h1>
      <ScrollArea className="flex-1 p-4">
        <div className="space-y-4">
          {sessions.map((session) => (
            <div key={session.id} className="p-2 border">
              <p>id: {session.id}</p>
              <p>state: {session.state}</p>
              <p>protocol: {session.protocol}</p>
              <p>target_addr: {JSON.stringify(session.target_addr)}</p>
            </div>
          ))}
        </div>
      </ScrollArea>
    </div>
  );
};

export default Connections;

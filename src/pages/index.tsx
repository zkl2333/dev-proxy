import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import toast from "react-hot-toast";
import { Switch } from "@/components/ui/switch";

function Index() {
  const [proxyState, setProxyState] = useState(false);
  async function start_proxy() {
    try {
      await invoke<string>("start_proxy");
      get_proxy_state();
    } catch (error) {
      toast.error(error as string);
    }
  }

  async function stop_proxy() {
    try {
      await invoke<string>("stop_proxy");
      get_proxy_state();
    } catch (error) {
      toast.error(error as string);
    }
  }

  async function get_proxy_state() {
    try {
      const res = await invoke<boolean>("get_proxy_state");
      setProxyState(res);
    } catch (error) {
      toast.error(error as string);
    }
  }

  useEffect(() => {
    get_proxy_state();
  }, []);

  return (
    <div className="space-y-2">
      <div className="flex flex-col gap-4">
        <div className="flex items-center py-2 px-4 rounded-md justify-between hover:bg-accent hover:text-accent-foreground">
          代理服务
          <Switch
            id="airplane-mode"
            checked={proxyState}
            onCheckedChange={(checked) => {
              if (checked) {
                start_proxy();
              } else {
                stop_proxy();
              }
            }}
          />
        </div>
      </div>
    </div>
  );
}

export default Index;

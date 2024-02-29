import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/tauri";

const Connections = () => {
  async function get_proxy_connections() {
    try {
      const res = await invoke<boolean>("get_proxy_connections");
      console.log(res);
    } catch (error) {
      console.log(error);
    }
  }

  useEffect(() => {
    // get_proxy_connections();
  }, []);
  return <div>connections</div>;
};

export default Connections;

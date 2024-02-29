import { Suspense } from "react";
import { Sidebar } from "./components/sidebar/sidebar";
import { useRoutes } from "react-router";
import { Toaster } from "react-hot-toast";
import "./App.css";

import routes from "~react-pages";

const menu = [
  {
    name: "通用",
    path: "/",
  },
  {
    name: "连接",
    path: "/connections",
  },
  {
    name: "日志",
    path: "/contact",
  },
];

function App() {
  return (
    <div className="flex h-full">
      <Sidebar menu={menu} className="w-52 bg-background" />
      <div className="border-l p-4 w-full">
        <Suspense fallback={<p>Loading...</p>}>{useRoutes(routes)}</Suspense>
      </div>
      <Toaster position="top-right" />
    </div>
  );
}

export default App;
